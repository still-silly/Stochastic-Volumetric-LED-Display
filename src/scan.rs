use std::{
    cmp::{max, min},
    error::Error,
    fs::File,
    io::Write,
    path::Path,
    process,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};
use chrono::Local; // TODO: Play with different camera backends
use inquire;
use log::{debug, error, info, warn}; // TODO: Properly get HSV for each camera
use opencv::{
    core::{self, Point, Scalar, flip, get_default_algorithm_hint, min_max_loc, no_array},
    highgui::{self, EVENT_LBUTTONDOWN, EVENT_LBUTTONUP, EVENT_MOUSEMOVE},
    imgproc::{self, COLOR_BGR2GRAY, COLOR_BGR2HSV, LINE_8},
    prelude::*,
    videoio::{self, CAP_PROP_FRAME_HEIGHT, CAP_PROP_FRAME_WIDTH, VideoCapture},
};

use crate::{Config, CropPos, ManagerData, PosEntry, ScanData, led_manager};

type ScanResult = Result<(i32, i32, Option<i32>, Option<i32>), Box<dyn Error>>;
type CropData = Option<((i32, i32, i32, i32), (i32, i32, i32, i32))>;
type CallbackResult = (i32, i32, Option<i32>, Option<i32>);

pub fn scan(
    config: Config,
    manager_guard: &Arc<Mutex<ManagerData>>,
    streamlined: bool,
    crop_data: CropData,
) -> Result<()> {
    // streamlined skips cropping and ALL prompts, thus requiring multiple cameras to function
    let mut led_pos = vec![
        (
            "UNCALIBRATED".to_string(),
            (i32::MIN, i32::MIN),
            (i32::MIN, i32::MIN)
        );
        config.num_led as usize
    ];
    let num_led;
    let scan_mode;
    let filter_color;
    {
        num_led = manager_guard.lock().unwrap().config.num_led;
        scan_mode = manager_guard.lock().unwrap().config.scan_mode;
        filter_color = manager_guard.lock().unwrap().config.filter_color;
    }
    info!("Clearing strip");
    for i in 0..=num_led {
        led_manager::set_color(manager_guard, i.try_into().unwrap(), 0, 0, 0);
    }

    if scan_mode != 0 {
        if filter_color.unwrap() == 0 {
            debug!("Using red filter");
        } else if filter_color.unwrap() == 1 {
            debug!("Using green filter");
        } else if filter_color.unwrap() == 2 {
            debug!("Using blue filter");
        }
    }

    let window = "Please wait...";
    if let Some(no_video) = config.advanced.camera.no_video
        && !no_video
    {
        highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    }

    let mut pos = CropPos {
        // Needed because of possible uninitialization in the else bracket of the streamlined check below
        x1_start: 0,
        y1_start: 0,
        x1_end: 0,
        y1_end: 0,
        x2_start: None,
        y2_start: None,
        x2_end: None,
        y2_end: None,
        cam_1_brightest: None,
        cam_2_brightest: None,
        cam_1_darkest: None,
        cam_2_darkest: None,
    };

    if !streamlined {
        info!("Starting crop");
        pos = match crop(&config, manager_guard) {
            Ok(pos) => pos,
            Err(e) => return Err(anyhow!("There was a problem while trying to crop: {e}")),
        };
    }

    let cam = Arc::new(Mutex::new(
        get_cam(&config, &config.camera.camera_index_1.to_string()).unwrap(),
    )); // We need to constantly poll this in the background to get the most recent frame due to OpenCV bug(?)

    if let Some(video_width) = config.camera.video_width
        && let Some(video_height) = config.camera.video_height
    {
        cam.lock().unwrap().set(CAP_PROP_FRAME_WIDTH, video_width)?;
        cam.lock()
            .unwrap()
            .set(CAP_PROP_FRAME_HEIGHT, video_height)?;
    }

    let mut cam2: Option<Arc<Mutex<VideoCapture>>> = None;

    let cam_guard = Arc::clone(&cam);
    let cam2_guard: Arc<Mutex<VideoCapture>>;

    let keepalive = Arc::new(Mutex::new(true));

    let thread1_keepalive_guard = Arc::clone(&keepalive);

    match videoio::VideoCapture::is_opened(&cam.lock().unwrap())? {
        true => {}
        false => {
            return Err(anyhow!(
                "Unable to open camera {}! Please select another.",
                config.camera.camera_index_1
            ));
        }
    };

    if !config
        .advanced
        .camera
        .no_background_frame_consumer
        .unwrap_or(false)
    {
        thread::spawn(move || {
            while *thread1_keepalive_guard.lock().unwrap() {
                let mut frame = Mat::default();
                cam_guard.lock().unwrap().read(&mut frame).unwrap();
                thread::sleep(Duration::from_millis(1)); // Give us a chance to grab the lock
            }
        });
    }

    if streamlined {
        debug!("process is streamlined");
        if crop_data.is_none() {
            pos.x1_start = 0;
            pos.y1_start = 0;

            pos.x2_start = Some(0);
            pos.y2_start = Some(0);

            pos.x1_end = cam
                .lock()
                .unwrap()
                .get(opencv::videoio::CAP_PROP_FRAME_WIDTH)
                .unwrap() as i32; // TODO: Real error handling
            pos.y1_end = cam
                .lock()
                .unwrap()
                .get(opencv::videoio::CAP_PROP_FRAME_HEIGHT)
                .unwrap() as i32;
        } else if let Some(crop_data) = crop_data {
            pos.x1_start = crop_data.0.0;
            pos.x1_end = crop_data.0.1;
            pos.y1_start = crop_data.0.2;
            pos.y1_end = crop_data.0.3;

            pos.x2_start = Some(crop_data.1.0);
            pos.x2_end = Some(crop_data.1.1);
            pos.y2_start = Some(crop_data.1.2);
            pos.y2_end = Some(crop_data.1.3);
        }

        if config.camera.multi_camera {
            debug!("Getting second cam limits");
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
                    .set(CAP_PROP_FRAME_WIDTH, video_width)?;
                cam2.as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .set(CAP_PROP_FRAME_HEIGHT, video_height)?;
            }

            pos.x2_end = Some(
                cam2.as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .get(opencv::videoio::CAP_PROP_FRAME_WIDTH)
                    .unwrap() as i32,
            ); // Sometimes OpenCV will silently fail to set the width/height, so we can't rely on config.video_width here
            pos.y2_end = Some(
                cam2.as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .get(opencv::videoio::CAP_PROP_FRAME_HEIGHT)
                    .unwrap() as i32,
            );
        }
    }

    let streamlined = {
        if config.scan.scan_mode == 1 {
            false
        } else {
            streamlined
        }
    };

    let hsv_brightest: core::VecN<u8, 3>;

    (pos.cam_1_brightest, pos.cam_1_darkest, hsv_brightest) = match brightest_darkest(
        &cam,
        &config,
        manager_guard,
        pos.x1_start,
        pos.y1_start,
        pos.x1_end,
        pos.y1_end,
        !streamlined,
    ) {
        Ok((brightest, darkest, hsv_brightest)) => (Some(brightest), Some(darkest), hsv_brightest),
        Err(e) => {
            panic!("There was an issue trying to get the darkest and brightest values: {e}")
        }
    };

    debug!("hsv_brightest: {hsv_brightest:?}");
    debug!(
        "HSV (visualizer-friendly): H: {:.1}, S: {:.1}%, V: {:.1}%",
        hsv_brightest[0] as f32 * 2.0,
        hsv_brightest[1] as f32 / 255.0 * 100.0,
        hsv_brightest[2] as f32 / 255.0 * 100.0
    );

    if config.camera.multi_camera && !streamlined {
        highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
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
                .set(CAP_PROP_FRAME_WIDTH, video_width)?;
            cam2.as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .set(CAP_PROP_FRAME_HEIGHT, video_height)?;
        }

        cam2_guard = Arc::clone(cam2.as_ref().unwrap());

        match videoio::VideoCapture::is_opened(&cam2_guard.lock().unwrap())? {
            true => {}
            false => {
                return Err(anyhow!(
                    "Unable to open camera {}! Please select another.",
                    config.camera.camera_index_2.unwrap()
                ));
            }
        };

        if !config
            .advanced
            .camera
            .no_background_frame_consumer
            .unwrap_or(false)
        {
            let thread2_keepalive_guard = Arc::clone(&keepalive);
            match thread::Builder::new()
                .name("frame_consumer".to_string())
                .spawn(move || {
                    while *thread2_keepalive_guard.lock().unwrap() {
                        let mut frame = Mat::default();
                        cam2_guard.lock().unwrap().read(&mut frame).unwrap();
                        thread::sleep(Duration::from_millis(1)); // Give us a chance to grab the lock
                    }
                }) {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        "Failed to spawn frame_consumer! Scan results may be inaccurate! Error: {e}"
                    )
                }
            };
        }
        // let initial_cal_var = brightest_darkest(cam2.as_ref().unwrap(), &config, manager_guard, pos.x1_start, pos.y1_start, pos.x1_end, pos.y1_end, !streamlined);
        // (pos.cam_2_brightest, pos.cam_2_darkest) = (
        //     Some(match initial_cal_var {
        //         Ok(brightest) => brightest.0,
        //         Err(e) => {
        //             panic!("There was an issue trying to get the darkest and brightest values: {e}");
        //         }
        //     }),
        //     Some(match initial_cal_var {
        //         Ok(darkest) => darkest.1,
        //         Err(e) => {
        //             panic!("There was an issue trying to get the darkest and brightest values: {e}");
        //         }
        //     }),
        // );
    }

    if config.scan.scan_mode == 1 {
        debug!("Setting upper and lower bounds for LED");

        let mut manager = manager_guard.lock().unwrap();
        let filter_color = manager.config.filter_color.unwrap();
        let hsv_override;

        if filter_color == 0 {
            hsv_override = manager.config.hsv_red_override.as_mut();
        } else if filter_color == 1 {
            hsv_override = manager.config.hsv_green_override.as_mut();
        } else if filter_color == 2 {
            hsv_override = manager.config.hsv_blue_override.as_mut();
        } else {
            panic!("{filter_color} is not a valid filter color");
        }

        if hsv_override.is_none() {
            let override_vec = match filter_color {
                0 => &mut manager.config.hsv_red_override,
                1 => &mut manager.config.hsv_green_override,
                2 => &mut manager.config.hsv_blue_override,
                _ => panic!("{filter_color} is not a valid filter color"),
            };

            let range = config.scan.filter_range.unwrap();
            let hue_range = range / 4; // Only use half range for Hue

            let h = hsv_brightest.0[0];
            let s = hsv_brightest.0[1];
            let v = hsv_brightest.0[2];

            // Clamp lower bounds safely
            let lower_h = h.saturating_sub(hue_range).min(179);
            let lower_s = s.saturating_sub(range);
            let lower_v = v.saturating_sub(range);

            // Clamp upper bounds safely
            let upper_h = (h + hue_range).min(179);
            let upper_s = s + range;
            let upper_v = v + range;

            *override_vec = Some(vec![lower_h, lower_s, lower_v, upper_h, upper_s, upper_v]);

            debug!("lower bound: HSV: {lower_h} {lower_s} {lower_v}");
            debug!("upper bound: HSV: {upper_h} {upper_s} {upper_v}");
        } else {
            info!("Existing hsv_override found, not overriding");
        }
    }

    highgui::destroy_all_windows().unwrap();

    let data = Arc::new(Mutex::new(ScanData {
        pos,
        invert: false,
        depth: false,
    }));

    info!("Scan XY");
    let (success, failures, success_cam_2, failures_cam_2) = match scan_area(
        manager_guard,
        &config,
        &cam,
        cam2.as_ref(),
        &mut led_pos,
        data.clone(),
    ) {
        Ok((success, failures, success_cam_2, failures_cam_2)) => {
            (success, failures, success_cam_2, failures_cam_2)
        }
        Err(e) => {
            panic!(
                "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                failed_calibration(led_pos),
                e
            );
        }
    };

    if !config.camera.multi_camera {
        info!("{success} succesful calibrations, {failures} failed calibrations");
    } else {
        info!(
            "First camera: {success} succesful calibrations, {failures} failed calibrations. \nSecond camera: {} succesful calibrations, {} failed calibrations.",
            success_cam_2.unwrap(),
            failures_cam_2.unwrap()
        );
    }

    if failures > 0 && !streamlined && !config.camera.multi_camera {
        // Rescan XY from the back if there are failures
        {
            data.lock().unwrap().invert = true;
        }
        info!(
            "Please rotate the container 180 degrees to recalibrate failures. Press space to continue."
        );
        highgui::set_window_title(
            window,
            "Please rotate the container 180 degrees to recalibrate failures. Press space to continue.",
        )?;
        match wait(data.clone(), &cam, window) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                    failed_calibration(led_pos),
                    e
                );
            }
        }
        let (cam_1_success, cam_1_failures, cam_2_success, cam_2_failures) = match scan_area(
            manager_guard,
            &config,
            &cam,
            cam2.as_ref(),
            &mut led_pos,
            data.clone(),
        ) {
            Ok((cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)) => {
                (cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)
            }
            Err(e) => {
                panic!(
                    "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                    failed_calibration(led_pos),
                    e
                );
            }
        };
        if !config.camera.multi_camera {
            info!("{cam_1_success} succesful calibrations, {cam_1_failures} failed calibrations");
        } else {
            info!(
                "First camera: {cam_1_success} succesful calibrations, {cam_1_failures} failed calibrations. \nSecond camera: {} succesful calibrations, {} failed calibrations.",
                cam_2_success.unwrap(),
                cam_2_failures.unwrap()
            );
        }
        if failures > 0 {
            info!("Entering manual calibration mode!");
            match manual_calibrate(manager_guard, &config, window, &cam, &mut led_pos, &data) {
                Ok(_) => {}
                Err(e) => {
                    panic!("Something went wrong during manual calibration: {e}");
                }
            }
        }
        info!("Please rotate the container 270 degrees to calibrate Z. Press space to continue."); // The LEDS will be 180 degrees away from the original position, and they need to be rotated 270 degrees in this case to go to the appropriate Z calibration position.
        highgui::set_window_title(
            window,
            "Please rotate the container 270 degrees to calibrate Z. Press space to continue.",
        )?;
    }

    if !config.camera.multi_camera {
        if failures == 0 && !streamlined {
            info!(
                "Please rotate the container 90 degrees to calibrate Z. Press space to continue."
            );
            highgui::set_window_title(
                window,
                "Please rotate the container 90 degrees to calibrate Z. Press space to continue.",
            )?;
        }
        match wait(data.clone(), &cam, window) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                    failed_calibration(led_pos),
                    e
                );
            }
        }

        info!("Scan Z");
        {
            data.lock().unwrap().invert = false;
            data.lock().unwrap().depth = true;
        }

        let (success, failures, success_cam_2, failures_cam_2) = match scan_area(
            manager_guard,
            &config,
            &cam,
            cam2.as_ref(),
            &mut led_pos,
            data.clone(),
        ) {
            Ok((cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)) => {
                (cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)
            }
            Err(e) => {
                panic!(
                    "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                    failed_calibration(led_pos),
                    e
                );
            }
        };

        if !config.camera.multi_camera {
            info!("{success} succesful calibrations, {failures} failed calibrations");
        } else {
            info!(
                "First camera: {success} succesful calibrations, {failures} failed calibrations. \nSecond camera: {} succesful calibrations, {} failed calibrations.",
                success_cam_2.unwrap(),
                failures_cam_2.unwrap()
            );
        }

        if failures > 0 {
            {
                data.lock().unwrap().invert = true;
            }
            info!(
                "Please rotate the container 180 degrees to recalibrate failures. Press space to continue."
            );
            highgui::set_window_title(
                window,
                "Please rotate the container 180 degrees to recalibrate failures. Press space to continue.",
            )?;
            match wait(data.clone(), &cam, window) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                        failed_calibration(led_pos),
                        e
                    );
                }
            }
            let (cam_1_success, cam_1_failures, cam_2_success, cam_2_failures) = match scan_area(
                manager_guard,
                &config,
                &cam,
                cam2.as_ref(),
                &mut led_pos,
                data.clone(),
            ) {
                Ok((cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)) => {
                    (cam_1_success, cam_1_failures, cam_2_success, cam_2_failures)
                }
                Err(e) => {
                    panic!(
                        "There was an error trying to scan the XY portion. The data that has been gathered so far has been saved to {}. The error was: {}",
                        failed_calibration(led_pos),
                        e
                    );
                }
            };
            if !config.camera.multi_camera {
                info!(
                    "{cam_1_success} succesful calibrations, {cam_1_failures} failed calibrations"
                );
            } else {
                info!(
                    "First camera: {cam_1_success} succesful calibrations, {cam_1_failures} failed calibrations. \nSecond camera: {} succesful calibrations, {} failed calibrations.",
                    cam_2_success.unwrap(),
                    cam_2_failures.unwrap()
                );
            }
            if failures > 0 {
                info!("Entering manual calibration mode!");
                match manual_calibrate(
                    manager_guard,
                    &config,
                    window,
                    &cam,
                    &mut led_pos,
                    &data.clone(),
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        panic!("Something went wrong during manual calibration: {e}");
                    }
                }
            }
        }
    }

    *keepalive.lock().unwrap() = false;
    cam.lock().unwrap().release().unwrap();
    if config.camera.multi_camera {
        cam2.unwrap().lock().unwrap().release().unwrap();
    }

    highgui::destroy_all_windows().unwrap();

    post_process(&mut led_pos, manager_guard.lock().unwrap().config.num_led);

    if let Some(adjust) = config.advanced.transform.adjust_after_scan
        && adjust
    {
        info!("Performing position adjustment");
        position_adjustment(&mut led_pos, &config);
    }

    if !streamlined {
        loop {
            let date = Local::now();
            let name = inquire::Text::new(&format!(
                "File name:({}-ledpos.json)",
                date.format("%Y-%m-%d-%H:%M:%S")
            ))
            .prompt();

            match name {
                Ok(mut name) => {
                    if name.is_empty() {
                        name = format!("{}-ledpos.json", date.format("%Y-%m-%d-%H:%M:%S"));
                    }
                    let json = serde_json::to_string_pretty(&led_pos)
                        .expect("Unable to serialize metadata!");
                    let mut file = match File::create(Path::new(&name)) {
                        Ok(file) => file,
                        Err(e) => {
                            error!("Unable to write temp-pos to {name}");
                            println!(
                                "Something went wrong trying to save the LED positions. What has been collected has been written to {}. Error: {}",
                                failed_calibration(led_pos.clone()),
                                e
                            );
                            process::exit(1);
                        }
                    };

                    match file.write_all(json.as_bytes()) {
                        Ok(_) => {}
                        Err(e) => {
                            error!("Unable to write temp-pos to {name}");
                            println!(
                                "Something went wrong trying to save the LED positions. What has been collected has been written to {}. Error: {}",
                                failed_calibration(led_pos.clone()),
                                e
                            );
                        }
                    }
                    break;
                }
                Err(_) => println!(
                    "Something went wrong trying to save the LED positions. What has been collected has been written to {}.",
                    failed_calibration(led_pos.clone())
                ),
            };
        }
    } else {
        let name = "streamlined.json";

        let json = serde_json::to_string_pretty(&led_pos).expect("Unable to serialize metadata!");
        let mut file = match File::create(Path::new(&name)) {
            Ok(file) => file,
            Err(e) => {
                error!("Unable to write temp-pos to {name}");
                println!(
                    "Something went wrong trying to save the LED positions. What has been collected has been written to {}. Error: {}",
                    failed_calibration(led_pos.clone()),
                    e
                );
                process::exit(1);
            }
        };

        match file.write_all(json.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                error!("Unable to write temp-pos to {name}");
                println!(
                    "Something went wrong trying to save the LED positions. What has been collected has been written to {}. Error: {}",
                    failed_calibration(led_pos.clone()),
                    e
                );
            }
        }

        info!("Scan exiting!");
    }

    Ok(())
}

