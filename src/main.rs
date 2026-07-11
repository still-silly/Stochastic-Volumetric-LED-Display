// this was actually written by hand, which may explain alot
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process,
    sync::{Arc, Mutex, atomic::Ordering},
};

use env_logger::Builder;
use gumdrop::Options;
use log::{LevelFilter, debug, error, info};
use opencv::{
    core::{Mat, MatTraitConst},
    videoio::{self, VideoCaptureTrait, VideoCaptureTraitConst},
};
#[cfg(feature = "scan")]
use svled::scan;
use svled::{
    PosEntry,
    demo::{self, render_jpg_onto_leds},
    driver_wizard,
    led_manager::{self, set_color},
    read_vled,
    scan::position_adjustment,
    speedtest,
    unity::{self, start_listeners},
    utils,
};

#[derive(Debug, Options)]
struct MyOptions {
    #[options(help = "print help message")]
    help: bool,
    #[options(help = "be verbose")]
    verbose: bool,
    #[options(help = "specify a specific config file")]
    config: Option<String>,

    // The `command` option will delegate option parsing to the command type,
    // starting at the first free argument.
    #[options(command)]
    command: Option<Command>,
}

#[derive(Debug, Options)]
enum Command {
    // Command names are generated from variant names.
    // By default, a CamelCase name will be converted into a lowercase,
    // hyphen-separated name; e.g. `FooBar` becomes `foo-bar`.
    //
    // Names can be explicitly specified using `#[options(name = "...")]`
    #[options(help = "perform a connection speedtest")]
    Speedtest(SpeedtestOptions),

    #[options(help = "play back a vled file")]
    ReadVled(ReadvledOptions),

    #[cfg(feature = "scan")]
    #[options(help = "calibrate a svled container")]
    Calibrate(CalibrateOptions),

    #[options(help = "send positions and connect to Unity")]
    Unity(UnityCommandOptions),

    #[options(help = "send positions to Unity")]
    SendPos(SendPosOptions),

    #[options(help = "connect to Unity")]
    ConnectUnity(ConnectUnity),

    #[options(help = "interactively create a ino/cpp file for your LED driver")]
    DriverWizard(DriverWizardOptions),

    #[options(help = "set a single leds color")]
    SetColor(SetColorOptions),

    #[options(help = "clear the strip")]
    Clear(ClearOptions),

    #[options(help = "run a simple demo")]
    Demo(DemoOptions),

    #[options(help = "convert an led position json into a C++ compatible constant")]
    ConvertLedpos(ConvertLedposOptions),

    #[options(help = "list functioning camera indexes")]
    ListCams(ListCamsOptions),

    #[options(help = "perform perspective adjustment")]
    AdjustPerspective(AdjustPerspectiveOptions),
}

#[derive(Debug, Options)]
struct SpeedtestOptions {}

#[derive(Debug, Options)]
struct ReadvledOptions {
    #[options(help = "vled file to read", required)]
    vled_file: PathBuf,
}

#[derive(Debug, Options)]
struct SetColorOptions {
    #[options(help = "index of LED", required)]
    n: u16,
    #[options(help = "R value to set", required)]
    r: u8,
    #[options(help = "G value to set", required)]
    g: u8,
    #[options(help = "B value to set", required)]
    b: u8,
}

#[cfg(feature = "scan")]
#[derive(Debug, Options)]
struct CalibrateOptions {}

#[derive(Debug, Options)]
struct UnityCommandOptions {}

#[derive(Debug, Options)]
struct SendPosOptions {}

#[derive(Debug, Options)]
struct ConnectUnity {}

#[derive(Debug, Options)]
struct DriverWizardOptions {}

#[derive(Debug, Options)]
struct DemoOptions {
    #[options(help = "demo to run (rainbow, rainbow-loop)", required)]
    active_demo: String,

    #[options(help = "image to render")]
    image_path: Option<String>,
}

#[derive(Debug, Options)]
struct ClearOptions {}

#[derive(Debug, Options)]
struct ConvertLedposOptions {
    #[options(help = "input JSON file", required)]
    input: String,

    #[options(help = "output file")]
    output: Option<String>,
}

#[derive(Debug, Options)]
struct ListCamsOptions {
    #[options(help = "which index to start search at")]
    lower_index: Option<i32>,

