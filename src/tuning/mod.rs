mod five_limit;

pub use five_limit::FiveLimitLowestRoot;

pub type VoiceId = u64;

#[derive(Clone, Copy, Debug)]
pub struct ActiveNote {
    pub id: VoiceId,
    pub input_note: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}

impl std::fmt::Display for Ratio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TunedNote {
    pub id: VoiceId,
    pub ratio: Ratio,
    pub cents_offset: f64,
}

/// Return one tuning per voice ID, including repeated instances of the same key.
pub trait TuningStrategy {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote>;
}
