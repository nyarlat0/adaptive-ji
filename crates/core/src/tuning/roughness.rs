//! Approximate sensory dissonance between synthetic spectra.
//! This is a Sethares/Plomp-Levelt-style metric, not a model of a real patch or ear.

use super::profile::{SpectrumPreset, SpectrumProfile};

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct RoughnessConfig {
    pub partials: usize,
    pub rolloff: f64,
}

impl Default for RoughnessConfig {
    fn default() -> Self {
        Self {
            partials: 12,
            rolloff: 1.0,
        }
    }
}

impl RoughnessConfig {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=64).contains(&self.partials) {
            return Err("roughness-partials must be in 1..=64".into());
        }
        if !self.rolloff.is_finite() || !(0.1..=4.0).contains(&self.rolloff) {
            return Err("roughness-rolloff must be finite and in 0.1..=4.0".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Partial {
    pub frequency_hz: f64,
    pub amplitude: f64,
}

#[derive(Clone, Debug)]
pub struct Spectrum {
    partials: Vec<Partial>,
}

impl Spectrum {
    pub fn partials(&self) -> &[Partial] {
        &self.partials
    }
}

/// Applies one immutable, unit-energy profile to every candidate fundamental.
pub struct SpectrumModel {
    profile: SpectrumProfile,
}

impl SpectrumModel {
    pub fn new(config: RoughnessConfig) -> Result<Self, String> {
        Ok(Self::from_profile(SpectrumProfile::builtin(
            SpectrumPreset::Harmonic,
            config,
        )?))
    }

    pub fn from_profile(profile: SpectrumProfile) -> Self {
        Self { profile }
    }

    pub fn profile(&self) -> &SpectrumProfile {
        &self.profile
    }

    pub fn spectrum(&self, fundamental_hz: f64) -> Result<Spectrum, String> {
        let mut spectrum = Spectrum {
            partials: Vec::with_capacity(self.profile.partials().len()),
        };
        self.fill_spectrum(fundamental_hz, &mut spectrum)?;
        Ok(spectrum)
    }

    /// Reuse worker scratch; no allocation after profile-sized initialization.
    pub fn fill_spectrum(
        &self,
        fundamental_hz: f64,
        spectrum: &mut Spectrum,
    ) -> Result<(), String> {
        if !fundamental_hz.is_finite() || fundamental_hz <= 0.0 {
            return Err("fundamental frequency must be finite and positive".into());
        }
        spectrum.partials.clear();
        for partial in self.profile.partials() {
            let frequency_hz = fundamental_hz * partial.frequency_ratio;
            if !frequency_hz.is_finite() || frequency_hz <= 0.0 {
                return Err("profile frequency is not representable for this fundamental".into());
            }
            spectrum.partials.push(Partial {
                frequency_hz,
                amplitude: partial.amplitude,
            });
        }
        Ok(())
    }
}

pub fn fundamental_frequency(midi_note: u8, offset_cents: f64) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(midi_note) - 69.0 + offset_cents / 100.0) / 12.0)
}

