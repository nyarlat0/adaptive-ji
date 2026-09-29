use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};

use super::{
    Ratio, adaptive::OptimizerConfig, prime_limit::PrimeLimit, rational::harmonic_complexity,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub ratio: Ratio,
    pub offset_cents: f64,
}

#[derive(Clone, Debug)]
pub struct CandidateSet {
    pub candidates: Vec<Candidate>,
    pub generated_count: usize,
    /// A finite prime-limited lattice cannot meet arbitrarily narrow windows for every interval.
    pub used_fallback: bool,
}

type CandidateCacheKey = (i16, u64, PrimeLimit, u64, u64, usize, u64, u64);

pub struct CandidateGenerator {
    config: OptimizerConfig,
    odd_pool: Vec<Ratio>,
    cache: BTreeMap<CandidateCacheKey, CandidateSet>,
    pub visited_search_nodes: usize,
}

impl CandidateGenerator {
    pub fn new(config: OptimizerConfig) -> Result<Self, String> {
        config.validate()?;
        let odd_pool = bounded_odd_ratios(config.prime_limit, config.max_complexity);
        let visited_search_nodes = odd_pool.len();
        Ok(Self {
            config,
            odd_pool,
            cache: BTreeMap::new(),
            visited_search_nodes,
        })
    }

    pub fn effective_primes(&self) -> Vec<u32> {
        self.config
            .prime_limit
            .effective_primes(self.config.max_complexity)
    }

    pub fn generated_candidates(&self, interval: i16) -> Option<usize> {
        self.cache
            .iter()
            .find(|((cached_interval, ..), _)| *cached_interval == interval)
            .map(|(_, set)| set.generated_count)
    }

    /// MIDI intervals are -127..=127. Cache makes unchanged-reference retunes cheap.
    pub fn for_interval(&mut self, interval: i16) -> &CandidateSet {
        self.for_target(interval, 0.0)
    }

