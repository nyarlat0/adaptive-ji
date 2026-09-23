use adaptive_ji::{
    midi,
    pitch_bend::BendRange,
    tuning::{
        AdaptiveRationalOptimizer, FiveLimitLowestRoot, OptimizerConfig, ScoreWeights,
        TuningStrategy, roughness::RoughnessConfig,
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
    about = "Contextual adaptive just intonation over Linux ALSA MIDI/MPE"
)]
struct Args {
    /// List available MIDI input ports (indices start at 0)
    #[arg(long)]
    list_inputs: bool,

    /// Input port index, exact name, or unique case-insensitive name fragment
    #[arg(long, required_unless_present = "list_inputs", conflicts_with = "list_inputs", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    input: Option<String>,

    /// Per-note pitch bend range in semitones (0.01..=127.99, steps of 0.01)
    #[arg(long, default_value = "2")]
    bend_range: BendRange,

    /// Tuning strategy (five-limit preserves the v0.1 lowest-root baseline)
    #[arg(long, value_enum, default_value = "adaptive")]
    strategy: Strategy,

    /// Candidate window around 12-TET, in cents (0..=100)
    #[arg(long, default_value_t = 35.0)]
    max_deviation: f64,

    /// Maximum candidates per voice (1..=32)
    #[arg(long, default_value_t = 8)]
    candidates_per_note: usize,

    /// Maximum partial assignments retained by beam search (1..=16384)
    #[arg(long, default_value_t = 2048)]
    beam_width: usize,

    #[arg(long, default_value_t = 1.0)]
    w_harmonic: f64,
    #[arg(long, default_value_t = 0.35)]
    w_deviation: f64,
    #[arg(long, default_value_t = 1.5)]
    w_movement: f64,
    /// Weight of cross-note synthetic partial roughness (0 restores v0.2 scoring)
    #[arg(long, default_value_t = 20.0)]
    w_roughness: f64,

    /// Harmonic partials in the roughness model (1..=64)
    #[arg(long, default_value_t = 12)]
    roughness_partials: usize,

    /// Spectrum amplitude exponent (0.1..=4.0)
    #[arg(long, default_value_t = 1.0)]
    roughness_rolloff: f64,

    /// Log notes, tuning, and adaptive costs; -vv also logs ignored MIDI messages
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
        midi::list_inputs()
    } else {
        let config = OptimizerConfig {
            max_deviation_cents: args.max_deviation,
            candidates_per_note: args.candidates_per_note,
            beam_width: args.beam_width,
            weights: ScoreWeights {
                harmonic: args.w_harmonic,
                deviation: args.w_deviation,
                movement: args.w_movement,
                roughness: args.w_roughness,
            },
            roughness: RoughnessConfig {
                partials: args.roughness_partials,
                rolloff: args.roughness_rolloff,
            },
        };
        config.validate().map_err(anyhow::Error::msg)?;
        let tuning: Box<dyn TuningStrategy> = match args.strategy {
            Strategy::Adaptive => {
                Box::new(AdaptiveRationalOptimizer::new(config).map_err(anyhow::Error::msg)?)
            }
            Strategy::FiveLimit => Box::new(FiveLimitLowestRoot),
        };
        midi::run(
            args.input.as_deref().expect("clap requires --input"),
            args.bend_range,
            tuning,
        )
    }
}
