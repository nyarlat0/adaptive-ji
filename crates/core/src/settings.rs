//! Portable project configuration. File contents, never paths, define sound.
use crate::{
    glide::GlideConfig,
    pitch_bend::BendRange,
    tuning::{
        AdaptiveRationalOptimizer, OptimizerConfig, PrimeLimit,
        profile::{SpectrumPreset, SpectrumProfile},
        scala::{KeyboardMapping, ScalaScale, ScalaTuning},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedScale {
    pub name: String,
    pub scl: String,
    pub kbm: Option<String>,
    pub root_note: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSettings {
    pub version: u32,
    pub optimizer: OptimizerConfig,
    pub spectrum: SpectrumPreset,
    pub custom_spectrum_toml: Option<String>,
    pub scale: Option<EmbeddedScale>,
    pub glide: GlideConfig,
    pub bend_range: f64,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            version: 1,
            optimizer: OptimizerConfig {
                prime_limit: PrimeLimit::new(11).unwrap(),
                max_deviation_cents: 50.0,
                ..OptimizerConfig::default()
            },
            spectrum: SpectrumPreset::Harmonic,
            custom_spectrum_toml: None,
            scale: None,
            glide: GlideConfig::default(),
            bend_range: 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BasePitch {
    pub output_note: u8,
    pub offset: f64,
}

pub struct PreparedSettings {
    pub optimizer: AdaptiveRationalOptimizer,
    pub base: [Option<BasePitch>; 128],
    pub bend_range: BendRange,
    pub glide: GlideConfig,
}

impl EngineSettings {
    /// Non-realtime: validates and parses only embedded data, no filesystem IO.
    pub fn prepare(&self) -> Result<PreparedSettings, String> {
        if self.version != 1 {
            return Err("unsupported engine state version".into());
        }
        self.optimizer.validate()?;
        self.glide.validate()?;
        let range: BendRange = self.bend_range.to_string().parse()?;
        let scale = self
            .scale
            .as_ref()
            .map(|source| {
                if source.root_note > 127 {
                    return Err("root-note must be 0..127".into());
                }
                ScalaTuning::new(
                    ScalaScale::parse(&source.scl)?,
                    source
                        .kbm
                        .as_deref()
                        .map(KeyboardMapping::parse)
                        .transpose()?,
                    source.root_note,
                    self.bend_range,
                )?
                .with_adaptive_headroom(self.bend_range, self.optimizer.max_deviation_cents)
            })
            .transpose()?;
        let base = std::array::from_fn(|note| match &scale {
            Some(scale) => scale
                .output_note_for(note as u8)
                .zip(scale.baseline_offset(note as u8))
                .map(|(output_note, offset)| BasePitch {
                    output_note,
                    offset,
                }),
            None => Some(BasePitch {
                output_note: note as u8,
                offset: 0.0,
            }),
        });
        let profile = match &self.custom_spectrum_toml {
            Some(text) => SpectrumProfile::from_toml_str(text)?,
            None => SpectrumProfile::builtin(self.spectrum, self.optimizer.roughness)?,
        };
        Ok(PreparedSettings {
            optimizer: AdaptiveRationalOptimizer::with_profile_and_scale(
                self.optimizer,
                profile,
                scale,
            )?,
            base,
            bend_range: range,
            glide: self.glide,
        })
    }
}
