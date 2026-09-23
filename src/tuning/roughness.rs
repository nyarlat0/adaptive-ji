//! Approximate sensory dissonance between synthetic harmonic spectra.
//! This is a Sethares/Plomp-Levelt-style metric, not a model of a real patch or ear.

#[derive(Clone, Copy, Debug)]
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

/// Precomputes amplitude shape once per optimizer/scan, then applies it to each fundamental.
pub struct SpectrumModel {
    amplitudes: Vec<f64>,
}

impl SpectrumModel {
    pub fn new(config: RoughnessConfig) -> Result<Self, String> {
        config.validate()?;
        let mut amplitudes: Vec<_> = (1..=config.partials)
            .map(|n| 1.0 / (n as f64).powf(config.rolloff))
            .collect();
        let norm = amplitudes
            .iter()
            .map(|amplitude| amplitude * amplitude)
            .sum::<f64>()
            .sqrt();
        for amplitude in &mut amplitudes {
            *amplitude /= norm;
        }
        Ok(Self { amplitudes })
    }

    pub fn spectrum(&self, fundamental_hz: f64) -> Result<Spectrum, String> {
        if !fundamental_hz.is_finite()
            || fundamental_hz <= 0.0
            || fundamental_hz * self.amplitudes.len() as f64 > f64::MAX
        {
            return Err("fundamental frequency must be finite, positive and representable".into());
        }
        Ok(Spectrum {
            partials: self
                .amplitudes
                .iter()
                .enumerate()
                .map(|(index, &amplitude)| Partial {
                    frequency_hz: fundamental_hz * (index + 1) as f64,
                    amplitude,
                })
                .collect(),
        })
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
}
