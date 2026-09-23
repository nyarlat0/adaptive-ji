pub mod adaptive;
pub mod candidates;
mod five_limit;
pub mod rational;
pub mod roughness;

pub use adaptive::{AdaptiveRationalOptimizer, OptimizerConfig, ScoreWeights};
pub use five_limit::FiveLimitLowestRoot;
pub use rational::Ratio;

pub type VoiceId = u64;

#[derive(Clone, Copy, Debug)]
pub struct ActiveNote {
    pub id: VoiceId,
    pub input_note: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TunedNote {
    pub id: VoiceId,
    pub ratio: Ratio,
    pub cents_offset: f64,
}

/// Return one tuning per voice ID, including repeated instances of the same key.
pub trait TuningStrategy {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote>;
}

impl<T: TuningStrategy + ?Sized> TuningStrategy for Box<T> {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote> {
        (**self).tune(active_notes)
    }
}
