//! Inspect normalized profile partials without MIDI.
use std::path::Path;

use adaptive_ji::tuning::{
    profile::{SpectrumPreset, SpectrumProfile},
    roughness::RoughnessConfig,
};
use clap::ValueEnum;

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .expect("usage: spectrum_dump PRESET | --spectrum-file PATH");
    let profile = if first == "--spectrum-file" {
        adaptive_ji::files::load_spectrum(Path::new(&args.next().expect("missing path")))
            .expect("invalid spectrum file")
    } else {
        let preset = SpectrumPreset::from_str(&first, true).expect("unknown spectrum preset");
        SpectrumProfile::builtin(preset, RoughnessConfig::default()).unwrap()
    };
    assert!(args.next().is_none(), "unexpected extra argument");
    println!("ratio       amplitude");
    for partial in profile.partials() {
        println!(
            "{:10.5}  {:10.7}",
            partial.frequency_ratio, partial.amplitude
        );
    }
}
