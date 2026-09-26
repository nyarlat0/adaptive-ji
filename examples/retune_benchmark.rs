//! Run with `cargo run --release --example retune_benchmark -- 100`.
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use adaptive_ji::tuning::{
    ActiveNote, AdaptiveRationalOptimizer, OptimizerConfig, PrimeLimit, TuningStrategy,
    candidates::CandidateGenerator,
    profile::{ProfilePartial, SpectrumPreset, SpectrumProfile},
};

fn measure(label: &str, count: usize, mut retune: impl FnMut() -> Duration) {
    let mut times: Vec<_> = (0..count).map(|_| retune()).collect();
    times.sort_unstable();
    let mean_ms = times.iter().map(Duration::as_secs_f64).sum::<f64>() * 1000.0 / count as f64;
    println!(
        "{label}: mean={mean_ms:.3} ms median={:.3} ms p95={:.3} ms max={:.3} ms (n={count})",
        times[count / 2].as_secs_f64() * 1000.0,
        times[(count * 95 / 100).min(count - 1)].as_secs_f64() * 1000.0,
        times[count - 1].as_secs_f64() * 1000.0
    );
}

fn compare(
    label: &str,
    count: usize,
    notes: &[ActiveNote],
    config: OptimizerConfig,
    profile: SpectrumProfile,
) {
    println!(
        "{label}: partials={}, anchor={:?}, w_roughness={}",
        profile.partials().len(),
        config.anchor_mode,
        config.weights.roughness
    );
    measure("  Fresh chord (cold candidates)", count, || {
        let mut optimizer =
            AdaptiveRationalOptimizer::with_profile(config, profile.clone()).unwrap();
        let start = Instant::now();
        black_box(optimizer.tune(black_box(notes)));
        start.elapsed()
    });
    let mut optimizer = AdaptiveRationalOptimizer::with_profile(config, profile).unwrap();
    optimizer.tune(notes);
    measure("  Held chord (warm candidates)", count, || {
        let start = Instant::now();
        black_box(optimizer.tune(black_box(notes)));
        start.elapsed()
    });
    let mut add = false;
    let mut next_id = 100_u64;
    measure("  Alternating 14/15 voices", count, || {
        add = !add;
        let mut active = notes[..14].to_vec();
        if add {
            let mut new = notes[14];
            new.id = next_id;
            next_id += 1;
            active.push(new);
        }
        let start = Instant::now();
        black_box(optimizer.tune(black_box(&active)));
        start.elapsed()
    });
}

fn main() {
    let count = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>().expect("iteration count"))
        .unwrap_or(100)
        .max(1);
    let notes: Vec<_> = [48, 49, 51, 52, 54, 55, 57, 58, 60, 61, 63, 64, 66, 67, 70]
        .iter()
        .enumerate()
        .map(|(id, &input_note)| ActiveNote {
            id: id as u64,
            input_note,
            current_offset_cents: None,
        })
        .collect();
    for prime_limit in [
        PrimeLimit::new(3).unwrap(),
        PrimeLimit::new(5).unwrap(),
        PrimeLimit::new(7).unwrap(),
        PrimeLimit::new(11).unwrap(),
        PrimeLimit::new(13).unwrap(),
        PrimeLimit::new(17).unwrap(),
        PrimeLimit::new(23).unwrap(),
        PrimeLimit::new(31).unwrap(),
        PrimeLimit::new(101).unwrap(),
    ] {
        let started = Instant::now();
        let mut generator = CandidateGenerator::new(OptimizerConfig {
            prime_limit,
            ..OptimizerConfig::default()
        })
        .unwrap();
        let mut count = 0;
        for interval in -127..=127 {
            count += generator.for_interval(interval).candidates.len();
        }
        println!(
            "candidate generation prime-limit={} raw={} : {:.3} ms ({} candidates)",
            prime_limit.max_prime(),
            prime_limit.effective_primes(12.0).len(),
            started.elapsed().as_secs_f64() * 1000.0,
            count
        );
    }
    println!("15 voices; library K=8, beam=2048; construction/logging/MIDI excluded");
    let config = OptimizerConfig::default();
    for (name, preset) in [
        ("sine", SpectrumPreset::Sine),
        ("harmonic default", SpectrumPreset::Harmonic),
        ("bright", SpectrumPreset::Bright),
    ] {
        compare(
            name,
            count,
            &notes,
            config,
            SpectrumProfile::builtin(preset, config.roughness).unwrap(),
        );
    }
    let custom = SpectrumProfile::new(
        (1..=32)
            .map(|n| ProfilePartial {
                frequency_ratio: n as f64 * (1.0 + 0.0001 * (n - 1) as f64),
                amplitude: 1.0 / n as f64,
            })
            .collect(),
    )
    .unwrap();
    compare("custom 32-partial", count, &notes, config, custom);

    let cli_config = OptimizerConfig {
        prime_limit: PrimeLimit::new(11).unwrap(),
        max_deviation_cents: 50.0,
        candidates_per_note: 8,
        ..OptimizerConfig::default()
    };
    println!("v1.1 CLI defaults: 11-limit, 50c, K=8, memory=relations");
    compare(
        "v1.1 harmonic",
        count,
        &notes,
        cli_config,
        SpectrumProfile::builtin(SpectrumPreset::Harmonic, cli_config.roughness).unwrap(),
    );
}