    #[options(help = "which index to end search at")]
    upper_index: Option<i32>,
}

#[derive(Debug, Options)]
struct AdjustPerspectiveOptions {
    #[options(help = "path to position file", required)]
    position_file: String,

    #[options(help = "output path")]
    output_file: Option<String>,
}

fn main() {
    let opts = MyOptions::parse_args_default_or_exit();

    let path = opts.config.unwrap_or("svled.toml".to_string());
    let config_path = Path::new(&path);

    let mut builder = Builder::new();

    builder
        .filter_level(if opts.verbose {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        })
        .format_timestamp_secs();

    builder.init();

    if let Some(Command::ConvertLedpos(ref convert_ledpos_options)) = opts.command {
        let path = Path::new(&convert_ledpos_options.input);

        info!(
            "Converting {} into an C++ compatible data format",
            path.display()
        );

        if path.exists() {
            let mut pos_file = match File::open(path) {
                Ok(file) => file,
                Err(e) => {
                    panic!("Could not read {:?}: {}", path.display(), e)
                }
            };

            let mut file_contents = String::new();
            match pos_file.read_to_string(&mut file_contents) {
                Ok(_) => {}
                Err(e) => {
                    panic!("Could not read position file {}: {}", path.display(), e)
                }
            };

            let json: PosEntry = match serde_json::from_str(&file_contents) {
                Ok(json) => json,
                Err(e) => {
                    panic!(
                        "{} contains invalid or incomplete calibration data: {}",
                        path.display(),
                        e
                    )
                }
            };

            let mut output_string = String::new();

            let len = json.len();

            output_string.push_str(&format!(
                "static constexpr std::array<std::array<int16_t, 3>, {len}> coords = {{{{"
            ));

            for (index, entry) in json.iter().enumerate() {
                output_string.push_str(&format!("{{{}, {}, {}}}", entry.1.0, entry.1.1, entry.2.1));

                if index == len - 1 {
                    output_string.push_str("}};")
                } else {
                    output_string.push_str(", ");
                }
            }

            info!("{output_string}");
        } else {
            error!("{} not found", path.display());
        }
        return;
    } else if let Some(Command::ListCams(ref list_cams_options)) = opts.command {
        let lower_index = list_cams_options.lower_index.unwrap_or(0);
        let upper_index = list_cams_options.upper_index.unwrap_or(10);

        let mut working_cameras = Vec::new();
        info!("Testing camera indices from {lower_index} to {upper_index}");

        for index in lower_index..=upper_index {
            let mut cap = match videoio::VideoCapture::new(index, videoio::CAP_ANY) {
                Ok(c) => c,
                Err(_) => {
                    info!("Camera {index} is not available (error opening).");
                    continue;
                }
            };

            if cap.is_opened().unwrap() {
                let mut frame = Mat::default();
                let ret = cap.read(&mut frame).unwrap();

                if ret && !frame.empty() {
                    info!("Camera {index} is working.");
                    working_cameras.push(index);
                } else {
                    info!("Camera {index} opened but failed to read frame.");
                }
            } else {
                info!("Camera {index} is not available.");
            }
            cap.release().unwrap();
        }

        info!("\nWorking cameras: {working_cameras:?}");

        return;
    } else if let Some(Command::AdjustPerspective(ref perspective_adjust_options)) = opts.command {
        let config_holder = utils::load_validate_conf(config_path).2;

        let mut pos_file = match File::open(perspective_adjust_options.position_file.clone()) {
            Ok(file) => file,
            Err(e) => {
                panic!(
                    "Could not read {:?}: {}",
                    perspective_adjust_options.position_file, e
                )
            }
        };

        let mut file_contents = String::new();
        match pos_file.read_to_string(&mut file_contents) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Could not read position file {}: {}",
                    perspective_adjust_options.position_file, e
                )
            }
        };

        let mut json: PosEntry = match serde_json::from_str(&file_contents) {
            Ok(json) => json,
            Err(e) => {
                panic!(
                    "{} contains invalid or incomplete calibration data: {}",
                    perspective_adjust_options.position_file, e
                )
            }
        };

        position_adjustment(&mut json, &config_holder);

        let name = perspective_adjust_options
            .output_file
            .clone()
            .unwrap_or(format!(
                "{}-postprocess.json",
                perspective_adjust_options
                    .position_file
                    .strip_suffix(".json")
                    .unwrap_or(&perspective_adjust_options.position_file)
            ));

        let json = serde_json::to_string_pretty(&json).expect("Unable to serialize metadata!");
        let mut file = match File::create(Path::new(&name)) {
            Ok(file) => file,
            Err(e) => {
                error!("Unable to write temp-pos to {name}");
                println!("Something went wrong trying to save the LED positions. Error: {e}");
                process::exit(1);
            }
        };

        match file.write_all(json.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                error!("Unable to write temp-pos to {name}");
                println!("Something went wrong trying to save the LED positions. Error: {e}");
            }
        }

        info!("Wrote perspective-adjusted position file to {name}");

        return;
    }

    let config_load_result = utils::load_validate_conf(config_path);

    let (manager, unity_options, config_holder) = (
        Arc::new(Mutex::new(config_load_result.0)),
        config_load_result.1,
        config_load_result.2,
    );

    if let Some(Command::Speedtest(ref _speedtest_options)) = opts.command {
        info!("Performing speedtest...");

        speedtest::speedtest(
            &manager,
            config_holder.num_led,
            config_holder
                .advanced
                .communication
                .speedtest_writes
                .unwrap_or(1000),
        );
    } else if let Some(Command::ReadVled(ref readvled_options)) = opts.command {
        if !readvled_options.vled_file.is_file() {
            error!("You must pass a valid vled file with --vled-file!");
            process::exit(0);
        } else {
            info!("Playing back {}!", readvled_options.vled_file.display());

            {
                manager.lock().unwrap().config.record_data = false;
                manager.lock().unwrap().config.record_esp_data = false;
            }
            match read_vled::read_vled(&manager, readvled_options.vled_file.clone()) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "Could not read {}: {}",
                        readvled_options.vled_file.display(),
                        e
                    )
                }
            };
        }
    } else if let Some(Command::Unity(ref _unity_options)) = opts.command {
        let ctrlc_manager = Arc::clone(&manager);

        ctrlc::set_handler(move || {
            info!("Exiting cleanly...");

            let mut manager = ctrlc_manager.lock().unwrap();

            if let Some(file_buf) = &mut manager.io.data_file_buf {
                debug!("Flushing data_file_buf");
                file_buf.flush().unwrap();
            }

            if let Some(file_buf) = &mut manager.io.esp_data_file_buf {
                debug!("Flushing esp_data_file_buf");
                file_buf.flush().unwrap();
            }

            debug!("signaling to exit any threads");
            manager.state.keepalive.store(false, Ordering::Relaxed);
            manager.state.keepalive_get_events = false;

            debug!("joining handles");
            for handle in std::mem::take(&mut manager.state.all_thread_handles) {
                if let Err(e) = handle.join() {
                    error!("Thread panicked: {e:?}");
                }
            }

            drop(manager);

            debug!("finished joining handles");
        })
        .expect("Error setting Ctrl-C handler");

        // Validate Unity section of config, if we are using Unity.

        for i in 0..=unity_options.num_container - 1 {
            if !Path::new(&unity_options.unity_position_files[i as usize]).is_file() {
                error!(
                    "{} is not a valid file! Will attempt to continue anyway.",
                    unity_options.unity_position_files[i as usize]
                        .clone()
                        .display()
                );
            }
        }

        info!("Sending positions to Unity");

        match unity::send_pos(unity_options.clone()) {
            Ok(_) => {}
            Err(e) => {
                panic!("There was an issue connecting to Unity: {e}");
            }
        };

        info!("Spawning listening threads");

        manager
            .lock()
            .unwrap()
            .state
            .all_thread_handles
            .append(&mut start_listeners(&config_holder, &manager));
    } else if let Some(Command::SendPos(ref _sendpos_options)) = opts.command {
        info!("Sending positions to Unity");

        match unity::send_pos(unity_options.clone()) {
            Ok(_) => {}
            Err(e) => {
                panic!("There was an issue connecting to Unity: {e}");
            }
        };
    } else if let Some(Command::ConnectUnity(ref _connectunity_options)) = opts.command {
        info!("Spawning listening threads");

        let ctrlc_manager = Arc::clone(&manager);

        ctrlc::set_handler(move || {
            info!("Exiting cleanly...");

            let mut manager = ctrlc_manager.lock().unwrap();

            if let Some(file_buf) = &mut manager.io.data_file_buf {
                debug!("Flushing data_file_buf");
                file_buf.flush().unwrap();
            }

            if let Some(file_buf) = &mut manager.io.esp_data_file_buf {
                debug!("Flushing esp_data_file_buf");
                file_buf.flush().unwrap();
            }

            debug!("signaling to exit any threads");
            manager.state.keepalive.store(false, Ordering::Relaxed);
            manager.state.keepalive_get_events = false;

            drop(manager);

            debug!("finished joining handles");
        })
        .expect("Error setting Ctrl-C handler");

        let mut listener_thread_handles = start_listeners(&config_holder, &manager);

        {
            let mut guard = manager.lock().unwrap();
            guard
                .state
                .all_thread_handles
                .append(&mut listener_thread_handles);
        }

        let all_handles = {
            let mut guard = manager.lock().unwrap();
            std::mem::take(&mut guard.state.all_thread_handles)
        };

        for handle in all_handles {
            info!("joining thread");
            handle.join().unwrap();
        }
    } else if let Some(Command::DriverWizard(ref _driver_wizard_options)) = opts.command {
        info!("Starting driver configuration wizard!");
        driver_wizard::wizard();
    } else if let Some(Command::SetColor(ref set_color_options)) = opts.command {
        info!(
            "Setting LED {} to RGB: {}, {}, {}",
            set_color_options.n, set_color_options.r, set_color_options.g, set_color_options.b
        );

        set_color(
            &manager,
            set_color_options.n,
            set_color_options.r,
            set_color_options.g,
            set_color_options.b,
        );
    } else if let Some(Command::Demo(ref demo_options)) = opts.command {
        info!("Running demo!");

        let mut pos_file = match File::open(unity_options.unity_position_files[0].clone()) {
            Ok(file) => file,
            Err(e) => {
                panic!(
                    "Could not read {:?}: {}",
                    unity_options.unity_position_files[0], e
                )
            }
        };

        let mut file_contents = String::new();
        match pos_file.read_to_string(&mut file_contents) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Could not read position file {}: {}",
                    unity_options.unity_position_files[0].display(),
                    e
                )
            }
        };

        let json: PosEntry = match serde_json::from_str(&file_contents) {
            Ok(json) => json,
            Err(e) => {
                panic!(
                    "{} contains invalid or incomplete calibration data: {}",
                    unity_options.unity_position_files[0].display(),
                    e
                )
            }
        };

        match demo_options.active_demo.to_lowercase().as_str() {
            "rainbow-loop" => loop {
                demo::rainbow(&manager, &json, 80, 50, false, demo::Axis::X, true);
                demo::rainbow(&manager, &json, 50, 50, false, demo::Axis::Y, true);
                demo::rainbow(&manager, &json, 80, 50, false, demo::Axis::Z, true);
            },
            "rainbow" => {
                demo::rainbow(&manager, &json, 80, 50, false, demo::Axis::X, true);
                demo::rainbow(&manager, &json, 50, 50, false, demo::Axis::Y, true);
                demo::rainbow(&manager, &json, 80, 50, false, demo::Axis::Z, true);
            }
            "image" => {
                render_jpg_onto_leds(
                    &demo_options.image_path.clone().unwrap(),
                    &json,
                    &manager,
                    Some(0..=250),
                );
            }
            // "image-sequence" => {
            //     render_jpg_sequence("bad-apple", "output_", &json, &manager, Some(0..=250));
            // }
            option => {
                error!("Invalid option {option}");
            }
        }
    } else if let Some(Command::Clear(ref _clear_options)) = opts.command {
        for n in 0..config_holder.num_led {
            led_manager::set_color(&manager, n as u16, 0, 0, 0);
        }
    }

    #[cfg(feature = "scan")]
    if let Some(Command::Calibrate(ref _calibrate_options)) = opts.command {
        info!("Performing calibrating");
        scan::scan(config_holder.clone(), &manager, false, None).expect("failure");
    }

    // led_manager::set_color(&mut manager, 1, 255, 255, 255);

    utils::flush_data(manager);
}
