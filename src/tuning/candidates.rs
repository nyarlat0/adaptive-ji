use std::collections::BTreeMap;

use super::{Ratio, adaptive::OptimizerConfig, rational::harmonic_complexity};

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub ratio: Ratio,
    pub offset_cents: f64,
}

#[derive(Clone, Debug)]
pub struct CandidateSet {
    pub candidates: Vec<Candidate>,
    /// A finite 7-limit lattice cannot meet arbitrarily narrow windows for every interval.
    pub used_fallback: bool,
}

pub struct CandidateGenerator {
    config: OptimizerConfig,
    octave_pool: Vec<Ratio>,
    cache: BTreeMap<i16, CandidateSet>,
}

impl CandidateGenerator {
    pub fn new(config: OptimizerConfig) -> Result<Self, String> {
        config.validate()?;
        let mut octave_pool = Vec::with_capacity(315);
        for e3 in -4_i32..=4 {
            for e5 in -3_i32..=3 {
                for e7 in -2_i32..=2 {
                    let (mut num, mut den) = (1_u64, 1_u64);
                    for (prime, exponent) in [(3_u64, e3), (5, e5), (7, e7)] {
                        if exponent >= 0 {
                            num *= prime.pow(exponent as u32);
                        } else {
                            den *= prime.pow(exponent.unsigned_abs());
                        }
                    }
                    let ratio = Ratio::new(num, den).expect("positive bounded prime powers");
                    let octave = (ratio.cents() / 1200.0).floor() as i32;
                    octave_pool.push(ratio.scale_pow2(-octave).expect("bounded octave ratio"));
                }
            }
        }
        octave_pool.sort_unstable();
        octave_pool.dedup();
        Ok(Self {
            config,
            octave_pool,
            cache: BTreeMap::new(),
        })
    }

    /// MIDI intervals are -127..=127. Cache makes unchanged-reference retunes cheap.
    pub fn for_interval(&mut self, interval: i16) -> &CandidateSet {
        let config = &self.config;
        let pool = &self.octave_pool;
        self.cache.entry(interval).or_insert_with(|| {
            let target = f64::from(interval) * 100.0;
            let mut all: Vec<_> = pool
                .iter()
                .filter_map(|&ratio| {
                    let octave = ((target - ratio.cents()) / 1200.0).round() as i32;
                    let ratio = ratio.scale_pow2(octave)?;
                    Some(Candidate {
                        ratio,
                        offset_cents: ratio.cents() - target,
                    })
                })
                .collect();
            // Every valid MIDI interval is representable in this bounded pool.
            // The final fallback also keeps the public API total for extreme i16 inputs.
            if all.is_empty() {
                all.push(Candidate {
                    ratio: Ratio::ONE,
                    offset_cents: -target,
                });
            }
            let nearest = *all
                .iter()
                .min_by(|a, b| {
                    a.offset_cents
                        .abs()
                        .total_cmp(&b.offset_cents.abs())
                        .then_with(|| a.ratio.cmp(&b.ratio))
                })
                .expect("nonempty pool");
            all.retain(|candidate| {
                candidate.offset_cents.abs() <= config.max_deviation_cents + 1e-9
            });
            let used_fallback = all.is_empty();
            if used_fallback {
                log::warn!(
                    "No candidate within ±{}c for MIDI interval {interval}; using {} ({:+.3}c)",
                    config.max_deviation_cents,
                    nearest.ratio,
                    nearest.offset_cents
                );
                all.push(nearest);
            }
            let rank = |candidate: &Candidate| {
                config.weights.harmonic * harmonic_complexity(candidate.ratio)
                    + config.weights.deviation * (candidate.offset_cents / 25.0).powi(2)
            };
            all.sort_unstable_by(|a, b| {
                rank(a)
                    .total_cmp(&rank(b))
                    .then_with(|| a.offset_cents.abs().total_cmp(&b.offset_cents.abs()))
                    .then_with(|| a.ratio.cmp(&b.ratio))
            });
            all.truncate(config.candidates_per_note);
            CandidateSet {
                candidates: all,
                used_fallback,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_third_fifth_and_harmonic_seventh() {
        let mut generator = CandidateGenerator::new(OptimizerConfig::default()).unwrap();
        for (interval, num, den) in [(3, 6, 5), (4, 5, 4), (7, 3, 2), (10, 7, 4)] {
            let set = generator.for_interval(interval);
            assert!(!set.used_fallback);
            assert!(set.candidates.len() <= 8);
            assert!(
                set.candidates
                    .iter()
                    .any(|c| c.ratio == Ratio::new(num, den).unwrap())
            );
            assert!(set.candidates.iter().all(|c| c.offset_cents.abs() <= 35.0));
        }
    }

    #[test]
    fn negative_intervals_and_octaves() {
        let mut generator = CandidateGenerator::new(OptimizerConfig::default()).unwrap();
        for (interval, num, den) in [(-4, 4, 5), (-7, 2, 3), (-16, 2, 5), (16, 5, 2)] {
            assert!(
                generator
                    .for_interval(interval)
                    .candidates
                    .iter()
                    .any(|candidate| candidate.ratio == Ratio::new(num, den).unwrap())
            );
        }
    }

    #[test]
    fn all_midi_intervals_are_nonempty_bounded_and_seven_limit() {
        let mut generator = CandidateGenerator::new(OptimizerConfig::default()).unwrap();
        for interval in -127..=127 {
            let set = generator.for_interval(interval);
            assert!(!set.used_fallback);
            assert!(!set.candidates.is_empty());
            for candidate in &set.candidates {
                for mut part in [candidate.ratio.numerator(), candidate.ratio.denominator()] {
                    for prime in [2, 3, 5, 7] {
                        while part.is_multiple_of(prime) {
                            part /= prime;
                        }
                    }
                    assert_eq!(part, 1);
                }
                assert!(candidate.offset_cents.abs() <= 35.0);
            }
        }
    }

    #[test]
    fn impossible_window_has_explicit_nearest_fallback() {
        let mut generator = CandidateGenerator::new(OptimizerConfig {
            max_deviation_cents: 0.0,
            ..OptimizerConfig::default()
        })
        .unwrap();
        let set = generator.for_interval(1);
        assert!(set.used_fallback);
        assert_eq!(set.candidates.len(), 1);
        assert!(set.candidates[0].offset_cents.is_finite());
        assert!(!generator.for_interval(12).used_fallback);
    }
}
