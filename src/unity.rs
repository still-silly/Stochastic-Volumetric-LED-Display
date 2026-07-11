use std::{
    cmp::max,
    collections::HashMap,
    error::Error,
    fs::File,
    io::prelude::*,
    net::{Ipv4Addr, TcpStream, UdpSocket},
    str,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use log::{debug, error, info, warn};
use opencv::{
    core::{Mat, Point, Scalar},
    imgproc::{self, LINE_8},
    videoio::{
        self, CAP_PROP_FRAME_HEIGHT, CAP_PROP_FRAME_WIDTH, VideoCaptureTrait,
        VideoCaptureTraitConst,
    },
};

use crate::{
    Config, GetEventsFrameBuffer, IOHandles, LedState, ManagerData, ManagerState, PosEntry,
    RuntimeConfig, UnityOptions, VisionData, led_manager, scan::get_cam,
};

pub fn signal_restart(unity_ip: Ipv4Addr, unity_port: u32) {
    let mut stream = match TcpStream::connect(format!("{unity_ip}:{unity_port}")) {
        Ok(stream) => stream,
        Err(e) => {
            panic!("Could not establish connection on {unity_ip}:{unity_port} with Unity: {e}")
        }
    };
    stream
        .set_read_timeout(Some(Duration::new(0, 1000000000)))
        .unwrap();

    match stream.write_all("RESTART".as_bytes()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not signal restart: {e}")
        }
    };
}

pub fn send_pos(unity: UnityOptions) -> std::io::Result<()> {
    for mut i in 1..=unity.num_container {
        i -= 1; // TODO: There is def a better way
        debug!(
            "sending pos file {:?}",
            unity.unity_position_files[i as usize]
        );
        let mut pos_file = match File::open(unity.unity_position_files[i as usize].clone()) {
            Ok(file) => file,
            Err(e) => {
                panic!(
                    "Could not read {:?}: {}",
                    unity.unity_position_files[i as usize], e
                )
            }
        };

        let mut file_contents = String::new();
        match pos_file.read_to_string(&mut file_contents) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Could not read position file {}: {}",
                    unity.unity_position_files[i as usize].display(),
                    e
                )
            }
        };

        let json: PosEntry = match serde_json::from_str(&file_contents) {
            Ok(json) => json,
            Err(e) => {
                panic!(
                    "{} contains invalid or incomplete calibration data: {}",
                    unity.unity_position_files[i as usize].display(),
                    e
                )
            }
        };

        let pb = ProgressBar::new(json.len().try_into().unwrap());
        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>3}/{len:3} ({eta})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn std::fmt::Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
        .progress_chars("#>-")); // This can take a while, especially for alot of LEDs
        let mut pb_count = 0;

        debug!("establishing connection to unity");
        let mut stream = TcpStream::connect(format!(
            "{}:{}",
            unity.unity_ip.clone(),
            unity.unity_ports.clone()[i as usize]
        ))?;
        stream.set_read_timeout(Some(Duration::new(1, 0))).unwrap();
        stream.set_write_timeout(Some(Duration::new(1, 0))).unwrap();
        debug!("sending positions to connection");
        for led in json.iter() {
            pb_count += 1;
            pb.set_position(pb_count);
            stream
                .write_all(
                    format!(
                        "{},{},{}",
                        led.1.0 as f32 * unity.scale,
                        led.1.1 as f32 * unity.scale,
                        led.2.0 as f32 * unity.scale
                    )
                    .as_bytes(),
                )
                .unwrap();

            let mut response: [u8; 3] = [0; 3];
            stream.read_exact(&mut response).unwrap();

            if match str::from_utf8(&response) {
                Ok(v) => v,
                Err(e) => panic!("Invalid UTF-8 sequence: {e}"),
            } != "ack"
            {
                error!("Did not get acknowledgement from Unity! You may have missing LEDs.");
            }
        }
        pb.finish();

        stream.write_all("END".as_bytes())?;
    }
    Ok(())
}

