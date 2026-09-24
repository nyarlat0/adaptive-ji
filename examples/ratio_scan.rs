//! Inspect candidate ratios, e.g. `ratio_scan -- 60 66 --prime-limit 11 --max-deviation 60`.
use std::path::Path;

use adaptive_ji::tuning::{
    OptimizerConfig, PrimeLimit,
    candidates::CandidateGenerator,
    profile::{SpectrumPreset, SpectrumProfile},
    rational::harmonic_complexity,
    roughness::{SpectrumModel, fundamental_frequency, roughness},
};
use clap::ValueEnum;

fn note(value: &str) -> u8 {
    value.parse::<u8>().expect("MIDI note must be 0..127")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = note(
        &args
            .next()
            .expect("usage: ratio_scan MIDI_A MIDI_B [options]"),
    );
    let second = note(
        &args
            .next()
            .expect("usage: ratio_scan MIDI_A MIDI_B [options]"),
    );
    let mut prime_limit = PrimeLimit::new(7).unwrap();
    let mut max_deviation = 35.0;
    let mut preset = SpectrumPreset::Harmonic;
    let mut spectrum_file = None;
    while let Some(option) = args.next() {
        let value = args.next().expect("option requires a value");
        match option.as_str() {
            "--prime-limit" => prime_limit = value.parse().expect("prime limit must be prime"),
            "--max-deviation" => max_deviation = value.parse().expect("max deviation cents"),
            "--spectrum" => {
                preset = SpectrumPreset::from_str(&value, true).expect("unknown spectrum")
            }
            "--spectrum-file" => spectrum_file = Some(value),
            _ => panic!("unknown option {option}"),
        }
    }
    let config = OptimizerConfig {
        prime_limit,
        max_deviation_cents: max_deviation,
        candidates_per_note: 32,
        ..OptimizerConfig::default()
    };
    let profile = spectrum_file.map_or_else(
        || SpectrumProfile::builtin(preset, config.roughness).unwrap(),
        |path| SpectrumProfile::from_toml_file(Path::new(&path)).unwrap(),
    );
    let model = SpectrumModel::from_profile(profile);
    let mut generator = CandidateGenerator::new(config).unwrap();
    let candidates = generator
        .for_interval(i16::from(second) - i16::from(first))
        .clone();
    let reference = model.spectrum(fundamental_frequency(first, 0.0)).unwrap();
    println!(
        "MIDI {first}->{second}, prime-limit={}, max-deviation={}c",
        prime_limit.max_prime(),
        max_deviation
    );
    println!(
        "effective primes: {:?}, visited search nodes: {}, generated candidates: {}",
        generator.effective_primes(),
        generator.visited_search_nodes,
        candidates.generated_count
    );
    println!("ratio       factors             cents       offset      complexity  roughness");
    for candidate in &candidates.candidates {
        let spectrum = model
            .spectrum(fundamental_frequency(second, candidate.offset_cents))
            .unwrap();
        println!(
            "{:<11} {:<19?} {:+10.3} {:+10.3} {:11.4} {:10.6}",
            candidate.ratio,
            adaptive_ji::tuning::rational::prime_factorization_without_two(candidate.ratio),
            candidate.ratio.cents(),
            candidate.offset_cents,
            harmonic_complexity(candidate.ratio),
            roughness(&reference, &spectrum)
        );
    }
}
