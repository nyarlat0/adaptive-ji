//! Immutable, unit-energy spectral shapes shared by all candidate notes.

#[cfg(test)]
use std::{fs, path::Path};

use serde::Deserialize;

use super::roughness::RoughnessConfig;

pub const MAX_PROFILE_PARTIALS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfilePartial {
    pub frequency_ratio: f64,
    pub amplitude: f64,
}

#[derive(Clone, Debug)]
pub struct SpectrumProfile {
    partials: Vec<ProfilePartial>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum SpectrumPreset {
    #[default]
    Harmonic,
    Sine,
    Saw,
    Square,
    Soft,
    Bright,
}

impl SpectrumProfile {
    pub fn new(mut partials: Vec<ProfilePartial>) -> Result<Self, String> {
        if partials.is_empty() || partials.len() > MAX_PROFILE_PARTIALS {
            return Err(format!(
                "spectrum must contain 1..={MAX_PROFILE_PARTIALS} partials"
            ));
        }
        for (index, partial) in partials.iter().enumerate() {
            if !partial.frequency_ratio.is_finite() || partial.frequency_ratio <= 0.0 {
                return Err(format!(
                    "partial {} ratio must be finite and > 0",
                    index + 1
                ));
            }
            if !partial.amplitude.is_finite() || partial.amplitude < 0.0 {
                return Err(format!(
                    "partial {} amplitude must be finite and >= 0",
                    index + 1
                ));
            }
        }
        partials.retain(|partial| partial.amplitude > 0.0);
        let peak = partials
            .iter()
            .map(|partial| partial.amplitude)
            .fold(0.0_f64, f64::max);
        if peak == 0.0 {
            return Err("spectrum must contain a positive-amplitude partial".into());
        }
        // Scale before squaring so even very large finite input amplitudes can
        // be normalized without overflow.
        let norm = partials
            .iter()
            .map(|partial| (partial.amplitude / peak).powi(2))
            .sum::<f64>()
            .sqrt();
        for partial in &mut partials {
            partial.amplitude = (partial.amplitude / peak) / norm;
        }
        Ok(Self { partials })
    }

    pub fn partials(&self) -> &[ProfilePartial] {
        &self.partials
    }

    pub fn builtin(preset: SpectrumPreset, config: RoughnessConfig) -> Result<Self, String> {
        config.validate()?;
        if preset == SpectrumPreset::Sine {
            return Self::new(vec![ProfilePartial {
                frequency_ratio: 1.0,
                amplitude: 1.0,
            }]);
        }
        let exponent = match preset {
            SpectrumPreset::Harmonic | SpectrumPreset::Saw | SpectrumPreset::Square => {
                config.rolloff
            }
            SpectrumPreset::Soft => 2.0 * config.rolloff,
            SpectrumPreset::Bright => 0.5 * config.rolloff,
            SpectrumPreset::Sine => unreachable!(),
        };
        Self::new(
            (1..=config.partials)
                .filter(|n| preset != SpectrumPreset::Square || n % 2 == 1)
                .map(|n| ProfilePartial {
                    frequency_ratio: n as f64,
                    amplitude: 1.0 / (n as f64).powf(exponent),
                })
                .collect(),
        )
    }