    /// `baseline_difference` shifts the Scala target interval relative to 12-TET.
    pub fn for_target(&mut self, interval: i16, baseline_difference: f64) -> &CandidateSet {
        let config = &self.config;
        let key = (
            interval,
            baseline_difference.to_bits(),
            config.prime_limit,
            config.max_deviation_cents.to_bits(),
            config.max_complexity.to_bits(),
            config.candidates_per_note,
            config.weights.harmonic.to_bits(),
            config.weights.deviation.to_bits(),
        );
        self.cache.entry(key).or_insert_with(|| {
            let target = f64::from(interval) * 100.0 + baseline_difference;
            let mut all: Vec<_> = self
                .odd_pool
                .iter()
                .filter_map(|&ratio| {
                    let octave = ((target - ratio.cents()) / 1200.0).round() as i32;
                    let ratio = ratio.scale_pow2(octave)?;
                    Some(Candidate {
                        ratio,
                        offset_cents: ratio.cents() - f64::from(interval) * 100.0,
                    })
                })
                .collect();
            // Every valid MIDI interval is representable in this bounded pool.
            // The final fallback also keeps the public API total for extreme i16 inputs.
            if all.is_empty() {
                all.push(Candidate {
                    ratio: Ratio::ONE,
                    offset_cents: -f64::from(interval) * 100.0,
                });
            }
            let nearest = *all
                .iter()
                .min_by(|a, b| {
                    (a.offset_cents - baseline_difference)
                        .abs()
                        .total_cmp(&(b.offset_cents - baseline_difference).abs())
                        .then_with(|| a.ratio.cmp(&b.ratio))
                })
                .expect("nonempty pool");
            all.retain(|candidate| {
                (candidate.offset_cents - baseline_difference).abs()
                    <= config.max_deviation_cents + 1e-9
            });
            let used_fallback = all.is_empty();
            if used_fallback {
                log::warn!(
                    "No candidate within ±{}c for MIDI interval {interval}; using {} ({:+.3}c)",
                    config.max_deviation_cents,
                    nearest.ratio,
                    nearest.offset_cents - baseline_difference
                );
                all.push(nearest);
            }
            let rank = |candidate: &Candidate| {
                config.weights.harmonic * harmonic_complexity(candidate.ratio)
                    + config.weights.deviation
                        * ((candidate.offset_cents - baseline_difference) / 25.0).powi(2)
            };
            all.sort_unstable_by(|a, b| {
                rank(a)
                    .total_cmp(&rank(b))
                    .then_with(|| {
                        (a.offset_cents - baseline_difference)
                            .abs()
                            .total_cmp(&(b.offset_cents - baseline_difference).abs())
                    })
                    .then_with(|| a.ratio.cmp(&b.ratio))
            });
            let generated_count = all.len();
            all.truncate(config.candidates_per_note);
            CandidateSet {
                candidates: all,
                generated_count,
                used_fallback,
            }
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchState {
    ratio: Ratio,
    complexity: f64,
}

impl PartialEq for SearchState {
    fn eq(&self, other: &Self) -> bool {
        self.ratio == other.ratio
    }
}
impl Eq for SearchState {}
impl Ord for SearchState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .complexity
            .total_cmp(&self.complexity)
            .then_with(|| other.ratio.cmp(&self.ratio))
    }
}
impl PartialOrd for SearchState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Best-first expansion by Tenney lower bound. Each expansion changes one
/// prime exponent by ±1; complexity strictly grows, so the queue is finite.
fn bounded_odd_ratios(limit: PrimeLimit, budget: f64) -> Vec<Ratio> {
    let primes = limit.effective_primes(budget);
    let mut queue = BinaryHeap::from([SearchState {
        ratio: Ratio::ONE,
        complexity: 0.0,
    }]);
    let mut seen = BTreeSet::from([Ratio::ONE]);
    let mut output = Vec::new();
    let node_cap = 50_000usize;
    while let Some(state) = queue.pop() {
        if state.complexity > budget + 1e-9 {
            continue;
        }
        output.push(state.ratio);
        if output.len() >= node_cap {
            break;
        }
        for &prime in &primes {
            let factor = Ratio::new(u64::from(prime), 1).expect("prime");
            for neighbor in [
                state.ratio.checked_mul(factor),
                state.ratio.checked_div(factor),
            ] {
                let Some(neighbor) = neighbor else { continue };
                if !seen.insert(neighbor) {
                    continue;
                }
                let complexity = harmonic_complexity(neighbor);
                if complexity <= budget + 1e-9 {
                    queue.push(SearchState {
                        ratio: neighbor,
                        complexity,
                    });
                }
            }
        }
    }
    output.sort_unstable();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scala_target_shift_changes_candidates_and_has_distinct_cache_entry() {
        let mut generator = CandidateGenerator::new(OptimizerConfig::default()).unwrap();
        let plain = generator.for_target(4, 0.0).candidates.clone();
        let shifted = generator.for_target(4, -45.0).candidates.clone();
        assert_ne!(plain, shifted);
        assert_eq!(plain, generator.for_target(4, 0.0).candidates);
        assert_eq!(shifted, generator.for_target(4, -45.0).candidates);
    }

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

    fn make_generator(limit: PrimeLimit, max_deviation_cents: f64) -> CandidateGenerator {
        CandidateGenerator::new(OptimizerConfig {
            prime_limit: limit,
            max_deviation_cents,
            candidates_per_note: 32,
            ..OptimizerConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn prime_limits_filter_lattice_axes() {
        let mut five = make_generator(PrimeLimit::new(5).unwrap(), 60.0);
        let mut seven = make_generator(PrimeLimit::new(7).unwrap(), 60.0);
        let mut eleven = make_generator(PrimeLimit::new(11).unwrap(), 60.0);
        let fifth = five.for_interval(6).candidates.clone();
        assert!(!fifth.iter().any(|c| c.ratio == Ratio::new(11, 8).unwrap()));
        assert!(!fifth.iter().any(|c| c.ratio == Ratio::new(7, 4).unwrap()));
        assert!(seven.for_interval(6).candidates.iter().all(|c| {
            super::super::rational::prime_factorization_without_two(c.ratio)
                .keys()
                .all(|prime| *prime <= 7)
        }));
        assert!(
            !seven
                .for_interval(6)
                .candidates
                .iter()
                .any(|c| c.ratio == Ratio::new(11, 8).unwrap())
        );
        assert!(
            eleven
                .for_interval(6)
                .candidates
                .iter()
                .any(|c| c.ratio == Ratio::new(11, 8).unwrap())
        );
        for interval in -24..=24 {
            for candidate in &make_generator(PrimeLimit::new(5).unwrap(), 60.0)
                .for_interval(interval)
                .candidates
                .clone()
            {
                assert!(
                    super::super::rational::prime_factorization_without_two(candidate.ratio)
                        .keys()
                        .all(|prime| *prime <= 5)
                );
            }
        }
    }

    #[test]
    fn undecimal_tritone_candidate_requires_wide_window_and_is_deterministic() {
        let mut narrow = make_generator(PrimeLimit::new(11).unwrap(), 35.0);
        assert!(
            !narrow
                .for_interval(6)
                .candidates
                .iter()
                .any(|c| c.ratio == Ratio::new(11, 8).unwrap())
        );
        let mut wide = make_generator(PrimeLimit::new(11).unwrap(), 60.0);
        let first = wide.for_interval(6).candidates.clone();
        assert!(first.iter().any(|c| {
            c.ratio == Ratio::new(11, 8).unwrap() && (c.offset_cents + 48.6820576).abs() < 1e-5
        }));
        let second = wide.for_interval(6).candidates.clone();
        assert_eq!(first, second);

        let mut default_k = CandidateGenerator::new(OptimizerConfig {
            prime_limit: PrimeLimit::new(11).unwrap(),
            max_deviation_cents: 60.0,
            ..OptimizerConfig::default()
        })
        .unwrap();
        assert!(
            default_k
                .for_interval(6)
                .candidates
                .iter()
                .any(|candidate| candidate.ratio == Ratio::new(11, 8).unwrap())
        );
    }

    #[test]
    fn cache_key_includes_prime_limit() {
        let mut generator = make_generator(PrimeLimit::new(7).unwrap(), 60.0);
        let seven = generator.for_interval(6).candidates.clone();
        assert!(!seven.iter().any(|c| c.ratio == Ratio::new(11, 8).unwrap()));
        // A separate generator has a separate lattice/cache and must expose 11/8.
        let mut eleven = make_generator(PrimeLimit::new(11).unwrap(), 60.0);
        assert!(
            eleven
                .for_interval(6)
                .candidates
                .iter()
                .any(|c| c.ratio == Ratio::new(11, 8).unwrap())
        );
    }

    #[test]
    fn characteristic_undecimal_ratios_are_reachable_with_wide_window() {
        let mut generator = make_generator(PrimeLimit::new(11).unwrap(), 60.0);
        for (interval, ratio) in [
            (6, Ratio::new(11, 8).unwrap()),
            (8, Ratio::new(11, 7).unwrap()),
            (4, Ratio::new(14, 11).unwrap()),
            (10, Ratio::new(20, 11).unwrap()),
        ] {
            assert!(
                generator
                    .for_interval(interval)
                    .candidates
                    .iter()
                    .any(|candidate| candidate.ratio == ratio),
                "missing {ratio} at interval {interval}"
            );
        }
    }

    #[test]
    fn arbitrary_prime_limits_reach_new_prime_factors() {
        for (limit, interval, ratio) in [
            (13, 8, Ratio::new(13, 8).unwrap()),
            (17, 1, Ratio::new(17, 16).unwrap()),
            (19, 3, Ratio::new(19, 16).unwrap()),
        ] {
            let mut generator = make_generator(PrimeLimit::new(limit).unwrap(), 60.0);
            assert!(
                generator
                    .for_interval(interval)
                    .candidates
                    .iter()
                    .any(|c| c.ratio == ratio),
                "limit={limit} ratio={ratio}"
            );
        }
    }
}