pub fn get_events(
    manager: Arc<Mutex<ManagerData>>,
    unity: &UnityOptions,
    config: &Config,
    port: &u32,
    frame_buffer: &Option<Arc<Mutex<GetEventsFrameBuffer>>>, // Seperate buffer for frames to reduce locks on manager
    keepalive: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error>> {
    type JsonHashmap = HashMap<usize, ((f32, f32), (f32, f32), (u8, u8, u8), bool)>; // <index, xy, zy, rgb, illuminated>

    let ip = unity.unity_ip;

    debug!("get_events active on {ip}:{port}");
    let socket = UdpSocket::bind(format!("{ip}:{port}"))?;

    // load positions if we are streaming video with widgets
    let mut json: PosEntry;
    let mut json_hashmap: Arc<Mutex<JsonHashmap>> = Arc::new(Mutex::new(Default::default()));

    let mut frame_cam_1: Mat = Default::default();
    let mut frame_cam_2: Mat = Default::default();

    let owned_config = config.clone();
    let owned_manager = Arc::clone(&manager);

    if config
        .advanced
        .camera
        .get_events_video_widgets
        .unwrap_or(false)
    {
        // If one isn't set, assume the first pos file
        let pos_index = config
            .advanced
            .camera
            .get_events_widgets_pos_index
            .unwrap_or(0);

        let mut pos_file = match File::open(unity.unity_position_files[pos_index as usize].clone())
        {
            Ok(file) => file,
            Err(e) => {
                panic!(
                    "Could not read {:?}: {}",
                    unity.unity_position_files[pos_index as usize], e
                )
            }
        };

        let mut file_contents = String::new();
        match pos_file.read_to_string(&mut file_contents) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Could not read position file {}: {}",
                    unity.unity_position_files[pos_index as usize].display(),
                    e
                )
            }
        };

        json = match serde_json::from_str(&file_contents) {
            Ok(json) => json,
            Err(e) => {
                panic!(
                    "{} contains invalid or incomplete calibration data: {}",
                    unity.unity_position_files[pos_index as usize].display(),
                    e
                )
            }
        };

        let led_count = json.len();
        let mut y_max = i32::MIN;

        for item in json.iter().take(led_count) {
            // Get max and min values in led_pos
            y_max = max(item.1.1, y_max);
        }

        for item in json.iter_mut().take(led_count) {
            let y_mid: f32 = (y_max / 2) as f32;
            let current_y: f32 = item.1.1 as f32;

            item.1.1 = match current_y {
                y if y > y_mid => (y_mid - (y - y_mid)).round() as i32,
                y if y < y_mid => (y_mid + (y_mid - y)).round() as i32,
                _ => item.1.1,
            };
        }

        json_hashmap = Arc::new(Mutex::new(
            json.into_iter()
                .enumerate()
                .map(|(i, (_key, val1, val2))| {
                    (
                        i,
                        (
                            (val1.0 as f32, val1.1 as f32),
                            (val2.0 as f32, val2.1 as f32),
                            (0u8, 0u8, 0u8),
                            false,
                        ),
                    )
                })
                .collect(),
        ));
    }

    if config
        .advanced
        .camera
        .get_events_streams_video
        .unwrap_or(false)
        && frame_buffer.is_some()
    {
        info!("Spawning get_events_streams_video thread.");
        warn!("This should only be used in demos due to decreased performance!");

        let owned_frame_buffer = Arc::clone(frame_buffer.as_ref().unwrap());
        let json_hashmap_guard = Arc::clone(&json_hashmap);

        thread::Builder::new()
            .name("get_events_stream_video".to_string())
            .spawn(move || {
                debug!("Opening cameras!");

                let config = owned_config;
                let mut cam2 = None;

                let cam = Arc::new(Mutex::new(
                    get_cam(&config, &config.camera.camera_index_1).unwrap(),
                ));

                if let Some(video_width) = config.camera.video_width
                    && let Some(video_height) = config.camera.video_height
                {
                    cam.lock()
                        .unwrap()
                        .set(CAP_PROP_FRAME_WIDTH, video_width)
                        .unwrap();
                    cam.lock()
                        .unwrap()
                        .set(CAP_PROP_FRAME_HEIGHT, video_height)
                        .unwrap();
                }

                match videoio::VideoCapture::is_opened(cam.as_ref().lock().as_ref().unwrap())
                    .unwrap()
                {
                    true => {}
                    false => {
                        panic!("Unable to open camera 1!")
                    }
                };

                if config.camera.multi_camera {
                    cam2 = Some(Arc::new(Mutex::new(
                        get_cam(&config, &config.camera.camera_index_2.clone().unwrap()).unwrap(),
                    )));

                    if let Some(video_width) = config.camera.video_width
                        && let Some(video_height) = config.camera.video_height
                    {
                        cam2.as_ref()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .set(CAP_PROP_FRAME_WIDTH, video_width)
                            .unwrap();
                        cam2.as_ref()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .set(CAP_PROP_FRAME_HEIGHT, video_height)
                            .unwrap();
                    }

                    match videoio::VideoCapture::is_opened(&cam2.as_ref().unwrap().lock().unwrap())
                        .unwrap()
                    {
                        true => {}
                        false => {
                            panic!("Unable to open camera 2!")
                        }
                    };
                }

                loop {
                    cam.lock().unwrap().read(&mut frame_cam_1).unwrap();

                    if let Some(cam2) = &mut cam2
                        && config.camera.multi_camera
                    {
                        cam2.lock().unwrap().read(&mut frame_cam_2).unwrap();
                    }

                    if config
                        .advanced
                        .camera
                        .get_events_video_widgets
                        .unwrap_or(false)
                    {
                        for (_key, (xy, z, rgb, _enabled)) in json_hashmap_guard
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|(_, (_xy, _z, _rgb, enabled))| *enabled)
                        {
                            imgproc::circle(
                                &mut frame_cam_1,
                                Point::new(xy.0 as i32, xy.1 as i32),
                                20,
                                Scalar::new(rgb.2 as f64, rgb.1 as f64, rgb.0 as f64, 0.0f64),
                                3,
                                LINE_8,
                                0,
                            )
                            .unwrap();

                            imgproc::circle(
                                &mut frame_cam_2,
                                Point::new(z.0 as i32, xy.1 as i32),
                                20,
                                Scalar::new(rgb.2 as f64, rgb.1 as f64, rgb.0 as f64, 0.0f64),
                                3,
                                LINE_8,
                                0,
                            )
                            .unwrap();
                        }
                    }

                    {
                        let mut frame_buffer = owned_frame_buffer.lock().unwrap();
                        frame_buffer.shared_frame_1 = frame_cam_1.clone();
                        frame_buffer.shared_frame_2 = frame_cam_2.clone();
                        if !owned_manager.lock().unwrap().state.keepalive_get_events {
                            info!("get_events_streams_video thread exiting");
                            break;
                        }
                    }
                }
            })
            .unwrap();
    }

    socket.set_nonblocking(true)?;
    let mut buf = [0; 4096];

    while keepalive.load(Ordering::Relaxed) {
        let mut hashmap_lock = json_hashmap.lock().unwrap();

        match socket.recv_from(&mut buf) {
            Ok((len, _addr)) => {
                let data = &buf[..len];

                let text = match std::str::from_utf8(data) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Invalid UTF-8 packet: {:?}", e);
                        continue;
                    }
                };

                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    if line.contains("CLEAR") {
                        for i in 0..config.num_led {
                            led_manager::set_color(&manager, i.try_into().unwrap(), 0, 0, 0);
                        }
                    } else if line.contains("E") {
                        // Clear color of index `EN`
                        let mut line = line.to_string();
                        line.remove(0);
                        let index = match line.to_string().trim().parse::<u16>() {
                            Ok(index) => index,
                            Err(e) => {
                                panic!(
                                    "Unity packet was malformed: Attempted to convert {} to u16: {e}",
                                    line.to_string().trim()
                                )
                            }
                        };
                        led_manager::set_color(&manager, index, 0, 0, 0);

                        // Indicate this isn't illuminated
                        if let Some(value) = hashmap_lock.get_mut(&(index as usize)) {
                            value.3 = false;
                        }
                    } else if line.contains("|") {
                        // Set index n with r g b from string n|r|g|b
                        let mut xs: [u16; 4] = [0; 4];
                        let nrgb = line.trim_matches(char::is_control).split("|");
                        for (i, el) in nrgb.enumerate() {
                            xs[i] = match el.parse::<u16>() {
                                Ok(el) => el,
                                Err(e) => {
                                    panic!(
                                        "Unity packet was malformed: Attempted to convert {el} to u8: {e}"
                                    )
                                }
                            };
                        }

                        if xs[1] != 0 || xs[2] != 0 || xs[3] != 0 {
                            // Indicate this is illuminated
                            if let Some(value) = hashmap_lock.get_mut(&(xs[0] as usize)) {
                                value.3 = true;
                                value.2 = (xs[1] as u8, xs[2] as u8, xs[3] as u8);
                            }
                        } else {
                            // Indicate this isn't illuminated
                            if let Some(value) = hashmap_lock.get_mut(&(xs[0] as usize)) {
                                value.3 = false;
                            }
                        }
                        led_manager::set_color(
                            &manager,
                            xs[0],
                            xs[1] as u8,
                            xs[2] as u8,
                            xs[3] as u8,
                        );
                    } else {
                        error!("Unity packet was malformed! Packet: {line}");
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(e) => {
                error!("Socket recv_from error: {e}");
                break;
            }
        }

        let mut manager_lock = manager.lock().unwrap();

        // TODO: Remove after Open Sauce (legacy for OS demo code)
        if !manager_lock.state.keepalive_get_events {
            info!("get_events exiting.");
            manager_lock.state.keepalive_get_events = true;
            break;
        }
    }

    Ok(())
}

