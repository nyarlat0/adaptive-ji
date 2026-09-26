use std::path::PathBuf;

use adaptive_ji::{
    glide::GlideConfig,
    midi,
    pitch_bend::BendRange,
    tuning::{
        AdaptiveRationalOptimizer, FiveLimitLowestRoot, OptimizerConfig, PrimeLimit, ScoreWeights,
        TuningStrategy,
        adaptive::AnchorMode,
        profile::{SpectrumPreset, SpectrumProfile},
        roughness::RoughnessConfig,
        scala::{KeyboardMapping, ScalaScale, ScalaTuning, parse_root_note},
    },
};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Strategy {
    #[default]
    Adaptive,
    FiveLimit,
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Adaptive JI with optional Scala base tuning over Linux ALSA MIDI/MPE"
)]
struct Args {
    /// List available MIDI input ports (indices start at 0)
    #[arg(long)]
    list_inputs: bool,

    /// Input port index, exact name, or unique case-insensitive name fragment
    #[arg(long, required_unless_present_any = ["list_inputs", "scale_info"], conflicts_with_all = ["list_inputs", "scale_info"], value_parser = clap::builder::NonEmptyStringValueParser::new())]
    input: Option<String>,

    /// Import a Scala .scl as base pitches for the adaptive JI optimizer
    #[arg(long)]
    scale: Option<PathBuf>,

    /// Explicit Scala .kbm map; otherwise a same-stem sibling is loaded if present
    #[arg(long, requires = "scale", conflicts_with = "no_kbm")]
    kbm: Option<PathBuf>,

    /// Ignore an automatically discovered same-stem .kbm
    #[arg(long, requires = "scale")]
    no_kbm: bool,

    /// MIDI note mapped to Scala degree 0 without .kbm (default C4/60)
    #[arg(long, requires = "scale", value_parser = parse_root_note)]
    root_note: Option<u8>,

    /// Print the imported scale's MIDI mapping without opening ALSA
    #[arg(long, requires = "scale", conflicts_with = "list_inputs")]
    scale_info: bool,

    /// Per-note pitch bend range in semitones (0.01..=127.99, steps of 0.01)
    #[arg(long, default_value = "2")]
    bend_range: BendRange,

    /// Tuning strategy (five-limit preserves the v0.1 lowest-root baseline)
    #[arg(long, value_enum, default_value = "adaptive")]
    strategy: Strategy,

    /// Rational candidate prime limit (any prime 3..=997)
    #[arg(long, default_value = "11")]
    prime_limit: PrimeLimit,

    /// Modified Tenney complexity budget for bounded candidate search
    #[arg(long, default_value_t = 12.0)]
    max_complexity: f64,

    /// Candidate window around 12-TET or imported Scala baseline, in cents (0..=100)
    #[arg(long, default_value_t = 50.0)]
    max_deviation: f64,

    /// Maximum candidates per voice (1..=32)
    #[arg(long, default_value_t = 8)]
    candidates_per_note: usize,

    /// Maximum partial assignments retained by beam search (1..=16384)
    #[arg(long, default_value_t = 2048)]
    beam_width: usize,

    /// Absolute tuning anchor: floating optimizes it, fixed reproduces v0.3
    #[arg(long, value_enum, default_value = "floating")]
    anchor_mode: AnchorMode,

    /// Maximum absolute floating anchor in cents (0..=100)
    #[arg(long, default_value_t = 35.0)]
    anchor_max: f64,

    #[arg(long, default_value_t = 1.0)]
    w_harmonic: f64,
    #[arg(long, default_value_t = 0.35)]
    w_deviation: f64,
    #[arg(long, default_value_t = 1.5)]
    w_movement: f64,
    /// Weight of cross-note roughness (0 disables it; also use fixed anchor for v0.2)
    #[arg(long, default_value_t = 20.0)]
    w_roughness: f64,

    /// Experimental harmonic relation memory mode
    #[arg(long, value_enum, default_value = "relations")]
    memory_mode: adaptive_ji::tuning::MemoryMode,

    /// Experimental harmonic memory weight
    #[arg(long, default_value_t = 1.0)]
    w_memory: f64,

    /// Harmonic memory decay per retune event
    #[arg(long, default_value_t = 0.80)]
    memory_decay: f64,

    /// Clear experimental memory when all sounding voices disappear
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    memory_reset_on_empty: bool,

    /// Harmonic partials in the roughness model (1..=64)
    #[arg(long, default_value_t = 12)]
    roughness_partials: usize,

