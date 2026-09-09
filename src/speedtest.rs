use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use log::{debug, info, warn};
use rand::Rng;

use crate::{ManagerData, led_manager};

pub fn speedtest(manager: &Arc<Mutex<ManagerData>>, num_led: u32, writes: u32) {
    if writes == 0 {
        warn!("speedtest_writes is zero; there is nothing to benchmark");
        return;
    }

    let mut rng = rand::rng();
    info!("Clearing string");

    for n in 0..num_led {
        led_manager::set_color(manager, n as u16, 0, 0, 0);
    }

    info!("Testing {writes} random writes");
    let start = Instant::now();

    for _ in 0..writes {
        led_manager::set_color(
            manager,
            rng.random_range(0..num_led) as u16,
            rng.random_range(0..=255),
            rng.random_range(0..=255),
            rng.random_range(0..=255),
        );
    }

    let queue_lengths = manager
        .lock()
        .unwrap()
        .state
        .led_state
        .queue_lengths
        .clone();

    let end = start.elapsed();

    let queue_total_lengths: u32 = queue_lengths.iter().map(|length| u32::from(*length)).sum();

    info!("{end:.2?} seconds.");
    info!("{:.5?} seconds per LED", end / writes);
    info!("{:.3} LEDs per second", writes as f64 / end.as_secs_f64());

    if !queue_lengths.is_empty() {
        info!(
            "Average queue length: {}",
            queue_total_lengths / queue_lengths.len() as u32
        );
    } else {
        debug!(
            "queue_lengths.len() is 0, check debug logs for average queue lengths from threads."
        );
    }
}
