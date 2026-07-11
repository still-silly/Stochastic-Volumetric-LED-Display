use std::{
    error::Error,
    fs::{File, remove_file},
    io::{BufWriter, Read, Write},
    net::{Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread::JoinHandle,
    time::SystemTime,
};

use crossbeam_channel::Sender;
use log::{debug, error, info, warn}; // TODO: Depreceate unity export byte data
use opencv::prelude::*;
use serde::Deserialize;
use serialport::SerialPort;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub num_led: u32,
    pub num_strips: u32,
    pub communication: CommunicationConfig,
    pub recording: RecordingConfig,
    pub camera: CameraConfig,
    pub scan: ScanConfig,
    pub unity_options: UnityOptions,
    pub advanced: AdvancedConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ScanConfig {
    /// 0 is default, 1 filters by color first (Useful when you aren't scanning in perfect conditions)
    pub scan_mode: u32,
    /// 0 for red, 1 for green, 2 for blue
    pub filter_color: Option<u32>,
    /// Range for color filter
    pub filter_range: Option<u8>,
    /// LED brightness during scan
    pub color_bright: Option<u8>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CameraConfig {
    pub multi_camera: bool,
    pub camera_index_1: String,
    pub camera_index_2: Option<String>,
    pub video_width: Option<f64>,
    pub video_height: Option<f64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RecordingConfig {
    pub record_data: bool,
    pub record_esp_data: bool,
    pub unity_controls_recording: bool,
    pub record_data_file: PathBuf,
    pub record_esp_data_file: PathBuf,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CommunicationConfig {
    /// 1 uses UDP (in a format that supports WLED), 2 uses serial
    pub communication_mode: i8,
    /// UDP host
    pub host: Option<Ipv4Addr>,
    /// UDP port
    pub port: Option<i32>,
    /// Serial port(s) to use in serial mode
    pub serial_port_paths: Option<Vec<String>>,
    /// Baud rate to use in serial mode
    pub baud_rate: Option<u32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AdvancedCommConfig {
    /// Timeout in milliseconds for serial reads
    pub serial_read_timeout: Option<u32>,
    /// Timeout in milliseconds for UDP reads
    pub udp_read_timeout: Option<u32>,
    /// Limit of consecutive connection failures before giving up
    pub con_fail_limit: Option<u32>,
    /// Should set_color queue writes?
    pub use_queue: Option<bool>,
    pub queue_size: Option<usize>,
    /// Should we skip checking if the LED was properly set? Speeds things way up at the cost of accuracy. Not recommended.
    pub skip_confirmation: Option<bool>,
    ///  How many writes to perform during the speedtest
    pub speedtest_writes: Option<u32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AdvancedCameraConfig {
    /// When set in conjunction with streamlined being true, no video feed will show.
    pub no_video: Option<bool>,
    /// When set to true, get_events will stream to frame_cam_1/2
    pub get_events_streams_video: Option<bool>,
    /// When set to true, get_events video stream will include circles around illuminated LEDs for visualization purposes
    pub get_events_video_widgets: Option<bool>,
    /// Which pos file to use for visualization
    pub get_events_widgets_pos_index: Option<i32>,
    /// How many frames to capture before using the most recent. Needed because OpenCV will not always provide the most recent frame
    pub capture_frames: Option<i32>,
    /// When true, assume the second camera is overhead
    pub cam2_overhead: Option<bool>,
    /// When true, assume the overhead camera is upside down relative to the front camera
    pub cam2_overhead_flip: Option<bool>,
    /// When true, don't spawn a thread to consume frames in the background. Defaults to false.
    pub no_background_frame_consumer: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct HsvOverrideConfig {
    /// Override the filter band for the red color when using a color filter.
    /// Should be formatted like <upper_h, upper_s, upper_v, lower_h, lower_s, lower_v>
    pub hsv_red_override: Option<Vec<u8>>,
    pub hsv_green_override: Option<Vec<u8>>,
    pub hsv_blue_override: Option<Vec<u8>>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct ImageTransformConfig {
    /// When true, transform adjustments will be auto applied after scan.
    pub adjust_after_scan: Option<bool>,
    /// When set, cropping will be skipped.
    pub crop_override: Option<Vec<i32>>,
    /// Offsets distant LEDs toward the center to reduce perspective distortion, making the layout appear more orthographic.
    pub x_perspect_distort_adjust: Option<i32>,
    pub y_perspect_distort_adjust: Option<i32>,
    pub z_perspect_distort_adjust: Option<i32>,
    /// Amount to stretch (or shrink) from center on a specified axis
    pub x_stretch_adjust: Option<i32>,
    pub y_stretch_adjust: Option<i32>,
    pub z_stretch_adjust: Option<i32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct MiscConfig {
    pub print_send_back: Option<bool>,
    pub no_controller: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AdvancedConfig {
    #[serde(default)]
    pub communication: AdvancedCommConfig,
    #[serde(default)]
    pub camera: AdvancedCameraConfig,
    #[serde(default)]
    pub hsv_overrides: HsvOverrideConfig,
    #[serde(default)]
    pub transform: ImageTransformConfig,
    #[serde(default)]
    pub misc: MiscConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct UnityOptions {
    pub num_container: u8,
    pub unity_ip: Ipv4Addr,
    pub unity_ports: Vec<u32>,
    pub unity_position_files: Vec<PathBuf>,
    pub scale: f32,
}
pub struct ManagerData {
    pub config: RuntimeConfig,
    pub state: ManagerState,
    pub io: IOHandles,
    pub vision: VisionData,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub num_led: u32,
    pub num_strips: u32,
    pub communication_mode: i8,
    pub host: Option<Ipv4Addr>,
    pub port: Option<i32>,
    pub serial_port_paths: Option<Vec<String>>,
    pub baud_rate: Option<u32>,
    pub serial_read_timeout: Option<u32>,
    pub record_data: bool,
    pub record_esp_data: bool,
    pub unity_controls_recording: bool,
    pub record_data_file: PathBuf,
    pub record_esp_data_file: PathBuf,
    pub print_send_back: Option<bool>,
    pub udp_read_timeout: Option<u32>,
    pub con_fail_limit: Option<u32>,
    pub no_controller: Option<bool>,
    pub scan_mode: u32,
    pub filter_color: Option<u32>,
    pub filter_range: Option<u8>,
    pub hsv_red_override: Option<Vec<u8>>,
    pub hsv_green_override: Option<Vec<u8>>,
    pub hsv_blue_override: Option<Vec<u8>>,
    pub no_video: Option<bool>,
    pub skip_confirmation: Option<bool>,
    pub use_queue: Option<bool>,
    pub queue_size: Option<usize>,
    pub led_config: Option<LedConfig>, // Exists so that we don't have to create a new struct every time we call set_color. Acts just as a holder for other items from RuntimeConfig
}

pub struct ManagerState {
    pub first_run: bool,
    pub call_time: SystemTime,
    pub keepalive: Arc<AtomicBool>,
    pub keepalive_get_events: bool,
    pub led_state: LedState,
    pub led_thread_channels: Vec<Sender<Task>>,
    pub all_thread_handles: Vec<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct IOHandles {
    pub udp_socket: Option<UdpSocket>,
    pub serial_port: Vec<Box<dyn SerialPort>>,
    pub data_file_buf: Option<BufWriter<File>>,
    pub esp_data_file_buf: Option<BufWriter<File>>,
}

#[derive(Debug)]
pub struct VisionData {
    pub frame_cam_1: Mat,
    pub frame_cam_2: Mat,
}

#[derive(Clone)]
pub struct GetEventsFrameBuffer {
    pub shared_frame_1: Mat,
    pub shared_frame_2: Mat,
}

#[derive(Clone, Debug)]
pub struct ScanData {
    pub pos: CropPos,
    pub invert: bool,
    pub depth: bool,
}

#[derive(Clone, Debug)]
pub struct CropPos {
    pub x1_start: i32,
    pub y1_start: i32,
    pub x1_end: i32,
    pub y1_end: i32,
    pub x2_start: Option<i32>,
    pub y2_start: Option<i32>,
    pub x2_end: Option<i32>,
    pub y2_end: Option<i32>,
    pub cam_1_brightest: Option<f64>,
    pub cam_2_brightest: Option<f64>,
    pub cam_1_darkest: Option<f64>,
    pub cam_2_darkest: Option<f64>,
}

#[derive(Debug)]
pub struct LedState {
    pub failures: u32,
    pub queue_lengths: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LedConfig {
    // This contains values that will be cloned before moving into closure inside a thread so we don't have to deal with shared configs when using queues inside led_manager.
    // future me, not sure what the fuck i was thinking
    pub skip_confirmation: Option<bool>,
    pub unity_controls_recording: bool,
    pub no_controller: Option<bool>,
    pub port: Option<i32>,
    pub communication_mode: i8,
    pub num_led: u32,
    pub num_strips: u32,
    pub serial_read_timeout: Option<u32>,
    pub udp_read_timeout: Option<u32>,
    pub host: Option<Ipv4Addr>,
    pub con_fail_limit: Option<u32>,
    pub print_send_back: Option<bool>,
    pub serial_port_paths: Option<Vec<String>>,
}

#[derive(Copy, Clone)]
pub struct Task {
    pub command: (u16, u8, u8, u8),
    pub controller_queue_length: Option<u8>,
}

pub type PosEntry = Vec<(String, (i32, i32), (i32, i32))>;

pub fn load_validate_conf(config_path: &Path) -> (ManagerData, UnityOptions, Config) {
    // TODO: Make this an implmentation
    // Load and validate config
    if !config_path.exists() {
        panic!(
            "Could not find svled.toml! Please create one according to the documentation in the current directory."
        );
    }
    let mut config_file =
        File::open(config_path).expect("Could not open config file. Do I have permission?");
    let mut config_file_contents = String::new();
    config_file
        .read_to_string(&mut config_file_contents)
        .expect(
            "The config file contains non UTF-8 characters, what in the world did you put in it??",
        );
    let config_holder: Config = toml::from_str(&config_file_contents)
        .expect("The config file was not formatted properly and could not be read.");

    // Validate config and inform user of settings

    if let Some(no_controller) = config_holder.advanced.misc.no_controller
        && !no_controller
    {
        let comm_conf = &config_holder.communication;
        if let Some(paths) = &comm_conf.serial_port_paths
            && comm_conf.communication_mode == 2
        {
            for path in paths.iter() {
                if Path::new(&path).exists() {
                    info!("Using serial for communication on {path}!");
                } else {
                    panic!("Serial port {path} does not exist!");
                }
            }
        } else if let Some(host) = comm_conf.host
            && let Some(port) = comm_conf.port
            && comm_conf.communication_mode == 1
        {
            info!("Using udp for communication at {} on port {}", host, port);
        }
    }

    if config_holder.recording.unity_controls_recording
        || config_holder.recording.record_data
        || config_holder.recording.record_esp_data
    {
        if Path::new(&config_holder.recording.record_data_file).exists()
            && config_holder.recording.record_data
        {
            warn!(
                "{} will be overwritten once you start recording!",
                config_holder.recording.record_data_file.display()
            );
        }
        if Path::new(&config_holder.recording.record_esp_data_file).exists()
            && config_holder.recording.record_esp_data
        {
            warn!(
                "{} will be overwritten once you start recording!",
                config_holder.recording.record_esp_data_file.display()
            )
        }
    }
    if config_holder.recording.unity_controls_recording {
        info!("Unity will control recording of data.");
    } else if config_holder.recording.record_data {
        info!(
            "All data will be recorded during this session in VLED format to {}",
            config_holder.recording.record_data_file.display()
        );
    } else if config_holder.recording.record_esp_data {
        info!(
            "All data will be recorded during this session in bVLED format to {}",
            config_holder.recording.record_esp_data_file.display()
        );
    } else {
        warn!("No bVLED or VLED data will be recorded!");
    }

    if config_holder.camera.multi_camera {
        info!("Using multiple cameras!");
    }

    (
        ManagerData {
            config: RuntimeConfig {
                num_led: config_holder.num_led,
                num_strips: config_holder.num_strips,
                communication_mode: config_holder.communication.communication_mode,
                host: config_holder.communication.host,
                port: config_holder.communication.port,
                serial_port_paths: config_holder.communication.serial_port_paths.clone(), // So we can create new ManagerDatas
                baud_rate: config_holder.communication.baud_rate,
                serial_read_timeout: config_holder.advanced.communication.serial_read_timeout,
                record_data: config_holder.recording.record_data,
                record_data_file: config_holder.recording.record_data_file.clone(),
                record_esp_data: config_holder.recording.record_esp_data,
                unity_controls_recording: config_holder.recording.unity_controls_recording,
                record_esp_data_file: config_holder.recording.record_esp_data_file.clone(),
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
                led_config: None,
            },
            state: ManagerState {
                first_run: true,
                call_time: SystemTime::now(),
                keepalive_get_events: true,
                keepalive: Arc::new(AtomicBool::new(true)),
                led_state: {
                    LedState {
                        failures: 0,
                        queue_lengths: Vec::new(),
                    }
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
        },
        config_holder.unity_options.clone(),
        config_holder,
    )
}

pub fn check_and_create_file(file: &PathBuf) -> Result<File, Box<dyn Error>> {
    if Path::new(&file).exists() {
        let remove_file_result = remove_file(file);
        match remove_file_result {
            Ok(()) => debug!("Removed {}", file.display()),
            Err(error) => error!("Could not remove {}: {}.", file.display(), error),
        }
        match File::create(file) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not create {}: {}", file.display(), e);
            }
        }
    }
    let data_file = match File::create(file.clone()) {
        Ok(file) => file,
        Err(e) => panic!("Could not open {}: {}", file.display(), e),
    };

    Ok(data_file)
}

pub fn flush_data(manager_guard: Arc<Mutex<ManagerData>>) {
    let mut manager = manager_guard.lock().unwrap();
    // Flush our BufWriters
    if manager.io.data_file_buf.is_some()
        && let Some(data_file_buf) = manager.io.data_file_buf.as_mut()
        && let Err(e) = data_file_buf.flush()
    {
        error!(
            "Could not flush {}! It may be incomplete or corrupted. Error: {}",
            manager.config.record_data_file.display(),
            e
        )
    };

    if manager.io.esp_data_file_buf.is_some()
        && let Some(esp_data_file_buf) = manager.io.esp_data_file_buf.as_mut()
    {
        match esp_data_file_buf.flush() {
            Ok(_) => {}
            Err(e) => {
                error!(
                    "Could not flush {}! It may be incomplete or corrupted. Error: {}",
                    manager.config.record_esp_data_file.display(),
                    e
                )
            }
        }
    };
}
