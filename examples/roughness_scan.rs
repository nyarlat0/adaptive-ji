//! Run with `cargo run --release --example roughness_scan -- 60 67 --spectrum bright`.
use std::path::Path;

use adaptive_ji::tuning::{
    OptimizerConfig,
    candidates::CandidateGenerator,
    profile::{SpectrumPreset, SpectrumProfile},
    rational::harmonic_complexity,
    roughness::{SpectrumModel, fundamental_frequency, roughness},
};
use clap::ValueEnum;

fn midi_arg(value: &str) -> u8 {
    let note: u8 = value.parse().expect("MIDI note must be an integer 0..127");
    assert!(note <= 127, "MIDI note must be in 0..127");
    note
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first =
        midi_arg(&args.next().expect(
            "usage: roughness_scan MIDI_A MIDI_B [--spectrum NAME | --spectrum-file PATH]",
        ));
    let second =
        midi_arg(&args.next().expect(
            "usage: roughness_scan MIDI_A MIDI_B [--spectrum NAME | --spectrum-file PATH]",
        ));
    let mut preset = SpectrumPreset::Harmonic;
    let mut file = None;
    while let Some(option) = args.next() {
        let value = args.next().expect("option requires a value");
        match option.as_str() {
            "--spectrum" => {
                preset = SpectrumPreset::from_str(&value, true).expect("unknown spectrum preset")
            }
            "--spectrum-file" => file = Some(value),
            _ => panic!("unknown option {option}"),
        }
    }

    let config = OptimizerConfig {
        candidates_per_note: 32,
        ..OptimizerConfig::default()
    };
    let profile = if let Some(path) = &file {
        adaptive_ji::files::load_spectrum(Path::new(path)).expect("invalid spectrum file")
    } else {
        SpectrumProfile::builtin(preset, config.roughness).unwrap()
    };
    let partial_count = profile.partials().len();
    let profile_label =
        file.unwrap_or_else(|| preset.to_possible_value().unwrap().get_name().to_owned());
    let model = SpectrumModel::from_profile(profile);
    let reference = model.spectrum(fundamental_frequency(first, 0.0)).unwrap();
    let mut generator = CandidateGenerator::new(config).unwrap();
    let candidates = generator.for_interval(i16::from(second) - i16::from(first));
    println!(
        "MIDI {first} -> {second}, {} candidates within ±{} cents; spectrum={} partials={partial_count}, unit-energy amplitudes",
        candidates.candidates.len(),
        config.max_deviation_cents,
        profile_label
    );
    println!("ratio       cents       offset      complexity  roughness");
    for candidate in &candidates.candidates {
        let spectrum = model
            .spectrum(fundamental_frequency(second, candidate.offset_cents))
            .unwrap();
        println!(
            "{:<11} {:+10.3} {:+10.3} {:11.4} {:10.6}",
            candidate.ratio,
            candidate.ratio.cents(),
            candidate.offset_cents,
            harmonic_complexity(candidate.ratio),
            roughness(&reference, &spectrum)
        );
    }
}
