//! Run with `cargo run --release --example roughness_scan -- 60 67`.
use adaptive_ji::tuning::{
    OptimizerConfig,
    candidates::CandidateGenerator,
    rational::harmonic_complexity,
    roughness::{RoughnessConfig, SpectrumModel, fundamental_frequency, roughness},
};

fn midi_arg(value: &str) -> u8 {
    let note: u8 = value.parse().expect("MIDI note must be an integer 0..127");
    assert!(note <= 127, "MIDI note must be in 0..127");
    note
}

fn main() {
    let mut args = std::env::args().skip(1);
    let first = midi_arg(&args.next().expect("usage: roughness_scan MIDI_A MIDI_B"));
    let second = midi_arg(&args.next().expect("usage: roughness_scan MIDI_A MIDI_B"));
    assert!(args.next().is_none(), "usage: roughness_scan MIDI_A MIDI_B");

    let config = OptimizerConfig {
        candidates_per_note: 32,
        ..OptimizerConfig::default()
    };
    let model = SpectrumModel::new(RoughnessConfig::default()).unwrap();
    let reference = model.spectrum(fundamental_frequency(first, 0.0)).unwrap();
    let mut generator = CandidateGenerator::new(config).unwrap();
    let candidates = generator.for_interval(i16::from(second) - i16::from(first));
    println!(
        "MIDI {first} -> {second}, {} candidates within ±{} cents; partials=12 rolloff=1.0, unit-energy amplitudes",
        candidates.candidates.len(),
        config.max_deviation_cents
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
