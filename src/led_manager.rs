use std::{
    env,
    io::{BufWriter, ErrorKind::WouldBlock, IoSlice, Write},
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

use crate::{LedConfig, LedState, Task, utils::ManagerData};

enum ConnectionType<'a> {
    Udp(&'a mut Option<UdpSocket>),
    Serial(&'a mut dyn SerialPort),
}

enum SendCommandArgs<'a> {
    Manager(&'a mut ManagerData),
    ChannelConfigState(ConnectionType<'a>, &'a LedConfig, &'a mut LedState),
}

fn dispatch_threads(manager: &mut ManagerData) -> Vec<Sender<Task>> {
    let config = manager.config.clone();
    let mut channels = Vec::new();
    let handles = &mut manager.state.all_thread_handles;

    for path in config.serial_port_paths.clone() {
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

        let baud_rate = config.baud_rate;
        let serial_read_timeout = config.serial_read_timeout;
        let keepalive = Arc::clone(&manager.state.keepalive);

        let mut serial_port = match serialport::new(&path, baud_rate)
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

            let mut queue_total_lengths: u32 = 0;

            if !owned_state.queue_lengths.is_empty() {
                for n in owned_state
                    .queue_lengths
                    .iter()
                    .take((owned_state.queue_lengths.len() - 1) + 1)
                {
                    queue_total_lengths += owned_state.queue_lengths[*n as usize] as u32;
                }
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

    if let Some(use_queue) = manager.config.use_queue {
        if !use_queue {
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

            if let Some(no_controller) = manager.config.no_controller
                && !no_controller
            {
                send_color_command(SendCommandArgs::Manager(&mut manager), n, r, g, b);
            }
        } else if manager.state.led_thread_channels.is_empty() {
            manager.state.led_thread_channels = dispatch_threads(&mut manager);
        } else {
            let mut n = n;

            let leds_per_strip = manager.config.num_led / manager.config.num_strips;

            for index in 1..manager.config.num_strips + 1 {
                if (n as u32) < index * leds_per_strip && n as u32 >= (index - 1) * leds_per_strip {
                    // Determines which strip to send the index instruction to.
                    n = if index > 1 {
                        n - (leds_per_strip * (index - 1)) as u16
                    } else {
                        n
                    };

                    manager.state.led_thread_channels[(index - 1) as usize]
                        .send(Task {
                            command: (n, r, g, b),
                            controller_queue_length: None,
                        })
                        .expect("Could not dispatch task to a worker thread!");

                    break;
                }
            }
        }
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
                    if manager.config.communication_mode == 1 {
                        ConnectionType::Udp(&mut manager.io.udp_socket)
                    } else {
                        // Establish a serial connection on each serial port
                        if manager.io.serial_port.is_empty() {
                            for path in manager.config.serial_port_paths.clone().iter() {
                                let baud_rate = manager.config.baud_rate;
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

                        // Determine the correct index and serial port
                        let leds_per_strip = manager.config.num_led / manager.config.num_strips;
                        let mut serial_port = None;

                        for index in 1..manager.config.num_strips + 1 {
                            if (n as u32) < index * leds_per_strip
                                && n as u32 >= (index - 1) * leds_per_strip
                            {
                                // Determines which strip to send the index instruction to.
                                n = if index > 1 {
                                    n - (leds_per_strip * (index - 1)) as u16
                                } else {
                                    n
                                };
                                serial_port =
                                    Some(manager.io.serial_port[(index - 1) as usize].as_mut());

                                break;
                            }
                        }

                        ConnectionType::Serial(serial_port.expect("Could not determine the correct index and serial port to send LED command on!"))
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
        ConnectionType::Udp(udp_socket) => {
            udp_socket.get_or_insert_with(|| {
                debug!("Binding to 0.0.0.0:{}", config.port);
                UdpSocket::bind(format!("0.0.0.0:{}", config.port))
                    .unwrap_or_else(|e| panic!("Could not bind: {e}"))
            });

            match udp_socket.as_mut() {
                Some(udp_socket) => {
                    udp_socket
                        .set_read_timeout(Some(Duration::new(
                            0,
                            config.udp_read_timeout.unwrap_or(5) * 1000000,
                        )))
                        .expect("set_read_timeout call failed");

                    let mut bytes: [u8; 5] = [0; 5];
                    bytes[0..2].copy_from_slice(&n.to_le_bytes());
                    bytes = [bytes[0], bytes[1], r, g, b];
                    // debug!("Sending {:?}", bytes);
                    match udp_socket.send_to(&bytes, format!("{}:{}", config.host, config.port)) {
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
                        Err(ref e) if e.kind() == WouldBlock => {
                            if state.failures >= config.con_fail_limit.unwrap_or(5) {
                                error!("Too many consecutive communication failures, exiting.");
                                process::exit(1);
                            }
                            warn!(
                                "UDP timeout reached! Will resend packet, but won't wait for response!"
                            );
                            match udp_socket
                                .send_to(&bytes, format!("{}:{}", config.host, config.port))
                            {
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

                    if buf == [42, 41, 44] {
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
            let mut msg: [u8; 7] = [0; 7];
            msg[2..4].copy_from_slice(&n.to_le_bytes());
            msg = [0xFF, 0xBB, msg[2], msg[3], r, g, b]; // 0xFF & 0xBB indicate a start of packet.
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