    pub fn from_toml_str(input: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FileProfile {
            partial: Vec<FilePartial>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FilePartial {
            ratio: f64,
            amplitude: f64,
        }

        let parsed: FileProfile =
            toml::from_str(input).map_err(|error| format!("invalid spectrum TOML: {error}"))?;
        Self::new(
            parsed
                .partial
                .into_iter()
                .map(|partial| ProfilePartial {
                    frequency_ratio: partial.ratio,
                    amplitude: partial.amplitude,
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub fn from_toml_file(path: &Path) -> Result<Self, String> {
        let input = fs::read_to_string(path)
            .map_err(|error| format!("cannot read spectrum file {}: {error}", path.display()))?;
        Self::from_toml_str(&input)
            .map_err(|error| format!("spectrum file {}: {error}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(profile: &SpectrumProfile) -> f64 {
        profile.partials().iter().map(|p| p.amplitude.powi(2)).sum()
    }

    #[test]
    fn normalizes_and_removes_zero_amplitude_partials() {
        let profile = SpectrumProfile::new(vec![
            ProfilePartial {
                frequency_ratio: 1.0,
                amplitude: 3.0,
            },
            ProfilePartial {
                frequency_ratio: 2.0,
                amplitude: 0.0,
            },
            ProfilePartial {
                frequency_ratio: 2.001,
                amplitude: 4.0,
            },
        ])
        .unwrap();
        assert_eq!(profile.partials().len(), 2);
        assert_eq!(profile.partials()[1].frequency_ratio, 2.001);
        assert!((energy(&profile) - 1.0).abs() < 1e-12);
        assert!((profile.partials()[0].amplitude - 0.6).abs() < 1e-12);
        assert!((profile.partials()[1].amplitude - 0.8).abs() < 1e-12);
    }

    #[test]
    fn validates_ratio_amplitude_size_and_nonempty_energy() {
        for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                SpectrumProfile::new(vec![ProfilePartial {
                    frequency_ratio: ratio,
                    amplitude: 1.0
                }])
                .is_err()
            );
        }
        for amplitude in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                SpectrumProfile::new(vec![ProfilePartial {
                    frequency_ratio: 1.0,
                    amplitude
                }])
                .is_err()
            );
        }
        assert!(SpectrumProfile::new(vec![]).is_err());
        assert!(
            SpectrumProfile::new(vec![ProfilePartial {
                frequency_ratio: 1.0,
                amplitude: 0.0
            }])
            .is_err()
        );
        assert!(
            SpectrumProfile::new(vec![
                ProfilePartial {
                    frequency_ratio: 1.0,
                    amplitude: 1.0
                };
                MAX_PROFILE_PARTIALS + 1
            ])
            .is_err()
        );
    }

    #[test]
    fn builtin_shapes_have_expected_partials_and_energy() {
        let config = RoughnessConfig::default();
        let sine = SpectrumProfile::builtin(SpectrumPreset::Sine, config).unwrap();
        assert_eq!(
            sine.partials(),
            &[ProfilePartial {
                frequency_ratio: 1.0,
                amplitude: 1.0
            }]
        );
        let harmonic = SpectrumProfile::builtin(SpectrumPreset::Harmonic, config).unwrap();
        let saw = SpectrumProfile::builtin(SpectrumPreset::Saw, config).unwrap();
        assert_eq!(harmonic.partials(), saw.partials());
        assert_eq!(harmonic.partials().len(), 12);
        assert!(
            (harmonic.partials()[0].amplitude / harmonic.partials()[1].amplitude - 2.0).abs()
                < 1e-12
        );
        let square = SpectrumProfile::builtin(SpectrumPreset::Square, config).unwrap();
        assert!(
            square
                .partials()
                .iter()
                .all(|p| p.frequency_ratio as usize % 2 == 1)
        );
        assert_eq!(square.partials().len(), 6);
        let soft = SpectrumProfile::builtin(SpectrumPreset::Soft, config).unwrap();
        let bright = SpectrumProfile::builtin(SpectrumPreset::Bright, config).unwrap();
        assert!((soft.partials()[0].amplitude / soft.partials()[1].amplitude - 4.0).abs() < 1e-12);
        assert!(
            (bright.partials()[0].amplitude / bright.partials()[1].amplitude - 2.0_f64.sqrt())
                .abs()
                < 1e-12
        );
        for profile in [&sine, &harmonic, &square, &soft, &bright] {
            assert!((energy(profile) - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn parses_fractional_toml_and_rejects_invalid_files() {
        let valid = "[[partial]]\nratio = 1.0\namplitude = 1.0\n[[partial]]\nratio = 4.012\namplitude = 0.2\n";
        let profile = SpectrumProfile::from_toml_str(valid).unwrap();
        assert_eq!(profile.partials()[1].frequency_ratio, 4.012);
        for invalid in [
            "",
            "[[partial]]\nratio = 2.0",
            "[[partial]]\nratio = -2.0\namplitude = 1.0",
            "[[partial]]\nratio = 1.0\namplitude = nan",
            "[[partial]]\nratio = 1.0\namplitude = -1.0",
            "[[partial]]\nratio = 1.0\namplitude = 0.0",
        ] {
            assert!(
                SpectrumProfile::from_toml_str(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(
            SpectrumProfile::from_toml_file(Path::new("/path/that/does/not/exist/spectrum.toml"))
                .is_err()
        );
    }

    #[test]
    fn loads_custom_toml_file_and_accepts_128_partials_without_truncation() {
        let path = std::env::temp_dir().join(format!(
            "adaptive-ji-spectrum-test-{}.toml",
            std::process::id()
        ));
        let content = "[[partial]]\nratio = 1.0\namplitude = 1.0\n[[partial]]\nratio = 2.001\namplitude = 0.5\n";
        std::fs::write(&path, content).unwrap();
        let profile = SpectrumProfile::from_toml_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(profile.partials()[1].frequency_ratio, 2.001);
        assert!((energy(&profile) - 1.0).abs() < 1e-12);
        let full = SpectrumProfile::new(
            (1..=128)
                .map(|n| ProfilePartial {
                    frequency_ratio: n as f64,
                    amplitude: 1.0 / n as f64,
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(full.partials().len(), 128);
    }
}