    /// Spectrum amplitude exponent (0.1..=4.0)
    #[arg(long, default_value_t = 1.0)]
    roughness_rolloff: f64,

    /// Built-in spectral shape used by sensory roughness
    #[arg(long, value_enum, default_value = "harmonic")]
    spectrum: SpectrumPreset,

    /// Custom TOML spectral profile; overrides --spectrum and roughness generator options
    #[arg(long)]
    spectrum_file: Option<PathBuf>,

    /// Retune glide duration for already sounding voices, in ms (0..=2000)
    #[arg(long, default_value_t = 35)]
    glide_ms: u64,

    /// Bend update interval during glide, in ms (1..=100)
    #[arg(long, default_value_t = 5)]
    glide_tick_ms: u64,

    /// Log notes, anchor and costs; -vv also logs base/actual offsets and ignored MIDI
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
        match args.verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        },
    ))
    .format_timestamp(None)
    .format_target(false)
    .init();

    if args.list_inputs {
        return midi::list_inputs();
    }
    let scale_mapping = if let Some(path) = &args.scale {
        if matches!(args.strategy, Strategy::FiveLimit) {
            anyhow::bail!(
                "--scale currently requires --strategy adaptive; the five-limit baseline has no Scala-relative search"
            );
        }
        let scale = ScalaScale::load(path).map_err(anyhow::Error::msg)?;
        let mapping_path = if let Some(explicit) = &args.kbm {
            Some(explicit.clone())
        } else if args.no_kbm || args.root_note.is_some() {
            None
        } else {
            let sibling = path.with_extension("kbm");
            sibling.exists().then_some(sibling)
        };
        if mapping_path.is_some() && args.root_note.is_some() {
            anyhow::bail!("--root-note applies only without .kbm; omit --kbm or use --no-kbm");
        }
        let mapping = mapping_path
            .as_deref()
            .map(KeyboardMapping::load)
            .transpose()
            .map_err(anyhow::Error::msg)?;
        let tuning = ScalaTuning::new(
            scale,
            mapping,
            args.root_note.unwrap_or(60),
            args.bend_range.semitones(),
        )
        .map_err(anyhow::Error::msg)?
        .with_adaptive_headroom(args.bend_range.semitones(), args.max_deviation)
        .map_err(anyhow::Error::msg)?;
        if tuning.unreachable_keys() != 0 {
            log::warn!(
                "{} Scala keys cannot be represented by MIDI note 0..127 plus ±{} semitone bend and will be silent; increase --bend-range to cover more",
                tuning.unreachable_keys(),
                args.bend_range.semitones()
            );
        }
        if args.scale_info {
            println!(
                "SCALE {}: {} degrees, period={:.3}c",
                tuning.scale.description,
                tuning.scale.size(),
                tuning.scale.period_cents()
            );
            println!(
                "KBM {}",
                mapping_path.as_ref().map_or_else(
                    || format!("default, root={}", args.root_note.unwrap_or(60)),
                    |path| path.display().to_string()
                )
            );
            println!(
                "Unreachable keys: {} (bend range ±{} semitones)",
                tuning.unreachable_keys(),
                args.bend_range.semitones()
            );
            for note in 0..=127u8 {
                if let (Some(output_note), Some(offset)) =
                    (tuning.output_note_for(note), tuning.offset(note))
                {
                    let base = tuning
                        .baseline_offset(note)
                        .expect("mapped key has baseline");
                    println!("{note:3} -> {output_note:3}  {offset:+8.3}c  base={base:+.3}c");
                }
            }
            return Ok(());
        }
        log::info!(
            "SCALE {} degrees={} kbm={}",
            tuning.scale.description,
            tuning.scale.size(),
            mapping_path
                .as_ref()
                .map_or_else(|| "default".into(), |path| path.display().to_string())
        );
        Some(tuning)
    } else {
        None
    };
    {
        let roughness_config = if args.spectrum_file.is_some() {
            // Generator options do not apply to a static custom spectrum.
            RoughnessConfig::default()
        } else {
            RoughnessConfig {
                partials: args.roughness_partials,
                rolloff: args.roughness_rolloff,
            }
        };
        let profile = if let Some(path) = &args.spectrum_file {
            SpectrumProfile::from_toml_file(path).map_err(anyhow::Error::msg)?
        } else {
            SpectrumProfile::builtin(args.spectrum, roughness_config).map_err(anyhow::Error::msg)?
        };
        if let Some(path) = &args.spectrum_file {
            log::info!(
                "SPECTRUM custom path={} partials={}",
                path.display(),
                profile.partials().len()
            );
        } else {
            log::info!(
                "SPECTRUM {} partials={}",
                format!("{:?}", args.spectrum).to_lowercase(),
                profile.partials().len()
            );
        }
        log::info!(
            "PRIME LIMIT {} effective_primes={} max_complexity={}",
            args.prime_limit.max_prime(),
            args.prime_limit.effective_primes(args.max_complexity).len(),
            args.max_complexity
        );
        let config = OptimizerConfig {
            prime_limit: args.prime_limit,
            max_deviation_cents: args.max_deviation,
            max_complexity: args.max_complexity,
            memory_mode: args.memory_mode,
            memory_decay: args.memory_decay,
            memory_reset_on_empty: args.memory_reset_on_empty,
            candidates_per_note: args.candidates_per_note,
            beam_width: args.beam_width,
            anchor_mode: args.anchor_mode,
            anchor_max_cents: args.anchor_max,
            weights: ScoreWeights {
                harmonic: args.w_harmonic,
                deviation: args.w_deviation,
                movement: args.w_movement,
                roughness: args.w_roughness,
                memory: args.w_memory,
            },
            roughness: roughness_config,
        };
        config.validate().map_err(anyhow::Error::msg)?;
        let tuning: Box<dyn TuningStrategy> = match args.strategy {
            Strategy::Adaptive => Box::new(
                AdaptiveRationalOptimizer::with_profile_and_scale(config, profile, scale_mapping)
                    .map_err(anyhow::Error::msg)?,
            ),
            Strategy::FiveLimit => Box::new(FiveLimitLowestRoot),
        };
        let glide = GlideConfig {
            duration_ms: args.glide_ms,
            tick_ms: args.glide_tick_ms,
        };
        glide.validate().map_err(anyhow::Error::msg)?;
        midi::run(
            args.input.as_deref().expect("clap requires --input"),
            args.bend_range,
            tuning,
            glide,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adaptive_ji::tuning::{ActiveNote, Ratio, candidates::CandidateGenerator};

    #[test]
    fn version_11_cli_defaults_enable_exploratory_adaptive_memory() {
        let args = Args::try_parse_from(["adaptive-ji", "--input", "KeyRig"]).unwrap();
        assert_eq!(args.memory_mode, adaptive_ji::tuning::MemoryMode::Relations);
        assert_eq!(
            OptimizerConfig::default().memory_mode,
            adaptive_ji::tuning::MemoryMode::Relations
        );
        assert_eq!(args.prime_limit.max_prime(), 11);
        assert_eq!(args.max_deviation, 50.0);
        assert_eq!(args.candidates_per_note, 8);
        assert_eq!(args.glide_ms, 35);
        let mut generator = CandidateGenerator::new(OptimizerConfig {
            prime_limit: args.prime_limit,
            max_deviation_cents: args.max_deviation,
            candidates_per_note: args.candidates_per_note,
            ..OptimizerConfig::default()
        })
        .unwrap();
        assert!(
            generator
                .for_interval(6)
                .candidates
                .iter()
                .any(|candidate| candidate.ratio == Ratio::new(11, 8).unwrap())
        );
    }

    #[test]
    fn exploratory_defaults_keep_simple_harmonic_chords() {
        let args = Args::try_parse_from(["adaptive-ji", "--input", "KeyRig"]).unwrap();
        let config = OptimizerConfig {
            prime_limit: args.prime_limit,
            max_deviation_cents: args.max_deviation,
            candidates_per_note: args.candidates_per_note,
            ..OptimizerConfig::default()
        };
        let mut optimizer = AdaptiveRationalOptimizer::new(config).unwrap();
        let mut notes = Vec::new();
        for (index, midi_note) in [60, 64, 67, 70].into_iter().enumerate() {
            notes.push(ActiveNote {
                id: index as u64 + 1,
                input_note: midi_note,
                current_offset_cents: None,
            });
            let tuned = optimizer.tune(&notes);
            assert_eq!(tuned.len(), notes.len());
            assert!(tuned.iter().all(|note| note.cents_offset.is_finite()));
            if index == 2 {
                assert_eq!(
                    tuned
                        .iter()
                        .map(|note| note.ratio.to_string())
                        .collect::<Vec<_>>(),
                    ["1/1", "5/4", "3/2"]
                );
            }
            if index == 3 {
                assert_eq!(
                    tuned
                        .iter()
                        .map(|note| note.ratio.to_string())
                        .collect::<Vec<_>>(),
                    ["1/1", "5/4", "3/2", "7/4"]
                );
            }
        }
    }
}
