use adaptive_ji::{midi, pitch_bend::BendRange};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Dynamic five-limit just intonation over Linux ALSA MIDI/MPE"
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

    /// Log note changes, tuning, and ignored MIDI messages
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if args.verbose { "debug" } else { "warn" }),
    )
    .format_timestamp(None)
    .format_target(false)
    .init();

    if args.list_inputs {
        midi::list_inputs()
    } else {
        midi::run(
            args.input.as_deref().expect("clap requires --input"),
            args.bend_range,
        )
    }
}