fn brightest_darkest(
    cam: &Arc<Mutex<VideoCapture>>,
    config: &Config,
    manager: &Arc<Mutex<ManagerData>>,
    x_start: i32,
    y_start: i32,
    x_end: i32,
    y_end: i32,
    prompt: bool,
) -> Result<(f64, f64, core::VecN<u8, 3>), Box<dyn Error>> {
    debug!("Getting brightest and darkest points");

    let filter_color = manager.lock().unwrap().config.filter_color.unwrap();
    let scan_mode = manager.lock().unwrap().config.scan_mode;

    let brightness = config.scan.color_bright.unwrap();
    if scan_mode == 1 {
        debug!("using color filter for brightest darkest");
        if filter_color == 0 {
            led_manager::set_color(manager, 5, brightness, 0, 0);
        } else if filter_color == 1 {
            led_manager::set_color(manager, 5, 0, brightness, 0);
        } else if filter_color == 2 {
            led_manager::set_color(manager, 5, 0, 0, brightness);
        }
    } else {
        led_manager::set_color(manager, 5, brightness, brightness, brightness);
    }

    info!("Collecting brightest and darkest points, please wait...");

    debug!("getting frame");
    let mut frame = Mat::default();
    cam.lock().unwrap().read(&mut frame)?;

    if scan_mode == 1 {
        filter(&mut frame, &filter_color, manager);
    }

    let brightest: u8;
    let hsv;
    let mut image_hsv: Mat = Default::default();

    if !prompt {
        let brightest_pos;

        imgproc::cvt_color(
            &frame,
            &mut image_hsv,
            COLOR_BGR2HSV,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap();

        let frame = Mat::roi(
            &frame,
            opencv::core::Rect {
                x: x_start,
                y: y_start,
                width: x_end - x_start,
                height: y_end - y_start,
            },
        )?;

        let brightest_result = get_brightest_cam_1_pos(frame.try_clone()?);
        (_, brightest, brightest_pos) = (
            brightest_result.0,
            brightest_result.1 as u8,
            brightest_result.2,
        );

        hsv = image_hsv
            .at_2d::<opencv::core::Vec3b>(brightest_pos.y, brightest_pos.x)
            .unwrap();
    } else {
        let select_brightest_result = select_brightest(cam, manager, config).unwrap();
        let brightest_pos = Point::new(select_brightest_result.0, select_brightest_result.1);

        imgproc::cvt_color(
            &frame,
            &mut image_hsv,
            COLOR_BGR2HSV,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap(); // Used to get our HSV

        imgproc::cvt_color(
            &frame.clone(),
            &mut frame,
            COLOR_BGR2GRAY,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap();

        brightest = *frame.at_2d::<u8>(brightest_pos.y, brightest_pos.x).unwrap();
        hsv = image_hsv
            .at_2d::<opencv::core::Vec3b>(brightest_pos.y, brightest_pos.x)
            .unwrap();

        debug!("brightest from manual select is {brightest}");
        debug!("hsv from manual select is {hsv:?}");
        debug!("pos from manual select is {brightest_pos:?}");
    }

    debug!("get darkest_cam_1");
    led_manager::set_color(manager, 5, 0, 0, 0);

    let mut frame = Mat::default();
    cam.lock().unwrap().read(&mut frame)?;
    if scan_mode == 1 {
        filter(&mut frame, &filter_color, manager);
    }

    let frame = Mat::roi(
        &frame,
        opencv::core::Rect {
            x: x_start,
            y: y_start,
            width: x_end - x_start,
            height: y_end - y_start,
        },
    )?;
    let (_, darkest, _) = get_brightest_cam_1_pos(frame.try_clone()?);

    Ok((brightest as f64, darkest, *hsv))
}

pub fn select_brightest(
    cam: &Arc<Mutex<VideoCapture>>,
    manager: &Arc<Mutex<ManagerData>>,
    config: &Config,
) -> Result<(i32, i32), Box<dyn Error>> {
    let x1 = Arc::new(Mutex::new(0));
    let y1 = Arc::new(Mutex::new(0));

    let h_lower = Arc::new(Mutex::new(0));
    let s_lower = Arc::new(Mutex::new(0));
    let v_lower = Arc::new(Mutex::new(0));

    let h_upper = Arc::new(Mutex::new(0));
    let s_upper = Arc::new(Mutex::new(0));
    let v_upper = Arc::new(Mutex::new(0));

    let override_active = Arc::new(Mutex::new(true)); // Have we already overidden the HSV upper and lower limits from a click?

    let x1_guard = Arc::clone(&x1);
    let y1_guard = Arc::clone(&y1);

    let h_lower_guard = Arc::clone(&h_lower);
    let s_lower_guard = Arc::clone(&s_lower);
    let v_lower_guard = Arc::clone(&v_lower);

    let h_upper_guard = Arc::clone(&h_upper);
    let s_upper_guard = Arc::clone(&s_upper);
    let v_upper_guard = Arc::clone(&v_upper);

    let override_active_guard = Arc::clone(&override_active);

    let window = "Color Calibration";

    highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    highgui::set_mouse_callback(
        window,
        Some(Box::new(move |event, x, y, _flag| {
            if event == EVENT_LBUTTONUP {
                debug!("lbuttonup");
                *override_active_guard.lock().unwrap() = false;
                *x1_guard.lock().unwrap() = x;
                *y1_guard.lock().unwrap() = y;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Hue Upper Limit",
        window,
        None,
        179,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = h_upper_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Saturation Upper Limit",
        window,
        None,
        255,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = s_upper_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Value Upper Limit",
        window,
        None,
        255,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = v_upper_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Hue Lower Limit",
        window,
        None,
        179,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = h_lower_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Saturation Lower Limit",
        window,
        None,
        255,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = s_lower_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    highgui::create_trackbar(
        "Value Lower Limit",
        window,
        None,
        255,
        Some(Box::new(move |pos| {
            if let Ok(mut v) = v_lower_guard.lock() {
                *v = pos;
            }
        })),
    )?;

    debug!("Starting filter selector");

    let mut cam = cam.lock().unwrap();
    highgui::set_window_title(window, "Modify upper and lower bounds").unwrap();

    match videoio::VideoCapture::is_opened(&cam)? {
        true => {}
        false => {
            panic!("Unable to open camera!")
        }
    };

    let mut manager = manager.lock().unwrap();

    loop {
        let mut frame = &mut manager.vision.frame_cam_1;
        cam.read(&mut frame)?;

        // debug!("callback_loop in brightness select");
        let x_guard = *x1.lock().unwrap();
        let y_guard = *y1.lock().unwrap();

        let pos = Point::new(x_guard, y_guard);

        imgproc::circle(
            &mut frame,
            pos,
            20,
            Scalar::new(0.0, 255.0, 0.0, 255.0),
            2,
            LINE_8,
            0,
        )?;

        let mut image_hsv: Mat = Default::default();
        let brightest_pos = Point::new(x_guard, y_guard);

        imgproc::cvt_color(
            frame,
            &mut image_hsv,
            COLOR_BGR2HSV,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap(); // Used to get our HSV

        let hsv = image_hsv
            .at_2d::<opencv::core::Vec3b>(brightest_pos.y, brightest_pos.x)
            .unwrap();

        debug!("hsv from manual select is {hsv:?}");
        debug!("pos from manual select is {brightest_pos:?}");

        // Set hsv upper and lower based on the selected area automatically
        if !*override_active.lock().unwrap() {
            let h = hsv.0[0];
            let s = hsv.0[1];
            let v = hsv.0[2];

            let range = config.scan.filter_range.unwrap();
            let hue_range = range / 4; // Only use half range for Hue

            {
                debug!("overriding temporary hsv from selection");
                *override_active.lock().unwrap() = true;

                let mut h_lower_override = h_lower.lock().unwrap();
                let mut s_lower_override = s_lower.lock().unwrap();
                let mut v_lower_override = v_lower.lock().unwrap();

                let mut h_upper_override = h_upper.lock().unwrap();
                let mut s_upper_override = s_upper.lock().unwrap();
                let mut v_upper_override = v_upper.lock().unwrap();

                // Clamp lower bounds safely
                *h_lower_override = h.saturating_sub(hue_range).min(179) as i32;
                *s_lower_override = s.saturating_sub(range) as i32;
                *v_lower_override = v.saturating_sub(range) as i32;

                // Clamp upper bounds safely without overflow
                *h_upper_override = (h + hue_range).min(179) as i32;
                *s_upper_override = s.saturating_add(range) as i32;
                *v_upper_override = v.saturating_add(range) as i32;
            }
            debug!("Setting trackbar_pos");
            // We can't use *h_lower_override directly since the callback will try to lock it, while it's already locked and hang.
            highgui::set_trackbar_pos("Hue Upper Limit", window, (h + hue_range).min(179) as i32)
                .unwrap();
            highgui::set_trackbar_pos(
                "Saturation Upper Limit",
                window,
                s.saturating_add(range) as i32,
            )
            .unwrap();
            highgui::set_trackbar_pos("Value Upper Limit", window, v.saturating_add(range) as i32)
                .unwrap();

            highgui::set_trackbar_pos(
                "Hue Lower Limit",
                window,
                h.saturating_sub(hue_range).min(179) as i32,
            )
            .unwrap();
            highgui::set_trackbar_pos(
                "Saturation Lower Limit",
                window,
                s.saturating_sub(range) as i32,
            )
            .unwrap();
            highgui::set_trackbar_pos("Value Lower Limit", window, v.saturating_sub(range) as i32)
                .unwrap();
        }

        // Apply HSV filter bounds
        let h_up = *h_upper.lock().unwrap();
        let s_up = *s_upper.lock().unwrap();
        let v_up = *v_upper.lock().unwrap();

        let h_lo = *h_lower.lock().unwrap();
        let s_lo = *s_lower.lock().unwrap();
        let v_lo = *v_lower.lock().unwrap();

        // Need this so the value lives long enough
        let upper_vals = [h_up, s_up, v_up];
        let lower_vals = [h_lo, s_lo, v_lo];

        let upperb = Mat::from_slice(&upper_vals).unwrap();
        let lowerb = Mat::from_slice(&lower_vals).unwrap();

        debug!("applying upper and lower vals in select_brightest: {upper_vals:?} {lower_vals:?}");
        let mut hsv_frame = Mat::default();
        imgproc::cvt_color(
            frame,
            &mut hsv_frame,
            imgproc::COLOR_BGR2HSV,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap();

        let mut mask = Mat::default();
        core::in_range(&hsv_frame, &lowerb, &upperb, &mut mask).unwrap();

        let mut mask_color = Mat::default();
        imgproc::cvt_color(
            &mask,
            &mut mask_color,
            imgproc::COLOR_GRAY2BGR,
            0,
            get_default_algorithm_hint().unwrap(),
        )
        .unwrap();

        core::add_weighted(&frame.clone(), 0.5, &mask_color, 0.7, 0.0, frame, -1).unwrap();

        if let Some(no_video) = config.advanced.camera.no_video {
            if frame.size()?.width > 0 && !no_video {
                highgui::imshow(window, frame)?;
            } else {
                warn!("frame is too small!");
            }
        }

        let key = highgui::wait_key(10)?;
        if key == 32 && key != 255 {
            if x_guard != 0 && y_guard != 0 {
                let filter_color = manager.config.filter_color.unwrap();
                // let hsv_override ;

                // if filter_color == 0 {
                //     hsv_override = manager.hsv_red_override.as_mut();
                // } else if filter_color == 1 {
                //     hsv_override = manager.hsv_green_override.as_mut();
                // } else if filter_color == 2 {
                //     hsv_override = manager.hsv_blue_override.as_mut();
                // } else {
                //     panic!("{filter_color} is not a valid filter color");
                // }

                let override_vec = match filter_color {
                    0 => &mut manager.config.hsv_red_override,
                    1 => &mut manager.config.hsv_green_override,
                    2 => &mut manager.config.hsv_blue_override,
                    _ => panic!("{filter_color} is not a valid filter color"),
                };

                *override_vec = Some(vec![
                    h_lo.try_into().unwrap(),
                    s_lo.try_into().unwrap(),
                    v_lo.try_into().unwrap(),
                    h_up.try_into().unwrap(),
                    s_up.try_into().unwrap(),
                    v_up.try_into().unwrap(),
                ]);

                debug!("Overriding hsv_override vec with {override_vec:?}");

                break;
            } else {
                error!("Please select a valid area!");
            }
        }
    }

    debug!("select_brightest finished");

    // Also needed to let the value live long enough
    Ok((*x1.lock().unwrap(), *y1.lock().unwrap()))
}

pub fn crop(config: &Config, manager: &Arc<Mutex<ManagerData>>) -> Result<CropPos, Box<dyn Error>> {
    if config.advanced.transform.crop_override.is_some() {
        let crop_override = config.advanced.transform.crop_override.clone().unwrap();
        if config.camera.multi_camera {
            if crop_override.len() == 8 {
                return Ok(CropPos {
                    x1_start: crop_override[0],
                    y1_start: crop_override[1],
                    x1_end: crop_override[2],
                    y1_end: crop_override[3],
                    x2_start: Some(crop_override[4]),
                    y2_start: Some(crop_override[5]),
                    x2_end: Some(crop_override[6]),
                    y2_end: Some(crop_override[7]),
                    cam_1_brightest: None,
                    cam_1_darkest: None,
                    cam_2_brightest: None,
                    cam_2_darkest: None,
                });
            } else {
                error!("crop_override needs 8 elements to use with multiple cameras.");
            }
        } else {
            return Ok(CropPos {
                x1_start: crop_override[0],
                y1_start: crop_override[1],
                x1_end: crop_override[2],
                y1_end: crop_override[3],
                x2_start: None,
                y2_start: None,
                x2_end: None,
                y2_end: None,
                cam_1_brightest: None,
                cam_1_darkest: None,
                cam_2_brightest: None,
                cam_2_darkest: None,
            });
        }
    }
    let window = "Calibration";

    let x1_start = Arc::new(Mutex::new(0));
    let y1_start = Arc::new(Mutex::new(0));
    let x1_end = Arc::new(Mutex::new(0));
    let y1_end = Arc::new(Mutex::new(0));

    let x2_start = Arc::new(Mutex::new(0));
    let y2_start = Arc::new(Mutex::new(0));
    let x2_end = Arc::new(Mutex::new(0));
    let y2_end = Arc::new(Mutex::new(0));

    let camera_active = Arc::new(Mutex::new(0)); // 0 for first, 1 for second.

    let x1_start_guard = Arc::clone(&x1_start);
    let y1_start_guard = Arc::clone(&y1_start);
    let x1_end_guard = Arc::clone(&x1_end);
    let y1_end_guard = Arc::clone(&y1_end);

    let x2_start_guard = Arc::clone(&x2_start);
    let y2_start_guard = Arc::clone(&y2_start);
    let x2_end_guard = Arc::clone(&x2_end);
    let y2_end_guard = Arc::clone(&y2_end);

    let camera_active_guard = Arc::clone(&camera_active);

    highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    set_crop_mouse_callback(
        window,
        camera_active_guard.clone(),
        x1_start_guard.clone(),
        y1_start_guard.clone(),
        x1_end_guard.clone(),
        y1_end_guard.clone(),
        x2_start_guard.clone(),
        y2_start_guard.clone(),
        x2_end_guard.clone(),
        y2_end_guard.clone(),
    )?;

    let cam_guard = Arc::new(Mutex::new(
        get_cam(config, &config.camera.camera_index_1).unwrap(),
    )); // callback_loop only accepts a Arc<Mutex<VideoCapture>>

    if let Some(video_width) = config.camera.video_width
        && let Some(video_height) = config.camera.video_height
    {
        cam_guard
            .lock()
            .unwrap()
            .set(CAP_PROP_FRAME_WIDTH, video_height)?;
        cam_guard
            .lock()
            .unwrap()
            .set(CAP_PROP_FRAME_HEIGHT, video_width)?;
    }

    match videoio::VideoCapture::is_opened(&cam_guard.lock().unwrap())? {
        true => {}
        false => {
            panic!(
                "Unable to open camera {}! Please select another.",
                config.camera.camera_index_1
            )
        }
    };

    let x_start_result;
    let x_end_result;
    let y_start_result;
    let y_end_result;

    let mut x2_start_result = None;
    let mut x2_end_result = None;
    let mut y2_start_result = None;
    let mut y2_end_result = None;

    debug!("starting callback_loop for first camera.");
    (x_start_result, x_end_result, y_start_result, y_end_result) = match callback_loop(
        &cam_guard,
        manager,
        x1_start.clone(),
        y1_start.clone(),
        Some(x1_end.clone()),
        Some(y1_end.clone()),
        window,
        "Please drag the mouse around the container. Press space to continue".to_string(),
        true,
    ) {
        Ok((x_start, x_end, y_start, y_end)) => (x_start, x_end, y_start, y_end),
        Err(e) => panic!("Something went wrong during cropping: {e}"),
    };

    drop(cam_guard);

    highgui::named_window(window, highgui::WINDOW_AUTOSIZE)?;
    set_crop_mouse_callback(
        window,
        camera_active_guard.clone(),
        x1_start_guard.clone(),
        y1_start_guard.clone(),
        x1_end_guard.clone(),
        y1_end_guard.clone(),
        x2_start_guard.clone(),
        y2_start_guard.clone(),
        x2_end_guard.clone(),
        y2_end_guard.clone(),
    )?;

    if let Some(index) = &config.camera.camera_index_2 {
        debug!("Cropping second camera with index {index}");
        *camera_active.lock().unwrap() = 1;
        let cam_guard = Arc::new(Mutex::new(get_cam(config, index).unwrap()));

        if let Some(video_width) = config.camera.video_width
            && let Some(video_height) = config.camera.video_height
        {
            cam_guard
                .lock()
                .unwrap()
                .set(CAP_PROP_FRAME_WIDTH, video_width)?;
            cam_guard
                .lock()
                .unwrap()
                .set(CAP_PROP_FRAME_HEIGHT, video_height)?;
        }

        match videoio::VideoCapture::is_opened(&cam_guard.lock().unwrap())? {
            true => {}
            false => {
                panic!("Unable to open camera {index}! Please select another.")
            }
        };

        let loop_out = callback_loop(
            &cam_guard,
            manager,
            x2_start,
            y2_start,
            Some(x2_end),
            Some(y2_end),
            window,
            "Please drag the mouse around the second container. Press space to continue"
                .to_string(),
            true,
        )
        .unwrap();
        (
            x2_start_result,
            x2_end_result,
            y2_start_result,
            y2_end_result,
        ) = (Some(loop_out.0), Some(loop_out.1), loop_out.2, loop_out.3);
    }

    let pos = CropPos {
        x1_start: x_start_result,
        y1_start: y_start_result.unwrap(),
        x1_end: x_end_result,
        y1_end: y_end_result.unwrap(),
        x2_start: x2_start_result,
        y2_start: y2_start_result,
        x2_end: x2_end_result,
        y2_end: y2_end_result,
        cam_1_brightest: None,
        cam_1_darkest: None,
        cam_2_brightest: None,
        cam_2_darkest: None,
    };
    debug!("crop finished, returning: {pos:?}");

    Ok(pos)
}

pub fn set_crop_mouse_callback(
    window: &str,
    camera_active_guard: Arc<Mutex<i32>>,
    x1_start_guard: Arc<Mutex<i32>>,
    y1_start_guard: Arc<Mutex<i32>>,
    x1_end_guard: Arc<Mutex<i32>>,
    y1_end_guard: Arc<Mutex<i32>>,
    x2_start_guard: Arc<Mutex<i32>>,
    y2_start_guard: Arc<Mutex<i32>>,
    x2_end_guard: Arc<Mutex<i32>>,
    y2_end_guard: Arc<Mutex<i32>>,
) -> opencv::Result<()> {
    let mut actively_cropping = false;

    highgui::set_mouse_callback(
        window,
        Some(Box::new(move |event, x, y, _flag| match event {
            #[allow(non_snake_case)]
            EVENT_LBUTTONDOWN => {
                debug!("lbuttondown");
                actively_cropping = true;
                match *camera_active_guard.lock().unwrap() {
                    0 => {
                        *x1_start_guard.lock().unwrap() = x;
                        *y1_start_guard.lock().unwrap() = y;
                    }
                    1 => {
                        *x2_start_guard.lock().unwrap() = x;
                        *y2_start_guard.lock().unwrap() = y;
                    }
                    _ => {}
                }
            }
            #[allow(non_snake_case)]
            EVENT_LBUTTONUP => {
                debug!("lbuttonup");
                actively_cropping = false;
                match *camera_active_guard.lock().unwrap() {
                    0 => {
                        *x1_end_guard.lock().unwrap() = x;
                        *y1_end_guard.lock().unwrap() = y;
                    }
                    1 => {
                        *x2_end_guard.lock().unwrap() = x;
                        *y2_end_guard.lock().unwrap() = y;
                    }
                    _ => {}
                }
            }
            #[allow(non_snake_case)]
            EVENT_MOUSEMOVE if actively_cropping => match *camera_active_guard.lock().unwrap() {
                0 => {
                    *x1_end_guard.lock().unwrap() = x;
                    *y1_end_guard.lock().unwrap() = y;
                }
                1 => {
                    *x2_end_guard.lock().unwrap() = x;
                    *y2_end_guard.lock().unwrap() = y;
                }
                _ => {}
            },
            _ => {}
        })),
    )
}

fn callback_loop(
    cam: &Arc<Mutex<VideoCapture>>,
    manager: &Arc<Mutex<ManagerData>>,
    x_start: Arc<Mutex<i32>>,
    y_start: Arc<Mutex<i32>>,
    x_end: Option<Arc<Mutex<i32>>>,
    y_end: Option<Arc<Mutex<i32>>>,
    window: &str,
    msg: String,
    crop: bool,
) -> Result<CallbackResult, Box<dyn Error>> {
    info!("{msg}");
    debug!("window: {window}, title: {msg}");
    let mut cam = cam.lock().unwrap();
    highgui::set_window_title(window, &msg).unwrap();

    match videoio::VideoCapture::is_opened(&cam)? {
        true => {}
        false => {
            panic!("Unable to open camera!")
        }
    };

    let no_video = manager.lock().unwrap().config.no_video;
    let mut manager = manager.lock().unwrap();

    loop {
        let mut frame = &mut manager.vision.frame_cam_1;
        cam.read(&mut frame)?;

        if crop {
            debug!("callback_loop in crop mode");
            debug!(
                "frame width: {} height: {}",
                frame.size()?.width,
                frame.size()?.height
            );

            let x_end = x_end.clone().unwrap();
            let y_end = y_end.clone().unwrap();

            let x_end_guard = *x_end.lock().unwrap();
            let y_end_guard = *y_end.lock().unwrap();
            let x_start_guard = *x_start.lock().unwrap();
            let y_start_guard = *y_start.lock().unwrap();

            let rect = core::Rect::new(
                x_start_guard,
                y_start_guard,
                x_end_guard - x_start_guard,
                y_end_guard - y_start_guard,
            );

            imgproc::rectangle(
                &mut frame,
                rect,
                core::Scalar::new(255.0, 0.0, 0.0, 0.0),
                3,
                8,
                0,
            )
            .expect("Could not draw a rectangle");

            debug!("crop rectangle is {rect:?}");

            if let Some(no_video) = no_video {
                if frame.size()?.width > 0 && !no_video {
                    highgui::imshow(window, frame)?;
                } else {
                    error!(
                        "frame is too small or no_video is true! size: {:?} rect: {:?}, ",
                        frame.size()?,
                        rect
                    );
                }
            }

            let key = highgui::wait_key(10)?;
            if key == 32 && key != 255 {
                if x_start_guard != 0 && x_end_guard != 0 {
                    highgui::destroy_all_windows().unwrap();

                    break Ok((
                        x_start_guard,
                        x_end_guard,
                        Some(y_start_guard),
                        Some(y_end_guard),
                    ));
                } else {
                    error!("Please select a valid are for the crop");
                }
            }
        } else {
            // debug!("callback_loop in brightness select");
            let x_guard = *x_start.lock().unwrap();
            let y_guard = *y_start.lock().unwrap();

            let pos = Point::new(x_guard, y_guard);

            imgproc::circle(
                &mut frame,
                pos,
                20,
                Scalar::new(0.0, 255.0, 0.0, 255.0),
                2,
                LINE_8,
                0,
            )?;

            let mut image_hsv: Mat = Default::default();
            let brightest_pos = Point::new(x_guard, y_guard);

            imgproc::cvt_color(
                frame,
                &mut image_hsv,
                COLOR_BGR2HSV,
                0,
                get_default_algorithm_hint().unwrap(),
            )
            .unwrap(); // Used to get our HSV

            let hsv = image_hsv
                .at_2d::<opencv::core::Vec3b>(brightest_pos.y, brightest_pos.x)
                .unwrap();

            debug!("hsv from manual select is {hsv:?}");
            debug!("pos from manual select is {brightest_pos:?}");

            if let Some(no_video) = no_video {
                if frame.size()?.width > 0 && !no_video {
                    highgui::imshow(window, frame)?;
                } else {
                    warn!("frame is too small!");
                }
            }

            let key = highgui::wait_key(10)?;
            if key == 32 && key != 255 {
                if x_guard != 0 && y_guard != 0 {
                    // highgui::destroy_all_windows().unwrap();
                    break Ok((x_guard, y_guard, None, None));
                } else {
                    error!("Please select a valid area!");
                }
            }
        }
    }
}

pub fn get_brightest_cam_1_pos(mut frame: Mat) -> (f64, f64, Point) {
    debug!("Frame channels: {}", frame.channels());

    imgproc::gaussian_blur(
        // Blur frame to increase accuracy of min_max_loc
        &frame.clone(),
        &mut frame,
        core::Size::new(41, 41),
        0.0,
        0.0,
        0,
        get_default_algorithm_hint().unwrap(),
    )
    .unwrap();

    imgproc::cvt_color(
        &frame.clone(),
        &mut frame,
        COLOR_BGR2GRAY,
        0,
        get_default_algorithm_hint().unwrap(),
    )
    .unwrap(); // Greyscales frame

    let mut min_val = 0.0;
    let mut max_val = 0.0;
    let mut max_loc = Point::new(0, 0);

    min_max_loc(
        &frame,
        Some(&mut min_val),
        Some(&mut max_val),
        None,
        Some(&mut max_loc),
        &no_array(),
    )
    .unwrap();

    (min_val, max_val, max_loc)
}

pub fn scan_area(
    manager: &Arc<Mutex<ManagerData>>,
    config: &Config,
    cam: &Arc<Mutex<videoio::VideoCapture>>,
    cam2: Option<&Arc<Mutex<videoio::VideoCapture>>>,
    led_pos: &mut PosEntry,
    scan_data: Arc<Mutex<ScanData>>,
) -> ScanResult {
    let scan_data = &mut *scan_data.lock().unwrap();

    let cam_1_window = "Camera 1";
    let cam_2_window = "Camera 2";

    if let Some(no_video) = config.advanced.camera.no_video {
        if !no_video {
            highgui::named_window(cam_1_window, highgui::WINDOW_AUTOSIZE)?;
        }

        if config.camera.multi_camera && !no_video {
            highgui::named_window(cam_2_window, highgui::WINDOW_AUTOSIZE)?;
        }
    }

    let mut success = 0;
    let mut failures = 0;

    let mut cam_2_success = None;
    let mut cam_2_failures = None;

    for i in 0..config.num_led {
        let valid_cycle = if scan_data.depth {
            led_pos[i as usize].0 != "SUCCESS-Z"
        } else {
            !led_pos[i as usize].0.contains("SUCCESS")
        };

        if valid_cycle {
            debug!("calling scan_area_cycle for cam1");
            (success, failures) = scan_area_cycle(
                manager,
                config,
                Some(cam),
                scan_data,
                led_pos,
                i,
                false,
                cam_1_window,
            )
            .unwrap();

            if config.camera.multi_camera {
                debug!("calling scan_area_cycle for cam2");
                let scan_area_result = scan_area_cycle(
                    manager,
                    config,
                    cam2,
                    scan_data,
                    led_pos,
                    i,
                    true,
                    cam_2_window,
                )
                .unwrap();
                (cam_2_success, cam_2_failures) =
                    (Some(scan_area_result.0), Some(scan_area_result.1));
            }
        }
    }
    Ok((success, failures, cam_2_success, cam_2_failures))
}

// TODO: WIP
// fn dim_until_no_white(mut frame: &mut Mat) {
//     let mut gray_frame = Mat::default();
//     let mut frame = frame.clone();

//     // Step 1: Convert the frame to grayscale
//     imgproc::cvt_color(&frame, &mut gray_frame, imgproc::COLOR_BGR2GRAY, 0, get_default_algorithm_hint().unwrap()).unwrap();

//     // Step 2: Threshold the image to detect "white" regions
//     let mut white_mask = Mat::default();
//     imgproc::threshold(&gray_frame, &mut white_mask, 240.0, 255.0, imgproc::THRESH_BINARY).unwrap();

//     // Step 3: Start dimming process
//     let mut brightness_factor = 1.0;

//     while !white_mask.empty() {
//         // Reduce brightness gradually by multiplying with a factor
//         let mut dimmed = Mat::default();
//         frame.convert_to(&mut dimmed, -1, brightness_factor, 0.0).unwrap();

//         // Update frame with the dimmed image
//         frame = dimmed.clone();

//         // Update the white mask with the new dimmed image
//         imgproc::cvt_color(&frame, &mut gray_frame, imgproc::COLOR_BGR2GRAY, 0, get_default_algorithm_hint().unwrap()).unwrap();
//         imgproc::threshold(&gray_frame, &mut white_mask, 240.0, 255.0, imgproc::THRESH_BINARY).unwrap();

//         // Reduce the brightness factor further
//         brightness_factor -= 0.05;

//         // If the brightness_factor is too low, stop the loop to avoid making it too dark.
//         if brightness_factor <= 0.05 {
//             break;
//         }
//     }
// }

pub fn filter(mut frame: &mut Mat, filter_color: &u32, manager: &Arc<Mutex<ManagerData>>) {
    let manager = manager.lock().unwrap();
    // Filters frame for color
    let lowerb;
    let upperb;

    let tmp_array_lower: Vec<u8>;
    let tmp_array_upper: Vec<u8>;

    if *filter_color == 0 {
        if let Some(tmp_override) = &manager.config.hsv_red_override {
            tmp_array_lower = vec![tmp_override[0], tmp_override[1], tmp_override[2]];
            tmp_array_upper = vec![tmp_override[3], tmp_override[4], tmp_override[5]];

            lowerb = Mat::from_slice(&tmp_array_lower);
            upperb = Mat::from_slice(&tmp_array_upper);
        } else {
            lowerb = Mat::from_slice(&[0, 100, 100]);
            upperb = Mat::from_slice(&[5, 255, 255]);
        }
    } else if *filter_color == 1 {
        if let Some(tmp_override) = &manager.config.hsv_green_override {
            tmp_array_lower = vec![tmp_override[0], tmp_override[1], tmp_override[2]];
            tmp_array_upper = vec![tmp_override[3], tmp_override[4], tmp_override[5]];

            lowerb = Mat::from_slice(&tmp_array_lower);
            upperb = Mat::from_slice(&tmp_array_upper);
        } else {
            lowerb = Mat::from_slice(&[35, 100, 100]);
            upperb = Mat::from_slice(&[85, 255, 255]);
        }
    } else if *filter_color == 2 {
        if let Some(tmp_override) = &manager.config.hsv_blue_override {
            tmp_array_lower = vec![tmp_override[0], tmp_override[1], tmp_override[2]];
            tmp_array_upper = vec![tmp_override[3], tmp_override[4], tmp_override[5]];

            lowerb = Mat::from_slice(&tmp_array_lower);
            upperb = Mat::from_slice(&tmp_array_upper);
        } else {
            lowerb = Mat::from_slice(&[120, 150, 150]);
            upperb = Mat::from_slice(&[140, 255, 255]);
        }
    } else {
        panic!("Invalid filter_color selected: {filter_color}");
    }

    debug!("applying upper and lower vals in filter function: {upperb:?} {lowerb:?}");

    let mut hsv_frame = Mat::default();
    imgproc::cvt_color(
        frame,
        &mut hsv_frame,
        imgproc::COLOR_BGR2HSV,
        0,
        get_default_algorithm_hint().unwrap(),
    )
    .unwrap();

    let mut mask = Mat::default();
    core::in_range(&hsv_frame, &lowerb.unwrap(), &upperb.unwrap(), &mut mask).unwrap();

    let mut mask_color = Mat::default();
    imgproc::cvt_color(
        &mask,
        &mut mask_color,
        imgproc::COLOR_GRAY2BGR,
        0,
        get_default_algorithm_hint().unwrap(),
    )
    .unwrap();

    core::add_weighted(&frame.clone(), 0.1, &mask_color, 0.9, 0.0, &mut frame, -1).unwrap();
}

// this isn't an public function
#[allow(clippy::too_many_arguments)]
fn scan_area_cycle(
    manager: &Arc<Mutex<ManagerData>>,
    config: &Config,
    cam: Option<&Arc<Mutex<VideoCapture>>>,
    scan_data: &mut ScanData,
    led_pos: &mut PosEntry,
    i: u32,
    second_cam: bool,
    window: &str,
) -> Result<(i32, i32), Box<dyn Error>> {
    let capture_frames = config.advanced.camera.capture_frames.unwrap_or(3);

    let mut success = 0;
    let mut failures = 0;

    let x_start;
    let x_end;
    let y_start;
    let y_end;

    if !second_cam {
        x_start = scan_data.pos.x1_start;
        x_end = scan_data.pos.x1_end;
        y_start = scan_data.pos.y1_start;
        y_end = scan_data.pos.y1_end;
    } else {
        x_start = scan_data.pos.x2_start.unwrap();
        x_end = scan_data.pos.x2_end.unwrap();
        y_start = scan_data.pos.y2_start.unwrap();
        y_end = scan_data.pos.y2_end.unwrap();
    }

    let filter_color = manager.lock().unwrap().config.filter_color.unwrap();
    let scan_mode = manager.lock().unwrap().config.scan_mode;

    let brightness = config.scan.color_bright.unwrap();

    if scan_mode == 1 {
        debug!("using color filter");
        if filter_color == 0 {
            debug!(
                "Using red color filter with filter range: {:?}",
                manager.lock().unwrap().config.hsv_red_override
            );
            led_manager::set_color(manager, i.try_into().unwrap(), brightness, 0, 0);
        } else if filter_color == 1 {
            debug!(
                "Using green color filter with filter range: {:?}",
                manager.lock().unwrap().config.hsv_green_override
            );
            led_manager::set_color(manager, i.try_into().unwrap(), 0, brightness, 0);
        } else if filter_color == 2 {
            debug!(
                "Using blue color filter with filter range: {:?}",
                manager.lock().unwrap().config.hsv_blue_override
            );
            led_manager::set_color(manager, i.try_into().unwrap(), 0, 0, brightness);
        }
    } else {
        led_manager::set_color(
            manager,
            i.try_into().unwrap(),
            brightness,
            brightness,
            brightness,
        );
    }

    let mut frame = Mat::default();
    {
        let mut cam = cam.unwrap().lock().unwrap();

        // if config.camera.video_width.is_some() && config.camera.video_height.is_some() {
        //     cam.set(CAP_PROP_FRAME_WIDTH, config.camera.video_width.unwrap())?;
        //     cam.set(CAP_PROP_FRAME_HEIGHT, config.camera.video_height.unwrap())?;
        // }

        for _ in 0..300 {
            // Mostly only needed when using RTSP
            if frame.size()?.width == 0 || frame.size()?.height == 0 {
                debug!("capturing another frame as previous was empty");
                cam.read(&mut frame)?;
            } else {
                break;
            }
        }

        for _ in 0..capture_frames {
            // This is still needed unfortunately. It may need to be increased if you continue to encounter issues
            cam.read(&mut frame)?;
        }
    }

    let mut frame = match Mat::roi(
        &frame,
        opencv::core::Rect {
            x: x_start,
            y: y_start,
            width: x_end - x_start,
            height: y_end - y_start,
        },
    ) {
        Ok(frame) => frame,
        Err(e) => panic!(
            "Tried to crop frame with x: {x_start} y: {y_start} width: {} height: {} while frame dimensions are width: {} height: {}\ngot error: {e}",
            x_end - x_start,
            y_end - y_start,
            frame.size()?.width,
            frame.size()?.height
        )
    }
    .try_clone()?;

    if scan_data.invert {
        flip(&frame.clone(), &mut frame, 1).unwrap();
    }

    if scan_mode == 1 {
        debug!("applying filter");
        filter(&mut frame, &filter_color, manager);
    }

    let (_, max_val, pos) = get_brightest_cam_1_pos(frame.try_clone()?);

    led_pos[i as usize] = {
        let status_msg = {
            if max_val
                >= scan_data.pos.cam_1_darkest.unwrap()
                    + ((scan_data.pos.cam_1_brightest.unwrap()
                        - scan_data.pos.cam_1_darkest.unwrap())
                        * 0.5)
            {
                debug!("Succesful xy calibration: {pos:?} on index: {i}");
                success += 1;
                imgproc::circle(
                    &mut frame,
                    pos,
                    20,
                    Scalar::new(0.0, 255.0, 0.0, 255.0),
                    2,
                    LINE_8,
                    0,
                )?;

                if scan_data.depth || second_cam {
                    "SUCCESS-Z"
                } else {
                    "SUCCESS-XY"
                }
            } else {
                debug!("Failed xy calibration: {pos:?} on index: {i}");
                failures += 1;
                imgproc::circle(
                    &mut frame,
                    pos,
                    20,
                    Scalar::new(0.0, 0.0, 255.0, 255.0),
                    2,
                    LINE_8,
                    0,
                )?;

                if scan_data.depth {
                    "RECALIBRATE-Z"
                } else {
                    "RECALIBRATE-XY"
                }
            }
        };

        if scan_data.depth || second_cam {
            if config.advanced.camera.cam2_overhead.unwrap_or(false) {
                if config.advanced.camera.cam2_overhead_flip.unwrap_or(false) {
                    (
                        status_msg.to_string(),
                        led_pos[i as usize].1,
                        (pos.y, pos.x),
                    )
                } else {
                    (
                        status_msg.to_string(),
                        led_pos[i as usize].1,
                        (y_start + y_end - pos.y, pos.x),
                    )
                }
            } else {
                (
                    status_msg.to_string(),
                    led_pos[i as usize].1,
                    (pos.x, pos.y),
                )
            }
        } else {
            (
                status_msg.to_string(),
                (pos.x, pos.y),
                led_pos[i as usize].2,
            )
        }
    };

    // Update frame_cam_x after all our modifications
    if second_cam {
        manager.lock().unwrap().vision.frame_cam_2 = frame.clone();
    } else {
        manager.lock().unwrap().vision.frame_cam_1 = frame.clone();
    }

    if let Some(no_video) = config.advanced.camera.no_video
        && !no_video
    {
        highgui::set_window_title(window, &("LED index: ".to_owned() + &i.to_string()))?;
        highgui::imshow(window, &frame)?;
    }

    highgui::wait_key(1)?;
    led_manager::set_color(manager, i.try_into().unwrap(), 0, 0, 0);
    Ok((success, failures))
}

pub fn failed_calibration(led_pos: PosEntry) -> String {
    let json = serde_json::to_string_pretty(&led_pos).expect("Unable to serialize metadata!");

    let date = Local::now();
    let path = format!("temp-pos-{}", date.format("%Y-%m-%d-%H:%M:%S"));
    let mut file = File::create(Path::new(&path))
        .unwrap_or_else(|_| panic!("Unable to write temp-pos to {path}, temp-pos: {led_pos:?}"));
    file.write_all(json.as_bytes())
        .unwrap_or_else(|_| panic!("Unable to write temp-pos file at {path}"));
    path
}

pub fn wait(
    scan_data_lock: Arc<Mutex<ScanData>>,
    cam: &Arc<Mutex<videoio::VideoCapture>>,
    window: &str,
) -> Result<(), Box<dyn Error>> {
    let scan_data = scan_data_lock.lock().unwrap();
    loop {
        let mut frame = Mat::default();
        {
            let mut cam = cam.lock().unwrap();
            cam.read(&mut frame)?;
        }

        let cropped_image = Mat::roi(
            &frame,
            opencv::core::Rect {
                x: scan_data.pos.x1_start,
                y: scan_data.pos.y1_start,
                width: scan_data.pos.x1_end - scan_data.pos.x1_start,
                height: scan_data.pos.y1_end - scan_data.pos.y1_start,
            },
        )
        .unwrap();

        if frame.size()?.width > 0 {
            highgui::imshow(window, &cropped_image)?;
        }

        let key = highgui::wait_key(10)?;
        if key == 32 && key != 255 {
            break;
        }
    }
    Ok(())
}

pub fn manual_calibrate(
    manager_guard: &Arc<Mutex<ManagerData>>,
    config: &Config,
    window: &str,
    cam: &Arc<Mutex<videoio::VideoCapture>>,
    led_pos: &mut PosEntry,
    scan_data_guard: &Arc<Mutex<ScanData>>,
) -> Result<()> {
    let scan_data = scan_data_guard.lock().unwrap();

    debug!("scan_data: {scan_data:?}");
    info!("You are entering manual calibration mode. \n
    This is to manually calibrate all LEDs that failed to properly calibrate, and to make sure all LEDs did calibrate properly. The controls are:\n
    R: Move to the next LED
    E: Move to the previous LED
    F: Move to the next uncalibrated LED
    Left Click: Select the illuminated LED.
    Q: Exit calibration and move on.
    
    Any LEDs that are thought to need recalibration will be circled red, and the LEDs that are thought to be accurate will be blue.");

    let x_click = Arc::new(Mutex::new(0));
    let y_click = Arc::new(Mutex::new(0));
    let callback_called = Arc::new(Mutex::new(false));

    let x_click_guard = Arc::clone(&x_click);
    let y_click_guard = Arc::clone(&y_click);
    let callback_called_guard = Arc::clone(&callback_called);

    highgui::set_mouse_callback(
        window,
        Some(Box::new(move |event, x, y, _flag| {
            if event == EVENT_LBUTTONDOWN {
                *callback_called_guard.lock().unwrap() = true;
                *x_click_guard.lock().unwrap() = x;
                *y_click_guard.lock().unwrap() = y;
            }
        })),
    )?;

    let mut led_index: usize = 0;
    'video: loop {
        debug!("led_index: {led_index}");
        highgui::set_window_title(
            window,
            &format!("R for next, E for previous, Q to finish. On LED {led_index}",),
        )
        .unwrap();
        led_manager::set_color(manager_guard, led_index as u16, 255, 255, 255);
        let mut frame = Mat::default();
        {
            let mut cam = cam.lock().unwrap();
            for _ in 0..3 {
                cam.read(&mut frame)?;
            }
        }

        let mut frame = Mat::roi(
            &frame,
            opencv::core::Rect {
                x: scan_data.pos.x1_start,
                y: scan_data.pos.y1_start,
                width: scan_data.pos.x1_end - scan_data.pos.x1_start,
                height: scan_data.pos.y1_end - scan_data.pos.y1_start,
            },
        )
        .unwrap()
        .try_clone()?;
        if scan_data.invert {
            debug!("inverting display");
            flip(&frame.clone(), &mut frame, 1).unwrap();
        }

        let mut color = Scalar::new(0.0, 0.0, 255.0, 255.0);
        if !led_pos[led_index].0.contains("RECALIBRATE") {
            color = Scalar::new(0.0, 255.0, 0.0, 255.0);
        }
        let pos;
        if scan_data.depth {
            if !*callback_called.lock().unwrap() {
                debug!("pos is from depth, callback uncalled.");
                pos = match led_pos[led_index].2 {
                    (i32::MIN, i32::MIN) => {
                        error!("led_pos does not contain any depth data, setting to 0, 0!");
                        Point::new(0, 0)
                    }
                    pos => Point::new(pos.0, pos.1),
                };
            } else {
                debug!("pos is from callback");
                led_pos[led_index].0 = "MANUAL-Z".to_string();
                led_pos[led_index].2 = {
                    if config.advanced.camera.cam2_overhead.unwrap_or(false) {
                        if config.advanced.camera.cam2_overhead_flip.unwrap_or(false) {
                            (*y_click.lock().unwrap(), *x_click.lock().unwrap())
                        } else {
                            let (y_start, y_end) = {
                                if config.camera.multi_camera {
                                    // We always get depth from second camera if multi-camera
                                    (
                                        scan_data.pos.y2_start.unwrap(),
                                        scan_data.pos.y2_end.unwrap(),
                                    )
                                } else {
                                    (scan_data.pos.y1_start, scan_data.pos.y1_end)
                                }
                            };
                            (
                                y_start + y_end - *y_click.lock().unwrap(),
                                *x_click.lock().unwrap(),
                            )
                        }
                    } else {
                        (*x_click.lock().unwrap(), *y_click.lock().unwrap())
                    }
                };
                pos = Point::new(*x_click.lock().unwrap(), *y_click.lock().unwrap());
                color = Scalar::new(0.0, 255.0, 0.0, 255.0);
                *callback_called.lock().unwrap() = false;
            }
        } else if !*callback_called.lock().unwrap() {
            debug!(
                "pos not from depth, from led_pos[led_index].1 which is {:?}",
                led_pos[led_index].1
            );
            pos = Point::new(led_pos[led_index].1.0, led_pos[led_index].1.1)
        } else {
            debug!("pos not from depth, from callback");
            led_pos[led_index].0 = "MANUAL-XY".to_string();
            led_pos[led_index].1 = (*x_click.lock().unwrap(), *y_click.lock().unwrap());
            pos = Point::new(*x_click.lock().unwrap(), *y_click.lock().unwrap());
            color = Scalar::new(0.0, 255.0, 0.0, 255.0);
            *callback_called.lock().unwrap() = false;
        }
        debug!("setting cricle at {pos:?}");
        imgproc::circle(&mut frame, pos, 20, color, 2, LINE_8, 0)?;

        if let Some(no_video) = config.advanced.camera.no_video
            && frame.size()?.width > 0
            && !no_video
        {
            highgui::imshow(window, &frame)?;
        }

        loop {
            if *callback_called.lock().unwrap() {
                debug!("Breaking key loop on detected callback!");
                break;
            }
            let key = highgui::wait_key(10)?;
            if key == 114 {
                debug!("got R");
                if led_index + 1 < config.num_led.try_into().unwrap() {
                    led_manager::set_color(manager_guard, led_index.try_into().unwrap(), 0, 0, 0);
                    led_index += 1;
                } else {
                    warn!("At end of LEDs!");
                }
                break;
            } else if key == 101 {
                debug!("got E");
                led_manager::set_color(manager_guard, led_index.try_into().unwrap(), 0, 0, 0);
                if led_index - 1 > 0 {
                    led_index -= 1;
                } else {
                    warn!("At first LED!");
                }
                break;
            } else if key == 102 {
                debug!("got F");
                led_manager::set_color(manager_guard, led_index.try_into().unwrap(), 0, 0, 0);
                let led_begin = led_index; // Needed because of clippy::mut_range_bound
                for _ in led_begin..config.num_led.try_into().unwrap() {
                    led_index += 1;
                    if led_pos[led_index].0.contains("RECALIBRATE") {
                        break;
                    }
                }
                break;
            } else if key == 113 {
                break 'video;
            }
        }
    }
    Ok(())
}

pub fn post_process(led_pos: &mut PosEntry, led_count: u32) {
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;
    let mut z_max = i32::MIN;

    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut z_min = i32::MAX;

    for i in 0..led_count {
        // Get max and min values in led_pos
        x_max = max(led_pos[i as usize].1.0, x_max);
        x_min = min(led_pos[i as usize].1.0, x_min);

        y_max = max(led_pos[i as usize].1.1, y_max);
        y_min = min(led_pos[i as usize].1.1, y_min);

        z_min = min(led_pos[i as usize].2.0, z_min);
        z_max = max(led_pos[i as usize].2.0, z_max);
    }

    for i in 0..led_count {
        // Normalize values
        led_pos[i as usize].1.0 -= x_min;
        led_pos[i as usize].1.1 -= y_min;
        led_pos[i as usize].2.0 -= z_min;
    }

    for i in 0..led_count {
        let y_mid = y_max / 2;
        let current_y = led_pos[i as usize].1.1;

        let z_mid = z_max / 2;
        let current_z = led_pos[i as usize].2.0;

        led_pos[i as usize].1.1 = match current_y {
            y if y > y_mid => y_mid - (y - y_mid),
            y if y < y_mid => y_mid + (y_mid - y),
            _ => led_pos[i as usize].1.1, // when current_y == y_mid, no change
        };

        if current_z > z_mid {
            led_pos[i as usize].2.0 = z_mid - (current_z - z_mid);
        } else if current_z < z_mid {
            led_pos[i as usize].2.0 = z_mid + (z_mid - current_z);
        }
    }
}

pub fn position_adjustment(led_pos: &mut PosEntry, config: &Config) {
    // Get mins and maxs
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;
    let mut z_max = i32::MIN;

    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut z_min = i32::MAX;

    for position in led_pos.iter() {
        x_max = max(position.1.0, x_max);
        x_min = min(position.1.0, x_min);

        y_max = max(position.1.1, y_max);
        y_min = min(position.1.1, y_min);

        z_min = min(position.2.0, z_min);
        z_max = max(position.2.0, z_max);
    }

    // Apply stretch/shrink

    let x_adjust = config.advanced.transform.x_stretch_adjust;
    let y_adjust = config.advanced.transform.y_stretch_adjust;
    let z_adjust = config.advanced.transform.z_stretch_adjust;

    if x_adjust.is_some() || y_adjust.is_some() || z_adjust.is_some() {
        for position in led_pos.iter_mut() {
            for (coordinate, adjust_opt, center) in [
                (&mut position.1.0, x_adjust, x_max / 2),
                (&mut position.1.1, y_adjust, y_max / 2),
                (&mut position.2.0, z_adjust, z_max / 2),
            ] {
                if let Some(adjust_amount) = adjust_opt {
                    let dist_from_center = (*coordinate - center).abs();

                    let stretch_factor = ((dist_from_center as f32 / (x_max as f32 / 2.0))
                        * adjust_amount as f32)
                        .round() as i32;

                    if *coordinate > center {
                        *coordinate += stretch_factor;
                    } else if *coordinate < center {
                        *coordinate -= stretch_factor;
                    }
                }
            }
        }
    }

    // Apply perspective distortion adjustment

    let x_transform = config.advanced.transform.x_perspect_distort_adjust;
    let y_transform = config.advanced.transform.y_perspect_distort_adjust;
    let z_transform = config.advanced.transform.z_perspect_distort_adjust;

    if x_transform.is_some() || y_transform.is_some() || z_transform.is_some() {
        for position in led_pos.iter_mut() {
            if let Some(z_transform_amount) = z_transform {
                // Calculates reduction based on distance from front
                let mut reduction_factor = ((position.2.0 as f32 / z_max as f32)
                    * z_transform_amount as f32)
                    .round() as i32;

                // Reduces reduction based on distance from center
                let x_mid = (x_max as f32 / 2.0).round() as i32;
                reduction_factor *= (position.1.0 - x_mid).abs() / x_mid;

                if position.1.0 > x_max / 2 {
                    position.1.0 = (position.1.0 - reduction_factor).max(x_max / 2);
                } else if position.1.0 < x_max / 2 {
                    position.1.0 = (position.1.0 + reduction_factor).min(x_max / 2);
                }
            }

            // TODO: Implement for X and Y aswell
            // if let Some(y_transform_amount) = y_transform {
            //     let reduction_factor = ((position.1 .1 as f32 / z_max as f32)
            //         * y_transform_amount as f32)
            //         .round() as i32;

            //     if position.2 .0 > x_max / 2 {
            //         position.2 .0 -= reduction_factor;
            //     } else if position.2 .0 < x_max / 2 {
            //         position.2 .0 += reduction_factor;
            //     }
            // }

            // if let Some(x_transform_amount) = x_transform {
            //     let reduction_factor = ((position.1 .0 as f32 / z_max as f32)
            //         * x_transform_amount as f32)
            //         .round() as i32;

            //     if position.1 .1 > x_max / 2 {
            //         position.1 .1 -= reduction_factor;
            //     } else if position.1 .1 < x_max / 2 {
            //         position.1 .1 += reduction_factor;
            //     }
            // }
        }
    }
}

pub fn get_cam(config: &Config, index_or_address: &str) -> Result<VideoCapture, Box<dyn Error>> {
    let mut cam = match index_or_address.parse::<i32>() {
        Ok(index) => videoio::VideoCapture::new(index, videoio::CAP_ANY)
            .unwrap_or_else(|_| panic!("Could not open camera on index {index}")),
        Err(_) => {
            let mut cam = videoio::VideoCapture::default()?;
            cam.open_file(index_or_address, videoio::CAP_ANY)
                .unwrap_or_else(|_| panic!("Could not open RTSP at {index_or_address}"));
            cam
        }
    };

    if let Some(video_width) = config.camera.video_width
        && let Some(video_height) = config.camera.video_height
    {
        cam.set(CAP_PROP_FRAME_WIDTH, video_width)?;
        cam.set(CAP_PROP_FRAME_HEIGHT, video_height)?;
    }

    Ok(cam)
}