pub fn start_listeners(
    config_holder: &Config,
    manager: &Arc<Mutex<ManagerData>>,
) -> Vec<JoinHandle<()>> {
    // info!("Clearing string");

    // for n in 0..config_holder.num_led {
    //     led_manager::set_color(manager, n as u16, 0, 0, 0);
    // }

    info!("Spawning listening threads");

    let mut children = Vec::new();

    for i in 0..config_holder.unity_options.num_container {
        debug!("Spawning listening thread.");

        let owned_manager;

        {
            let manager = manager.lock().unwrap();
            owned_manager = Arc::new(Mutex::new(ManagerData {
                config: RuntimeConfig {
                    num_led: config_holder.num_led,
                    num_strips: config_holder.num_strips,
                    communication_mode: config_holder.communication.communication_mode,
                    host: config_holder.communication.host,
                    port: config_holder.communication.port,
                    serial_port_paths: manager.config.serial_port_paths.clone(),
                    baud_rate: config_holder.communication.baud_rate,
                    serial_read_timeout: manager.config.serial_read_timeout,
                    record_data: manager.config.record_data,
                    record_data_file: manager.config.record_data_file.clone(),
                    record_esp_data: manager.config.record_esp_data,
                    unity_controls_recording: manager.config.unity_controls_recording,
                    record_esp_data_file: manager.config.record_esp_data_file.clone(),
                    print_send_back: config_holder.advanced.misc.print_send_back,
                    udp_read_timeout: config_holder.advanced.communication.udp_read_timeout,
                    con_fail_limit: config_holder.advanced.communication.con_fail_limit,
                    no_controller: config_holder.advanced.misc.no_controller,
                    scan_mode: config_holder.scan.scan_mode,
                    filter_color: config_holder.scan.filter_color,
                    filter_range: config_holder.scan.filter_range,
                    hsv_red_override: config_holder
                        .advanced
                        .hsv_overrides
                        .hsv_red_override
                        .clone(),
                    hsv_green_override: config_holder
                        .advanced
                        .hsv_overrides
                        .hsv_green_override
                        .clone(),
                    hsv_blue_override: config_holder
                        .advanced
                        .hsv_overrides
                        .hsv_blue_override
                        .clone(),
                    no_video: config_holder.advanced.camera.no_video,
                    skip_confirmation: config_holder.advanced.communication.skip_confirmation,
                    use_queue: config_holder.advanced.communication.use_queue,
                    queue_size: config_holder.advanced.communication.queue_size,
                    led_config: None, // This will be constructed as needed by led_manager
                },
                state: ManagerState {
                    first_run: true,
                    call_time: SystemTime::now(),
                    keepalive_get_events: true,
                    keepalive: Arc::new(AtomicBool::new(true)),
                    led_state: LedState {
                        failures: 0,
                        queue_lengths: Vec::new(),
                    },
                    led_thread_channels: Vec::new(),
                    all_thread_handles: Vec::new(),
                },
                io: IOHandles {
                    data_file_buf: None,
                    esp_data_file_buf: None,
                    udp_socket: None,
                    serial_port: Vec::new(),
                },
                vision: VisionData {
                    frame_cam_1: Default::default(),
                    frame_cam_2: Default::default(),
                },
            }));
        }

        let owned_options = config_holder.unity_options.clone();
        let owned_config = config_holder.clone();
        let owned_keepalive = Arc::clone(&manager.lock().unwrap().state.keepalive);

        info!("pushing child");
        children.push(
            thread::Builder::new()
                .name("get_events".to_string())
                .spawn(move || {
                    debug!("inside thread");
                    match get_events(
                        owned_manager,
                        &owned_options.clone(),
                        &owned_config,
                        &owned_options.unity_ports.clone()[i as usize],
                        &None,
                        owned_keepalive,
                    ) {
                        Ok(_) => {
                            debug!("get_events thread exited.")
                        }
                        Err(e) => {
                            panic!("get_events thread crashed with error: {e}")
                        }
                    }
                })
                .unwrap(),
        )
    }
    children
}
