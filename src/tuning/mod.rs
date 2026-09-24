pub mod adaptive;
pub mod candidates;
mod five_limit;
pub mod memory;
pub mod prime_limit;
pub mod profile;
pub mod rational;
pub mod roughness;

pub use adaptive::{AdaptiveRationalOptimizer, OptimizerConfig, ScoreWeights};
pub use five_limit::FiveLimitLowestRoot;
pub use memory::MemoryMode;
pub use prime_limit::PrimeLimit;
pub use rational::Ratio;

pub type VoiceId = u64;

#[derive(Clone, Copy, Debug)]
pub struct ActiveNote {
    pub id: VoiceId,
    pub input_note: u8,
    /// The audible pitch at the retune snapshot. `None` means this is a new
    /// voice and therefore receives no movement penalty.
    pub current_offset_cents: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TunedNote {
    pub id: VoiceId,
    pub ratio: Ratio,
    pub cents_offset: f64,
}

/// Return one tuning per voice ID, including repeated instances of the same key.
/// `ActiveNote::current_offset_cents` is one coherent audible-position
/// snapshot. Strategies must use it for movement costs instead of retaining
/// their own notion of the previous target.
pub trait TuningStrategy {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote>;
}

impl<T: TuningStrategy + ?Sized> TuningStrategy for Box<T> {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote> {
        (**self).tune(active_notes)
    }
}
