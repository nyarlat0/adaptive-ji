use adaptive_ji::{
    glide::GlideConfig,
    midi,
    pitch_bend::BendRange,
    tuning::{
        AdaptiveRationalOptimizer, FiveLimitLowestRoot, OptimizerConfig, ScoreWeights,
        TuningStrategy, adaptive::AnchorMode, roughness::RoughnessConfig,
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

    /// Harmonic partials in the roughness model (1..=64)
    #[arg(long, default_value_t = 12)]
    roughness_partials: usize,

    /// Spectrum amplitude exponent (0.1..=4.0)
    #[arg(long, default_value_t = 1.0)]
    roughness_rolloff: f64,

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
        midi::list_inputs()
    } else {
        let config = OptimizerConfig {
            max_deviation_cents: args.max_deviation,
            candidates_per_note: args.candidates_per_note,
            beam_width: args.beam_width,
            anchor_mode: args.anchor_mode,
            anchor_max_cents: args.anchor_max,
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