/// Cross-note partial pairs only. For f_min=min(f1,f2), delta=|f2-f1|:
/// s=0.24/(0.0207*f_min+18.96), x=s*delta,
/// r=a1*a2*(exp(-3.5*x)-exp(-5.75*x)).
pub fn roughness(a: &Spectrum, b: &Spectrum) -> f64 {
    let mut sum = 0.0;
    for left in &a.partials {
        for right in &b.partials {
            let minimum = left.frequency_hz.min(right.frequency_hz);
            let delta = (left.frequency_hz - right.frequency_hz).abs();
            let x = 0.24 * delta / (0.0207 * minimum + 18.96);
            let pair = left.amplitude * right.amplitude * ((-3.5 * x).exp() - (-5.75 * x).exp());
            sum += pair.max(0.0);
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::profile::{ProfilePartial, SpectrumPreset, SpectrumProfile};

    fn default_model() -> SpectrumModel {
        SpectrumModel::new(RoughnessConfig::default()).unwrap()
    }

    #[test]
    fn creates_harmonic_spectrum_with_unit_energy_amplitudes() {
        let spectrum = default_model().spectrum(220.0).unwrap();
        assert_eq!(spectrum.partials().len(), 12);
        assert!((spectrum.partials()[0].frequency_hz - 220.0).abs() < 1e-12);
        assert!((spectrum.partials()[1].frequency_hz - 440.0).abs() < 1e-12);
        assert!(
            (spectrum.partials()[0].amplitude / spectrum.partials()[1].amplitude - 2.0).abs()
                < 1e-12
        );
        let energy: f64 = spectrum
            .partials()
            .iter()
            .map(|p| p.amplitude.powi(2))
            .sum();
        assert!((energy - 1.0).abs() < 1e-12);
    }

    #[test]
    fn model_is_symmetric_finite_and_nonnegative() {
        let model = default_model();
        let a = model.spectrum(261.625565).unwrap();
        let b = model.spectrum(392.438348).unwrap();
        assert!((roughness(&a, &b) - roughness(&b, &a)).abs() < 1e-12);
        assert!(roughness(&a, &a).is_finite());
        assert!(roughness(&a, &a) >= 0.0);
        for left in [20.0, 55.0, 440.0, 20000.0] {
            for right in [20.0, 55.0, 440.0, 20000.0] {
                let value = roughness(
                    &model.spectrum(left).unwrap(),
                    &model.spectrum(right).unwrap(),
                );
                assert!(value.is_finite() && value >= 0.0);
            }
        }
    }

    #[test]
    fn exact_low_integer_intervals_beat_slight_detuning() {
        let model = default_model();
        let reference = model.spectrum(220.0).unwrap();
        for ratio in [2.0, 1.5, 1.25] {
            let exact = roughness(&reference, &model.spectrum(220.0 * ratio).unwrap());
            let detuned = roughness(
                &reference,
                &model
                    .spectrum(220.0 * ratio * 2.0_f64.powf(10.0 / 1200.0))
                    .unwrap(),
            );
            assert!(
                exact < detuned,
                "ratio={ratio}: exact={exact}, detuned={detuned}"
            );
        }
    }

    #[test]
    fn validates_model_settings_and_fundamental() {
        for partials in [0, 65] {
            assert!(
                SpectrumModel::new(RoughnessConfig {
                    partials,
                    ..RoughnessConfig::default()
                })
                .is_err()
            );
        }
        for rolloff in [0.0, 4.1, f64::NAN, f64::INFINITY] {
            assert!(
                SpectrumModel::new(RoughnessConfig {
                    rolloff,
                    ..RoughnessConfig::default()
                })
                .is_err()
            );
        }
        for partials in [1, 64] {
            let model = SpectrumModel::new(RoughnessConfig {
                partials,
                rolloff: 0.1,
            })
            .unwrap();
            let spectrum = model.spectrum(20000.0).unwrap();
            assert_eq!(spectrum.partials().len(), partials);
            let energy: f64 = spectrum
                .partials()
                .iter()
                .map(|p| p.amplitude.powi(2))
                .sum();
            assert!((energy - 1.0).abs() < 1e-12);
        }
        let model = default_model();
        assert!(model.spectrum(0.0).is_err());
        assert!(model.spectrum(f64::NAN).is_err());
        assert!(model.spectrum(f64::INFINITY).is_err());
        assert!((fundamental_frequency(69, 0.0) - 440.0).abs() < 1e-12);
        assert!((fundamental_frequency(60, 0.0) - 261.625565).abs() < 1e-6);
    }

    #[test]
    fn harmonic_default_regresses_v06_values_and_sine_changes_roughness() {
        let harmonic = default_model();
        let sine = SpectrumModel::from_profile(
            SpectrumProfile::builtin(SpectrumPreset::Sine, RoughnessConfig::default()).unwrap(),
        );
        let f = fundamental_frequency(64, 0.0);
        let harmonic_six_five = roughness(
            &harmonic.spectrum(f).unwrap(),
            &harmonic.spectrum(f * 6.0 / 5.0).unwrap(),
        );
        let harmonic_32_27 = roughness(
            &harmonic.spectrum(f).unwrap(),
            &harmonic.spectrum(f * 32.0 / 27.0).unwrap(),
        );
        assert!((harmonic_six_five - 0.082175).abs() < 1e-5);
        assert!((harmonic_32_27 - 0.093253).abs() < 1e-5);
        let sine_six_five = roughness(
            &sine.spectrum(f).unwrap(),
            &sine.spectrum(f * 6.0 / 5.0).unwrap(),
        );
        assert!((sine_six_five - harmonic_six_five).abs() > 0.001);
        assert!(
            (sine_six_five
                - roughness(
                    &sine.spectrum(f * 6.0 / 5.0).unwrap(),
                    &sine.spectrum(f).unwrap()
                ))
            .abs()
                < 1e-12
        );
    }

    #[test]
    fn profile_changes_candidate_ranking_without_special_case_rules() {
        let f = fundamental_frequency(60, 0.0);
        let config = RoughnessConfig::default();
        let sine = SpectrumModel::from_profile(
            SpectrumProfile::builtin(SpectrumPreset::Sine, config).unwrap(),
        );
        let bright = SpectrumModel::from_profile(
            SpectrumProfile::builtin(SpectrumPreset::Bright, config).unwrap(),
        );
        let pair = |model: &SpectrumModel, ratio: f64| {
            roughness(
                &model.spectrum(f).unwrap(),
                &model.spectrum(f * ratio).unwrap(),
            )
        };
        assert!(pair(&sine, 32.0 / 21.0) < pair(&sine, 3.0 / 2.0));
        assert!(pair(&bright, 3.0 / 2.0) < pair(&bright, 32.0 / 21.0));
    }

    #[test]
    fn eleven_limit_roughness_ranking_depends_on_spectrum_profile() {
        let f = fundamental_frequency(60, 0.0);
        let config = RoughnessConfig::default();
        let sine = SpectrumModel::from_profile(
            SpectrumProfile::builtin(SpectrumPreset::Sine, config).unwrap(),
        );
        let bright = SpectrumModel::from_profile(
            SpectrumProfile::builtin(SpectrumPreset::Bright, config).unwrap(),
        );
        let compare = |model: &SpectrumModel, ratio: f64| {
            roughness(
                &model.spectrum(f).unwrap(),
                &model.spectrum(f * ratio).unwrap(),
            )
        };
        assert!(compare(&sine, 16.0 / 11.0) < compare(&sine, 11.0 / 8.0));
        assert!(compare(&bright, 11.0 / 8.0) < compare(&bright, 16.0 / 11.0));
    }

    #[test]
    fn fractional_partial_ratios_scale_with_fundamental() {
        let profile = SpectrumProfile::new(vec![
            ProfilePartial {
                frequency_ratio: 1.0,
                amplitude: 1.0,
            },
            ProfilePartial {
                frequency_ratio: 4.03,
                amplitude: 0.2,
            },
        ])
        .unwrap();
        let spectrum = SpectrumModel::from_profile(profile)
            .spectrum(220.0)
            .unwrap();
        assert!((spectrum.partials()[1].frequency_hz - 886.6).abs() < 1e-10);
    }
}
