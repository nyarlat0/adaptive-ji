//! Scheduler/interpolation diagnostic; no ALSA and no sleeping.
use std::time::{Duration, Instant};

use adaptive_ji::glide::{GlideConfig, GlideEngine};

fn main() {
    let iterations = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(10_000);
    let mut engine = GlideEngine::new(GlideConfig::default(), 2.0);
    for id in 1..=15 {
        engine.set_target(id, 0.0, Duration::ZERO);
    }
    let started = Instant::now();
    let mut updates = 0;
    for iteration in 0..iterations {
        let origin = Duration::from_millis(iteration * 40);
        for id in 1..=15 {
            engine.set_target(id, if iteration % 2 == 0 { 20.0 } else { -20.0 }, origin);
        }
        for step in 1..=7 {
            updates += engine.tick(origin + Duration::from_millis(step * 5)).len();
        }
    }
    let elapsed = started.elapsed();
    println!(
        "15 gliding voices: {iterations} retargets + {} ticks, {updates} bend updates, {:.3} us/tick (including retarget work)",
        iterations * 7,
        elapsed.as_secs_f64() * 1e6 / (iterations * 7) as f64
    );
}
