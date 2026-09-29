//! AAX plugin descriptor and metadata handling.
//!
//! This module handles AAX-specific plugin metadata and descriptor generation.

/// AAX plugin category enumeration.
/// These categories help Pro Tools organize plugins in its plugin browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AaxCategory {
    /// None/unspecified
    None,
    /// EQ
    EQ,
    /// Dynamics (compressor, limiter, gate, etc.)
    Dynamics,
    /// Pitch Shift
    PitchShift,
    /// Reverb
    Reverb,
    /// Delay
    Delay,
    /// Modulation (chorus, flanger, phaser, etc.)
    Modulation,
    /// Harmonic (distortion, saturation, etc.)
    Harmonic,
    /// Noise Reduction
    NoiseReduction,
    /// Dither
    Dither,
    /// Sound Field (spatial processing)
    SoundField,
    /// Generic Effect
    Effect,
}

impl AaxCategory {
    /// Returns the AAX category constant value.
    /// These values correspond to the AAX SDK's PlugIn_Category enum.
    pub fn as_aax_constant(&self) -> i32 {
        match self {
            AaxCategory::None => 0,
            AaxCategory::EQ => 1,
            AaxCategory::Dynamics => 2,
            AaxCategory::PitchShift => 3,
            AaxCategory::Reverb => 4,
            AaxCategory::Delay => 5,
            AaxCategory::Modulation => 6,
            AaxCategory::Harmonic => 7,
            AaxCategory::NoiseReduction => 8,
            AaxCategory::Dither => 9,
            AaxCategory::SoundField => 10,
            AaxCategory::Effect => 11,
        }
    }
}

/// AAX plugin type ID enumeration.
/// Determines the processing mode of the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AaxTypeId {
    /// Native (real-time) processing
    /// This is the most common type for real-time effects and instruments.
    Native,
    /// AudioSuite (offline) processing
    /// Used for offline/non-real-time processing in Pro Tools.
    AudioSuite,
}

impl AaxTypeId {
    /// Returns the AAX type ID constant value.
    pub fn as_aax_constant(&self) -> i32 {
        match self {
            AaxTypeId::Native => 0,
            AaxTypeId::AudioSuite => 1,
        }
    }
}
