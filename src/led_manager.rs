use std::{
    env,
    io::{BufWriter, ErrorKind, IoSlice, Write},
    net::UdpSocket,
    path::{Path, PathBuf},
    process,
    sync::{Arc, Mutex, atomic::Ordering},
    thread,
    time::{Duration, SystemTime},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use log::{debug, error, info, warn};
use serialport::SerialPort;

use crate::{
    LedConfig, LedState, Task,
    protocol::{
        COMMUNICATION_MODE_SERIAL, COMMUNICATION_MODE_SVLED_UDP, COMMUNICATION_MODE_WLED,
        serial_color_packet, svled_udp_color_packet, wled_dnrgb_color_packet,
    },
    utils::ManagerData,
};

#[derive(Clone, Copy)]
enum UdpProtocol {
    Svled,
    WledDnrgb,
}

enum ConnectionType<'a> {
    Udp(&'a mut Option<UdpSocket>, UdpProtocol),
    Serial(&'a mut dyn SerialPort),
}

enum SendCommandArgs<'a> {
    Manager(&'a mut ManagerData),
    ChannelConfigState(ConnectionType<'a>, &'a LedConfig, &'a mut LedState),
}

// Queued workers are intentionally serial-only. The custom UDP protocol relies
// on a reply per packet, while WLED is best served by future frame batching.
fn dispatch_threads(manager: &mut ManagerData) -> Vec<Sender<Task>> {
    let config = manager.config.clone();
    let mut channels = Vec::new();
    let handles = &mut manager.state.all_thread_handles;

    for path in config
        .serial_port_paths
        .as_ref()
        .expect("serial_port_paths must be configured before dispatching serial workers")
    {
        let (tx, rx): (Sender<Task>, Receiver<Task>) = bounded(config.queue_size.unwrap_or(20));
        channels.push(tx);

        let owned_led_config = LedConfig {
            skip_confirmation: config.skip_confirmation,
            no_controller: config.no_controller,
            unity_controls_recording: config.unity_controls_recording,
            port: config.port,
            communication_mode: config.communication_mode,
            num_led: config.num_led,
            num_strips: config.num_strips,
            serial_read_timeout: config.serial_read_timeout,
            udp_read_timeout: config.udp_read_timeout,
            host: config.host,
            con_fail_limit: config.con_fail_limit,
            print_send_back: config.print_send_back,
            serial_port_paths: config.serial_port_paths.clone(),
        };

        let baud_rate = config.baud_rate.unwrap();
        let serial_read_timeout = config.serial_read_timeout;
        let keepalive = Arc::clone(&manager.state.keepalive);

        let mut serial_port = match serialport::new(path, baud_rate)
            .timeout(Duration::from_millis(
                serial_read_timeout.unwrap_or(200).into(),
            ))
            .open()
        {
            Ok(port) => port,
            Err(e) => panic!("Could not open {path}: {e}"),
        };

        debug!("Dispatching thread!");
        let my_keepalive = Arc::clone(&keepalive);

        handles.push(thread::spawn(move || {
            let mut owned_state = LedState {
                failures: 0,
                queue_lengths: Vec::new(),
                wled_colors: Vec::new(),
            };

            while my_keepalive.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(cmd) => {
                        send_color_command(
                            SendCommandArgs::ChannelConfigState(
                                ConnectionType::Serial(&mut *serial_port),
                                &owned_led_config,
                                &mut owned_state,
                            ),
                            cmd.command.0,
                            cmd.command.1,
                            cmd.command.2,
                            cmd.command.3,
                        );
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // just loop again and check `keepalive`
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }

            if !owned_state.queue_lengths.is_empty() {
                let queue_total_lengths: u32 = owned_state
                    .queue_lengths
                    .iter()
                    .map(|length| u32::from(*length))
                    .sum();
                debug!(
                    "Average queue length: {}",
                    queue_total_lengths / owned_state.queue_lengths.len() as u32
                );
                debug!("socket worker thread exiting!");
            }
        }));
    }

    channels
}

// TODO: Don't resend redundant packets
pub fn set_color(manager_guard: &Arc<Mutex<ManagerData>>, n: u16, r: u8, g: u8, b: u8) {
    let mut manager = manager_guard.lock().unwrap();

    let record_data;
    let record_esp_data;

    // Unity controls if we record commands to a file using a file in the tmp dir
    if manager.config.unity_controls_recording {
        let unity_start_anim_path: PathBuf = [env::temp_dir().to_str().unwrap(), "start_animate"]
            .iter()
            .collect();
        record_data = Path::new(&unity_start_anim_path.into_os_string()).exists();

        let unity_start_anim_byte_path: PathBuf =
            [env::temp_dir().to_str().unwrap(), "start_animate_byte"]
                .iter()
                .collect();
        record_esp_data = Path::new(&unity_start_anim_byte_path.into_os_string()).exists();
    } else {
        record_data = manager.config.record_data;
        record_esp_data = manager.config.record_esp_data;
    }

    if manager.state.first_run {
        manager.state.first_run = false;
        manager.state.call_time = SystemTime::now();
    }

    // If we want to record data
    if record_data || record_esp_data {
        if record_data && manager.io.data_file_buf.is_none() {
            manager.io.data_file_buf = Some(BufWriter::new(
                match crate::utils::check_and_create_file(&manager.config.record_data_file) {
                    Ok(file) => file,
                    Err(e) => {
                        panic!(
                            "Could not open {} for writing animation: {}",
                            manager.config.record_data_file.display(),
                            e
                        );
                    }
                },
            ));
        }
        if record_esp_data && manager.io.esp_data_file_buf.is_none() {
            manager.io.esp_data_file_buf = Some(BufWriter::new(
                match crate::utils::check_and_create_file(&manager.config.record_esp_data_file) {
                    Ok(file) => file,
                    Err(e) => {
                        panic!(
                            "Could not open {} for writing animation: {}",
                            manager.config.record_esp_data, e
                        )
                    }
                },
            ));
        }
        let end = SystemTime::now();
        match end.duration_since(manager.state.call_time) {
            Ok(duration) => {
                manager.state.call_time = SystemTime::now(); // Reset timer
                if record_data {
                    match manager.io.data_file_buf.as_mut() {
                        Some(data_file_buf) => {
                            let millis = duration.as_millis();
                            if millis >= 3 {
                                writeln!(data_file_buf, "T:{}", millis)
                                    .expect("Could not write to data_file_buf!");
                            }
                            writeln!(data_file_buf, "{n}|{r}|{g}|{b}")
                                .expect("Could not write to data_file_buf!");
                        }
                        None => error!(
                            "record_data is true, but data_file_buf is None! Something has gone very wrong, please report this."
                        ),
                    }
                }
                if record_esp_data {
                    match manager.io.esp_data_file_buf.as_mut() {
                        Some(esp_data_file_buf) => {
                            let mut millis = duration.as_millis();

                            while millis > 255 {
                                // Delay marker + max duration
                                write!(esp_data_file_buf, "0xFE, 0xFF, ")
                                    .expect("Failed to write delay");
                                millis -= 255;
                            }
                            if millis > 0 {
                                write!(esp_data_file_buf, "0xFE, {millis:#04X}, ")
                                    .expect("Failed to write delay");
                            }

                            let n_bytes = n.to_le_bytes();
                            write!(
                                esp_data_file_buf,
                                "0x{0:02X}, 0x{1:02X}, 0x{2:02X}, 0x{3:02X}, 0x{4:02X}, ",
                                n_bytes[0], n_bytes[1], r, g, b
                            )
                            .expect("Failed to write LED data");
                        }
                        None => error!(
                            "record_esp_data is true, but esp_data_file_buf is None!, Something has gone very wrong, please report this."
                        ),
                    }
                }
            }
            Err(e) => println!("Error: {e}"),
        }
    }

    if manager.config.no_controller.unwrap_or(false) {
        return;
    }

    if u32::from(n) >= manager.config.num_led {
        warn!(
            "Ignoring LED index {n}; configured LED count is {}",
            manager.config.num_led
        );
        return;
    }

    let use_serial_queue = manager.config.use_queue.unwrap_or(false)
        && manager.config.communication_mode == COMMUNICATION_MODE_SERIAL;

    if use_serial_queue {
        if manager.state.led_thread_channels.is_empty() {
            manager.state.led_thread_channels = dispatch_threads(&mut manager);
        }

        let leds_per_strip = manager.config.num_led / manager.config.num_strips;
        let controller_index = u32::from(n) / leds_per_strip;
        let local_index = u32::from(n) - controller_index * leds_per_strip;

        manager.state.led_thread_channels[controller_index as usize]
            .send(Task {
                command: (local_index as u16, r, g, b),
                controller_queue_length: None,
            })
            .expect("Could not dispatch task to a worker thread!");
    } else {
        if manager.config.led_config.is_none() {
            manager.config.led_config = Some(LedConfig {
                skip_confirmation: manager.config.skip_confirmation,
                unity_controls_recording: manager.config.unity_controls_recording,
                no_controller: manager.config.no_controller,
                port: manager.config.port,
                communication_mode: manager.config.communication_mode,
                num_led: manager.config.num_led,
                num_strips: manager.config.num_strips,
                serial_read_timeout: manager.config.serial_read_timeout,
                udp_read_timeout: manager.config.udp_read_timeout,
                host: manager.config.host,
                con_fail_limit: manager.config.con_fail_limit,
                print_send_back: manager.config.print_send_back,
                serial_port_paths: manager.config.serial_port_paths.clone(),
            });
        }

        send_color_command(SendCommandArgs::Manager(&mut manager), n, r, g, b);
    }
}

// In the case where we are not using queues and threads, we can just pass manager directly, since we don't care about locks.
// Otherwise, we need to be able to pass the channel, config, and state, all of which are declared within each thread itself, and thus
// will never block each other.
fn send_color_command(manager_or_config: SendCommandArgs, n: u16, r: u8, g: u8, b: u8) {
    let mut n = n;

    let (channel, config, state) = {
        match manager_or_config {
            SendCommandArgs::ChannelConfigState(channel, config, state) => (channel, config, state),
            SendCommandArgs::Manager(manager) => {
                let channel: ConnectionType = {
                    match manager.config.communication_mode {
                        COMMUNICATION_MODE_SVLED_UDP => {
                            ConnectionType::Udp(&mut manager.io.udp_socket, UdpProtocol::Svled)
                        }
                        COMMUNICATION_MODE_WLED => {
                            ConnectionType::Udp(&mut manager.io.udp_socket, UdpProtocol::WledDnrgb)
                        }
                        COMMUNICATION_MODE_SERIAL => {
                            // Establish a serial connection on each serial port
                            if manager.io.serial_port.is_empty() {
                                for path in manager
                                    .config
                                    .serial_port_paths
                                    .as_ref()
                                    .unwrap()
                                    .clone()
                                    .iter()
                                {
                                    let baud_rate = manager.config.baud_rate.unwrap();
                                    let serial_read_timeout = manager.config.serial_read_timeout;
                                    manager.io.serial_port.push(
                                        match serialport::new(path, baud_rate)
                                            .timeout(Duration::from_millis(
                                                serial_read_timeout.unwrap_or(200).into(),
                                            ))
                                            .open()
                                        {
                                            Ok(port) => port,
                                            Err(e) => panic!("Could not open {path}: {e}"),
                                        },
                                    );
                                }
                            }

                            // Preserve the existing equal-size controller mapping while making
                            // the route identical in queued and direct serial modes.
                            let leds_per_strip = manager.config.num_led / manager.config.num_strips;
                            let controller_index = u32::from(n) / leds_per_strip;
                            n = (u32::from(n) % leds_per_strip) as u16;

                            ConnectionType::Serial(
                                manager.io.serial_port[controller_index as usize].as_mut(),
                            )
                        }
                        mode => unreachable!("communication mode {mode} was not validated"),
                    }
                };

                (
                    channel,
                    manager.config.led_config.as_ref().expect("manager.config.led_config should have been set earlier by parent caller, but it is None!"),
                    &mut manager.state.led_state,
                )
            }
        }
    };

    match channel {
        ConnectionType::Udp(udp_socket, udp_protocol) => {
            let port = config
                .port
                .expect("UDP port should be validated before use");
            let bind_port = match udp_protocol {
                // Preserve the custom controller's existing source-port behavior.
                UdpProtocol::Svled => port,
                // WLED sends no acknowledgement, so an ephemeral source port is sufficient.
                UdpProtocol::WledDnrgb => 0,
            };

            udp_socket.get_or_insert_with(|| {
                debug!("Binding UDP sender to 0.0.0.0:{bind_port}");
                UdpSocket::bind(format!("0.0.0.0:{bind_port}"))
                    .unwrap_or_else(|e| panic!("Could not bind: {e}"))
            });

            match udp_socket.as_mut() {
                Some(udp_socket) => {
                    let destination = format!(
                        "{}:{}",
                        config
                            .host
                            .expect("UDP host should be validated before use"),
                        port
                    );

                    if let UdpProtocol::WledDnrgb = udp_protocol {
                        let num_led = config.num_led as usize;
                        if state.wled_colors.len() != num_led {
                            state.wled_colors = vec![[0, 0, 0]; num_led];
                        }

                        let index = usize::from(n);
                        state.wled_colors[index] = [r, g, b];

                        // Current WLED releases reject a seven-byte (one-pixel)
                        // DNRGB datagram. Include a neighboring pixel from the
                        // local color buffer while keeping the requested pixel
                        // and every packet field exact.
                        let (start, first, second) = if index + 1 < num_led {
                            (n, state.wled_colors[index], state.wled_colors[index + 1])
                        } else if index > 0 {
                            (
                                n - 1,
                                state.wled_colors[index - 1],
                                state.wled_colors[index],
                            )
                        } else {
                            (0, state.wled_colors[index], [0, 0, 0])
                        };
                        let bytes = wled_dnrgb_color_packet(start, first, second);
                        if let Err(e) = udp_socket.send_to(&bytes, &destination) {
                            error!("Could not write WLED DNRGB packet: {e}");
                        }
                        return;
                    }

                    udp_socket
                        .set_read_timeout(Some(Duration::new(
                            0,
                            config.udp_read_timeout.unwrap_or(5) * 1000000,
                        )))
                        .expect("set_read_timeout call failed");

                    let bytes = svled_udp_color_packet(n, r, g, b);
                    match udp_socket.send_to(&bytes, &destination) {
                        Ok(_) => {}
                        Err(e) => {
                            error!(
                                "Could not write bytes to UDP socket: {e}, trying to continue anyway"
                            )
                        }
                    }
                    let mut buf = [0; 3];
                    let udp_result = udp_socket.recv(&mut buf);

                    match udp_result {
                        Ok(_size) => {
                            state.failures = 0; // Reset consecutive failure count
                        }
                        Err(ref e)
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                        {
                            if state.failures >= config.con_fail_limit.unwrap_or(5) {
                                error!("Too many consecutive communication failures, exiting.");
                                process::exit(1);
                            }
                            warn!(
                                "UDP timeout reached! Will resend packet, but won't wait for response!"
                            );
                            match udp_socket.send_to(&bytes, &destination) {
                                Ok(_) => {}
                                Err(e) => {
                                    error!(
                                        "Could not write bytes to UDP socket: {e}, trying to continue anyway"
                                    )
                                }
                            }
                            state.failures += 1
                        }
                        Err(e) => {
                            error!("An error occurred sending data: {e}");
                        }
                    }

                    if buf == *b"BAD" {
                        // "BAD" - indicates the remote device reported a malformed packet
                        warn!("ESP reported a malformed packet!"); // TODO: Should we resend packet and not wait?
                        state.failures += 1
                    }
                }
                None => {
                    panic!("Could not send packet as manager.udp_socket does not exist!")
                }
            };
        }

        ConnectionType::Serial(serial_port) => {
            // This will not figure out the correct strip/index to send to, and will send the index unmodified.
            let msg = serial_color_packet(n, r, g, b);
            match serial_port.write_vectored(&[IoSlice::new(&msg)]) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "Could not write bytes to {}: {}",
                        serial_port.name().unwrap(),
                        e
                    )
                }
            }

            if let Some(true) = config.print_send_back {
                let mut serial_buf: Vec<u8> = vec![0; 7];

                let read_result = serial_port.read(serial_buf.as_mut_slice());

                match read_result {
                    Ok(_size) => {
                        info!(
                            "print_send_back returned {:?}",
                            String::from_utf8_lossy(&serial_buf)
                        );
                    }
                    Err(e) => {
                        error!("print_send_back could not read serial port: {e}");
                    }
                };
            } else if !config.skip_confirmation.unwrap_or(false) {
                let mut failures = 0;
                let mut serial_buf: Vec<u8> = vec![0; 1];

                loop {
                    match serial_port.read_exact(serial_buf.as_mut_slice()) {
                        Ok(_) => break,
                        Err(e) => {
                            warn!("Could not read from {}: {}", serial_port.name().unwrap(), e)
                        }
                    }
                    failures += 1;

                    if failures >= config.serial_read_timeout.unwrap_or(200) {
                        error!(
                            "Did not receive confirmation byte after {}ms! Ignoring and continuing anyway!",
                            config.serial_read_timeout.unwrap_or(200)
                        );
                        break;
                    }
                }
                state.queue_lengths.push(serial_buf[0]);
            }
        }
    }
}
