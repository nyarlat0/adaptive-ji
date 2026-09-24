use std::{cmp::Ordering, collections::BTreeMap};

use super::{
    ActiveNote, Ratio, TunedNote, TuningStrategy, VoiceId,
    candidates::{Candidate, CandidateGenerator},
    prime_limit::PrimeLimit,
    profile::{SpectrumPreset, SpectrumProfile},
    rational::harmonic_complexity,
    roughness::{RoughnessConfig, Spectrum, SpectrumModel, fundamental_frequency, roughness},
};

pub const MAX_VOICES: usize = 15;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AnchorMode {
    #[default]
    Floating,
    Fixed,
}

#[derive(Clone, Copy, Debug)]
pub struct ScoreWeights {
    pub harmonic: f64,
    pub deviation: f64,
    pub movement: f64,
    pub roughness: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            harmonic: 1.0,
            deviation: 0.35,
            movement: 1.5,
            roughness: 20.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OptimizerConfig {
    pub prime_limit: PrimeLimit,
    pub max_deviation_cents: f64,
    pub candidates_per_note: usize,
    pub beam_width: usize,
    pub weights: ScoreWeights,
    pub roughness: RoughnessConfig,
    pub anchor_mode: AnchorMode,
    pub anchor_max_cents: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            prime_limit: PrimeLimit::Seven,
            max_deviation_cents: 35.0,
            candidates_per_note: 8,
            beam_width: 2048,
            weights: ScoreWeights::default(),
            roughness: RoughnessConfig::default(),
            anchor_mode: AnchorMode::Floating,
            anchor_max_cents: 35.0,
        }
    }
}

impl OptimizerConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.roughness.validate()?;
        if !self.anchor_max_cents.is_finite() || !(0.0..=100.0).contains(&self.anchor_max_cents) {
            return Err("anchor-max must be finite and in 0..=100 cents".into());
        }
        if !self.max_deviation_cents.is_finite()
            || !(0.0..=100.0).contains(&self.max_deviation_cents)
        {
            return Err("max-deviation must be finite and in 0..=100 cents".into());
        }
        if !(1..=32).contains(&self.candidates_per_note) {
            return Err("candidates-per-note must be in 1..=32".into());
        }
        if !(1..=16384).contains(&self.beam_width) {
            return Err("beam-width must be in 1..=16384".into());
        }
        for (name, value) in [
            ("w-harmonic", self.weights.harmonic),
            ("w-deviation", self.weights.deviation),
            ("w-movement", self.weights.movement),
            ("w-roughness", self.weights.roughness),
        ] {
            if !value.is_finite() || !(0.0..=1e6).contains(&value) {
                return Err(format!("{name} must be finite and in 0..=1000000"));
            }
        }
        Ok(())
    }
}

/// Unweighted terms, kept separate from search and the weighted total.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Costs {
    pub harmonic: f64,
    pub deviation: f64,
    pub movement: f64,
    pub roughness: f64,
}

impl Costs {
    pub fn total(self, weights: ScoreWeights) -> f64 {
        weights.harmonic * self.harmonic
            + weights.deviation * self.deviation
            + weights.movement * self.movement
            + weights.roughness * self.roughness
    }
}

pub fn movement_cost(new_offset: f64, current_offset: Option<f64>) -> f64 {
    current_offset.map_or(0.0, |current| ((new_offset - current) / 10.0).powi(2))
}

#[derive(Clone, Debug)]
pub struct AdaptiveSolution {
    pub reference_id: VoiceId,
    pub reference_note: u8,
    pub notes: Vec<TunedNote>,
    pub costs: Costs,
    pub score: f64,
    pub anchor_cents: f64,
}

/// Q(a) = w_dev * sum((a + b)/25)^2
///      + w_move * sum_held((a + b - current)/10)^2.
/// Each candidate updates these sufficient statistics in O(1).
#[derive(Clone, Copy, Debug, Default)]
struct QuadraticStats {
    count: f64,
    held: f64,
    sum_base: f64,
    sum_base_sq: f64,
    sum_delta: f64,
    sum_delta_sq: f64,
}

impl QuadraticStats {
    fn add(&mut self, base: f64, current: Option<f64>) {
        self.count += 1.0;
        self.sum_base += base;
        self.sum_base_sq += base * base;
        if let Some(current) = current {
            let delta = base - current;
            self.held += 1.0;
            self.sum_delta += delta;
            self.sum_delta_sq += delta * delta;
        }
    }

    fn coefficients(self, weights: ScoreWeights) -> (f64, f64, f64) {
        let a = weights.deviation * self.count / 625.0 + weights.movement * self.held / 100.0;
        let b = 2.0
            * (weights.deviation * self.sum_base / 625.0
                + weights.movement * self.sum_delta / 100.0);
        let c = weights.deviation * self.sum_base_sq / 625.0
            + weights.movement * self.sum_delta_sq / 100.0;
        (a, b, c)
    }

    fn costs_at(self, anchor: f64) -> (f64, f64) {
        let deviation =
            (self.count * anchor * anchor + 2.0 * anchor * self.sum_base + self.sum_base_sq)
                / 625.0;
        let movement =
            (self.held * anchor * anchor + 2.0 * anchor * self.sum_delta + self.sum_delta_sq)
                / 100.0;
        (deviation.max(0.0), movement.max(0.0))
    }
}

#[derive(Clone, Copy, Debug)]
struct AnchorBounds {
    lower: f64,
    upper: f64,
}

impl AnchorBounds {
    fn restrict(self, base: f64, max_deviation: f64) -> Option<Self> {
        let lower = self.lower.max(-max_deviation - base);
        let upper = self.upper.min(max_deviation - base);
        (lower <= upper).then_some(Self { lower, upper })
    }

    fn minimize(self, stats: QuadraticStats, weights: ScoreWeights) -> f64 {
        let (a, b, _) = stats.coefficients(weights);
        let unconstrained = if a > 0.0 { -b / (2.0 * a) } else { 0.0 };
        unconstrained.clamp(self.lower, self.upper)
    }
}

#[derive(Clone, Copy)]
struct UnaryTerm {
    base: f64,
    current: Option<f64>,
    deviation: f64,
    movement: f64,
    fallback: bool,
}

#[derive(Clone, Copy)]
struct BeamEntry {
    choices: [u8; MAX_VOICES],
    costs: Costs,
    score: f64,
    stats: QuadraticStats,
    bounds: AnchorBounds,
    anchor: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PairCost {
    pub harmonic: f64,
    pub roughness: f64,
}

fn pair_cost(
    left: Candidate,
    right: Candidate,
    spectra: Option<(&Spectrum, &Spectrum)>,
) -> PairCost {
    PairCost {
        harmonic: harmonic_complexity(
            right
                .ratio
                .checked_div(left.ratio)
                .expect("bounded candidate pair"),
        ),
        roughness: spectra.map_or(0.0, |(a, b)| roughness(a, b)),
    }
}

fn compare_entries(a: &BeamEntry, b: &BeamEntry) -> Ordering {
    a.score
        .total_cmp(&b.score)
        .then_with(|| a.choices.cmp(&b.choices))
}

pub struct AdaptiveRationalOptimizer {
    config: OptimizerConfig,
    generator: CandidateGenerator,
    spectrum_model: SpectrumModel,
    /// Last targets are retained for diagnostics/backwards-compatible state
    /// inspection only. Movement is always based on ActiveNote snapshots.
    previous: BTreeMap<VoiceId, f64>,
    last_solution: Option<AdaptiveSolution>,
}

impl Default for AdaptiveRationalOptimizer {
    fn default() -> Self {
        Self::new(OptimizerConfig::default()).expect("valid default optimizer config")
    }
}

impl AdaptiveRationalOptimizer {
    pub fn new(config: OptimizerConfig) -> Result<Self, String> {
        let profile = SpectrumProfile::builtin(SpectrumPreset::Harmonic, config.roughness)?;
        Self::with_profile(config, profile)
    }

    pub fn with_profile(config: OptimizerConfig, profile: SpectrumProfile) -> Result<Self, String> {
        config.validate()?;
        let spectrum_model = SpectrumModel::from_profile(profile);
        // Reject finite-but-unrepresentable custom ratios at startup, not in a
        // live NoteOn callback. The candidate spectrum is computed pre-anchor.
        spectrum_model.spectrum(fundamental_frequency(0, -config.max_deviation_cents))?;
        spectrum_model.spectrum(fundamental_frequency(127, config.max_deviation_cents))?;
        Ok(Self {
            config,
            generator: CandidateGenerator::new(config)?,
            spectrum_model,
            previous: BTreeMap::new(),
            last_solution: None,
        })
    }

    pub fn last_solution(&self) -> Option<&AdaptiveSolution> {
        self.last_solution.as_ref()
    }

    fn search(&mut self, notes: &[ActiveNote]) -> AdaptiveSolution {
        // VoiceAllocator IDs increase at each NoteOn. They express age, not pitch.
        let reference = *notes
            .iter()
            .min_by_key(|note| note.id)
            .expect("nonempty chord");
        let mut ordered = notes.to_vec();
        ordered.sort_unstable_by_key(|note| (note.id != reference.id, note.input_note, note.id));
        let mut fallback_notes = Vec::with_capacity(ordered.len());
        let candidates: Vec<Vec<Candidate>> = ordered
            .iter()
            .map(|note| {
                if note.id == reference.id {
                    fallback_notes.push(false);
                    vec![Candidate {
                        ratio: Ratio::ONE,
                        offset_cents: 0.0,
                    }]
                } else {
                    let set = self
                        .generator
                        .for_interval(i16::from(note.input_note) - i16::from(reference.input_note));
                    fallback_notes.push(set.used_fallback);
                    set.candidates.clone()
                }
            })
            .collect();

        // Cache all unary terms and all unordered pair matrices. No gcd/log2 in
        // the beam expansion; each edge's complexity has the same weight.
        let unary: Vec<Vec<UnaryTerm>> = ordered
            .iter()
            .zip(&candidates)
            .enumerate()
            .map(|(index, (note, list))| {
                list.iter()
                    .map(|candidate| UnaryTerm {
                        base: candidate.offset_cents,
                        current: note.current_offset_cents,
                        deviation: (candidate.offset_cents / 25.0).powi(2),
                        movement: movement_cost(candidate.offset_cents, note.current_offset_cents),
                        fallback: fallback_notes[index],
                    })
                    .collect()
            })
            .collect();
        // Deliberate v0.4 approximation: pair roughness is evaluated before the
        // common anchor shift. Interval ratios are invariant, but absolute Hz are not.
        // Zero roughness weight avoids all spectrum/pair exp work.
        let spectra: Option<Vec<Vec<Spectrum>>> =
            (self.config.weights.roughness > 0.0).then(|| {
                ordered
                    .iter()
                    .zip(&candidates)
                    .map(|(note, list)| {
                        list.iter()
                            .map(|candidate| {
                                let frequency =
                                    fundamental_frequency(note.input_note, candidate.offset_cents);
                                self.spectrum_model
                                    .spectrum(frequency)
                                    .expect("finite MIDI candidate frequency")
                            })
                            .collect()
                    })
                    .collect()
            });
        let mut pairs: Vec<Vec<Vec<PairCost>>> = Vec::with_capacity(notes.len());
        for (j, list) in candidates.iter().enumerate() {
            let mut row = Vec::with_capacity(j);
            for (i, prior) in candidates[..j].iter().enumerate() {
                let mut matrix = Vec::with_capacity(list.len() * prior.len());
                for (right_index, &right) in list.iter().enumerate() {
                    for (left_index, &left) in prior.iter().enumerate() {
                        let pair_spectra = spectra
                            .as_ref()
                            .map(|spectra| (&spectra[i][left_index], &spectra[j][right_index]));
                        matrix.push(pair_cost(left, right, pair_spectra));
                    }
                }
                row.push(matrix);
            }
            pairs.push(row);
        }

        let mut beam = vec![BeamEntry {
            choices: [0; MAX_VOICES],
            costs: Costs::default(),
            score: 0.0,
            stats: QuadraticStats::default(),
            bounds: AnchorBounds {
                lower: -self.config.anchor_max_cents,
                upper: self.config.anchor_max_cents,
            },
            anchor: 0.0,
        }];
        let mut expanded =
            Vec::with_capacity(self.config.beam_width * self.config.candidates_per_note);
        for (j, list) in candidates.iter().enumerate() {
            expanded.clear();
            for entry in &beam {
                for (choice, terms) in unary[j].iter().enumerate() {
                    let mut next = *entry;
                    next.choices[j] = choice as u8;
                    match self.config.anchor_mode {
                        AnchorMode::Fixed => {
                            // Keep the v0.3 accumulation order and score path.
                            next.costs.deviation += terms.deviation;
                            next.costs.movement += terms.movement;
                        }
                        AnchorMode::Floating => {
                            // A fallback outside an impossibly narrow candidate
                            // window retains the pre-existing explicit exception.
                            if !terms.fallback {
                                let Some(bounds) = next
                                    .bounds
                                    .restrict(terms.base, self.config.max_deviation_cents + 1e-9)
                                else {
                                    continue;
                                };
                                next.bounds = bounds;
                            }
                            next.stats.add(terms.base, terms.current);
                            next.anchor = next.bounds.minimize(next.stats, self.config.weights);
                            (next.costs.deviation, next.costs.movement) =
                                next.stats.costs_at(next.anchor);
                        }
                    }
                    for (i, prior) in candidates[..j].iter().enumerate() {
                        let pair =
                            pairs[j][i][choice * prior.len() + usize::from(entry.choices[i])];
                        next.costs.harmonic += pair.harmonic;
                        next.costs.roughness += pair.roughness;
                    }
                    next.score = next.costs.total(self.config.weights);
                    expanded.push(next);
                }
            }
            debug_assert!(!list.is_empty());
            if expanded.len() > self.config.beam_width {
                expanded.select_nth_unstable_by(self.config.beam_width, compare_entries);
                expanded.truncate(self.config.beam_width);
            }
            expanded.sort_unstable_by(compare_entries);
            std::mem::swap(&mut beam, &mut expanded);
        }
        let best = beam[0];
        let selected: BTreeMap<_, _> = ordered
            .iter()
            .enumerate()
            .map(|(i, note)| {
                let candidate = candidates[i][usize::from(best.choices[i])];
                (
                    note.id,
                    TunedNote {
                        id: note.id,
                        ratio: candidate.ratio,
                        cents_offset: candidate.offset_cents + best.anchor,
                    },
                )
            })
            .collect();
        AdaptiveSolution {
            reference_id: reference.id,
            reference_note: reference.input_note,
            notes: notes.iter().map(|note| selected[&note.id]).collect(),
            costs: best.costs,
            score: best.score,
            anchor_cents: best.anchor,
        }
    }
}

impl TuningStrategy for AdaptiveRationalOptimizer {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote> {
        if active_notes.is_empty() {
            self.previous.clear();
            self.last_solution = None;
            return Vec::new();
        }
        assert!(
            active_notes.len() <= MAX_VOICES,
            "adaptive optimizer supports at most 15 voices"
        );
        let solution = self.search(active_notes);
        log::info!(
            "ADAPTIVE score={:.6} reference={} voice={} anchor={:+.3}c cost: harmonic={:.6} deviation={:.6} movement={:.6} roughness={:.6}",
            solution.score,
            crate::voice::note_name(solution.reference_note),
            solution.reference_id,
            solution.anchor_cents,
            solution.costs.harmonic,
            solution.costs.deviation,
            solution.costs.movement,
            solution.costs.roughness
        );
        if log::log_enabled!(log::Level::Debug) {
            for (active, tuned) in active_notes.iter().zip(&solution.notes) {
                let base = tuned.ratio.cents()
                    - f64::from(i16::from(active.input_note) - i16::from(solution.reference_note))
                        * 100.0;
                log::debug!(
                    "ANCHOR {} voice={} ratio={} base={:+.3}c anchor={:+.3}c actual={:+.3}c",
                    crate::voice::note_name(active.input_note),
                    active.id,
                    tuned.ratio,
                    base,
                    solution.anchor_cents,
                    tuned.cents_offset
                );
            }
        }
        // Keep the last solution for diagnostics/backwards-compatible state
        // inspection. It is deliberately not consulted for movement; the
        // caller supplies the audible snapshot on every tune call.
        self.previous = solution
            .notes
            .iter()
            .map(|note| (note.id, note.cents_offset))
            .collect();
        let result = solution.notes.clone();
        self.last_solution = Some(solution);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(pitches: &[u8]) -> Vec<ActiveNote> {
        pitches
            .iter()
            .enumerate()
            .map(|(id, &input_note)| ActiveNote {
                id: id as u64,
                input_note,
                current_offset_cents: None,
            })
            .collect()
    }

    fn ratios(tuned: &[TunedNote]) -> Vec<String> {
        tuned.iter().map(|note| note.ratio.to_string()).collect()
    }

    fn fixed_config() -> OptimizerConfig {
        OptimizerConfig {
            anchor_mode: AnchorMode::Fixed,
            ..OptimizerConfig::default()
        }
    }

    fn fixed_optimizer() -> AdaptiveRationalOptimizer {
        AdaptiveRationalOptimizer::new(fixed_config()).unwrap()
    }

    #[test]
    fn prime_limit_five_keeps_triad_but_forbids_septimal_seventh() {
        let mut config = fixed_config();
        config.prime_limit = PrimeLimit::Five;
        let mut optimizer = AdaptiveRationalOptimizer::new(config).unwrap();
        let triad = optimizer.tune(&notes(&[60, 64, 67]));
        assert_eq!(ratios(&triad), ["1/1", "5/4", "3/2"]);
        let seventh = optimizer.tune(&notes(&[60, 64, 67, 70]));
        assert!(seventh.iter().all(|note| {
            crate::tuning::rational::prime_factorization_without_two(note.ratio)
                .keys()
                .all(|prime| *prime <= 5)
        }));
        assert!(
            seventh
                .iter()
                .all(|note| note.ratio != Ratio::new(7, 4).unwrap())
        );
    }

    #[test]
    fn prime_limit_eleven_can_select_undecimal_tritone_when_complexity_dominates() {
        let mut config = fixed_config();
        config.prime_limit = PrimeLimit::Eleven;
        config.max_deviation_cents = 60.0;
        config.weights.harmonic = 10.0;
        config.weights.deviation = 0.0;
        config.weights.movement = 0.0;
        config.weights.roughness = 0.0;
        let mut optimizer = AdaptiveRationalOptimizer::new(config).unwrap();
        let tuned = optimizer.tune(&notes(&[60, 66]));
        assert!(
            tuned
                .iter()
                .any(|note| note.ratio == Ratio::new(11, 8).unwrap()),
            "{tuned:?}"
        );
        assert!((tuned[1].cents_offset + 48.6820576).abs() < 1e-5);
    }

    #[test]
    fn spectral_profiles_can_change_optimizer_decision_at_high_roughness_weight() {
        let mut config = fixed_config();
        config.weights.roughness = 2500.0;
        let mut sine = AdaptiveRationalOptimizer::with_profile(
            config,
            SpectrumProfile::builtin(SpectrumPreset::Sine, config.roughness).unwrap(),
        )
        .unwrap();
        let mut bright = AdaptiveRationalOptimizer::with_profile(
            config,
            SpectrumProfile::builtin(SpectrumPreset::Bright, config.roughness).unwrap(),
        )
        .unwrap();
        let pitches = notes(&[60, 67]);
        let sine_ratios = ratios(&sine.tune(&pitches));
        let bright_ratios = ratios(&bright.tune(&pitches));
        assert_ne!(
            sine_ratios, bright_ratios,
            "sine={sine_ratios:?} bright={bright_ratios:?}"
        );
        assert_eq!(bright_ratios[1], "3/2");
    }

    #[test]
    fn zero_roughness_weight_makes_profile_choice_irrelevant_and_all_presets_are_deterministic() {
        let presets = [
            SpectrumPreset::Harmonic,
            SpectrumPreset::Sine,
            SpectrumPreset::Saw,
            SpectrumPreset::Square,
            SpectrumPreset::Soft,
            SpectrumPreset::Bright,
        ];
        let pitches = notes(&[60, 64, 67, 70]);
        let mut config = OptimizerConfig::default();
        config.weights.roughness = 0.0;
        let baseline = AdaptiveRationalOptimizer::new(config)
            .unwrap()
            .tune(&pitches);
        for preset in presets {
            let profile = SpectrumProfile::builtin(preset, config.roughness).unwrap();
            let first = AdaptiveRationalOptimizer::with_profile(config, profile.clone())
                .unwrap()
                .tune(&pitches);
            let second = AdaptiveRationalOptimizer::with_profile(config, profile)
                .unwrap()
                .tune(&pitches);
            assert_eq!(first, baseline, "preset={preset:?}");
            assert_eq!(second, first, "preset={preset:?}");
        }
    }

    #[test]
    fn positive_roughness_is_deterministic_for_every_preset_and_rejects_unrepresentable_profile() {
        let config = OptimizerConfig::default();
        let pitches = notes(&[60, 64, 67, 70]);
        for preset in [
            SpectrumPreset::Harmonic,
            SpectrumPreset::Sine,
            SpectrumPreset::Saw,
            SpectrumPreset::Square,
            SpectrumPreset::Soft,
            SpectrumPreset::Bright,
        ] {
            let profile = SpectrumProfile::builtin(preset, config.roughness).unwrap();
            let mut first =
                AdaptiveRationalOptimizer::with_profile(config, profile.clone()).unwrap();
            let mut second = AdaptiveRationalOptimizer::with_profile(config, profile).unwrap();
            assert_eq!(
                first.tune(&pitches),
                second.tune(&pitches),
                "preset={preset:?}"
            );
        }
        let too_high = SpectrumProfile::new(vec![crate::tuning::profile::ProfilePartial {
            frequency_ratio: f64::MAX,
            amplitude: 1.0,
        }])
        .unwrap();
        assert!(AdaptiveRationalOptimizer::with_profile(config, too_high).is_err());
    }

    #[test]
    fn fresh_major_triad_is_four_five_six() {
        let mut optimizer = fixed_optimizer();
        let tuned = optimizer.tune(&notes(&[60, 64, 67]));
        assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2"]);
        for (note, expected) in tuned.iter().zip([0.0, -13.686286, 1.955001]) {
            assert!((note.cents_offset - expected).abs() < 1e-6);
        }
        assert_eq!(optimizer.last_solution().unwrap().costs.movement, 0.0);
    }

    #[test]
    fn fresh_harmonic_seventh_is_four_five_six_seven() {
        let mut optimizer = fixed_optimizer();
        let tuned = optimizer.tune(&notes(&[60, 64, 67, 70]));
        assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2", "7/4"]);
        assert!((tuned[3].cents_offset + 31.174094).abs() < 1e-6);
    }

    #[test]
    fn sequential_note_on_history_also_reaches_harmonic_seventh() {
        let mut optimizer = fixed_optimizer();
        let chord = notes(&[60, 64, 67, 70]);
        for count in 1..=4 {
            let tuned = optimizer.tune(&chord[..count]);
            assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2", "7/4"][..count]);
        }
        let solution = optimizer.last_solution().unwrap();
        assert_eq!(solution.costs.movement, 0.0);
        assert_eq!(ratios(&optimizer.tune(&chord[..3])), ["1/1", "5/4", "3/2"]);
    }

    #[test]
    fn inversion_scores_all_pairs_without_treating_reference_as_root() {
        let mut optimizer = fixed_optimizer();
        let tuned = optimizer.tune(&notes(&[64, 67, 72]));
        let g_over_e = tuned[1].ratio.checked_div(tuned[0].ratio).unwrap();
        let c_over_e = tuned[2].ratio.checked_div(tuned[0].ratio).unwrap();
        let c_over_g = tuned[2].ratio.checked_div(tuned[1].ratio).unwrap();
        assert_eq!(g_over_e, Ratio::new(6, 5).unwrap());
        assert_eq!(c_over_e, Ratio::new(8, 5).unwrap());
        assert_eq!(c_over_g, Ratio::new(4, 3).unwrap());
        assert!(tuned[2].cents_offset.abs() > 10.0);
        let all_pairs = harmonic_complexity(g_over_e)
            + harmonic_complexity(c_over_e)
            + harmonic_complexity(c_over_g);
        assert!((optimizer.last_solution().unwrap().costs.harmonic - all_pairs).abs() < 1e-10);
    }

    #[test]
    fn older_higher_voice_is_reference_even_if_input_is_unsorted() {
        let mut optimizer = fixed_optimizer();
        let chord = [
            ActiveNote {
                id: 8,
                input_note: 60,
                current_offset_cents: None,
            },
            ActiveNote {
                id: 2,
                input_note: 64,
                current_offset_cents: None,
            },
            ActiveNote {
                id: 9,
                input_note: 67,
                current_offset_cents: None,
            },
        ];
        let tuned = optimizer.tune(&chord);
        assert_eq!(optimizer.last_solution().unwrap().reference_id, 2);
        assert_eq!(ratios(&tuned), ["4/5", "1/1", "6/5"]);
        assert_eq!(tuned[1].cents_offset, 0.0);
    }

    #[test]
    fn movement_terms_use_explicit_current_offsets_after_reference_change() {
        assert_eq!(movement_cost(30.0, None), 0.0);
        assert_eq!(movement_cost(20.0, Some(0.0)), 4.0);
        let mut optimizer = fixed_optimizer();
        let chord = notes(&[60, 64, 67]);
        let old = optimizer.tune(&chord);
        let new_notes: Vec<_> = chord[1..]
            .iter()
            .zip(&old[1..])
            .map(|(note, tuned)| ActiveNote {
                current_offset_cents: Some(tuned.cents_offset),
                ..*note
            })
            .collect();
        let new = optimizer.tune(&new_notes);
        let expected: f64 = new
            .iter()
            .zip(&old[1..])
            .map(|(new, old)| ((new.cents_offset - old.cents_offset) / 10.0).powi(2))
            .sum();
        let solution = optimizer.last_solution().unwrap();
        assert_eq!(solution.reference_id, 1);
        assert!(new[0].cents_offset.abs() < 1.0);
        assert!((solution.costs.movement - expected).abs() < 1e-10);
        assert!(solution.costs.movement > 0.0);
        assert_eq!(optimizer.previous.len(), 2);
        optimizer.tune(&[]);
        assert!(optimizer.previous.is_empty());
        assert!(optimizer.last_solution().is_none());
    }

    #[test]
    fn duplicate_notes_keep_independent_ids_and_new_voice_has_no_movement_cost() {
        let mut optimizer = fixed_optimizer();
        let chord = notes(&[60, 64, 64, 67]);
        optimizer.tune(&chord[..2]);
        let tuned = optimizer.tune(&chord);
        assert_eq!(tuned.len(), 4);
        assert_ne!(tuned[1].id, tuned[2].id);
        assert_eq!(tuned[1].ratio, tuned[2].ratio);
        assert_eq!(optimizer.last_solution().unwrap().costs.movement, 0.0);
    }

    #[test]
    fn deterministic_across_instances_and_input_permutations() {
        let chord = notes(&[64, 55, 60, 70, 74]);
        let mut a = AdaptiveRationalOptimizer::default();
        let mut b = AdaptiveRationalOptimizer::default();
        let first = a.tune(&chord);
        let mut reversed = chord.clone();
        reversed.reverse();
        let mut second = b.tune(&reversed);
        second.sort_by_key(|note| note.id);
        for (left, right) in first.iter().zip(&second) {
            assert_eq!(left.id, right.id);
            assert_eq!(left.ratio, right.ratio);
            assert!((left.cents_offset - right.cents_offset).abs() < 1e-10);
        }
        assert!(
            (a.last_solution().unwrap().score - b.last_solution().unwrap().score).abs() < 1e-10
        );
        let held = a.tune(&chord);
        assert_eq!(ratios(&held), ratios(&first));
        assert!(a.last_solution().unwrap().costs.movement < 1e-8);
    }

    #[test]
    fn fifteen_voices_and_full_midi_span() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let mut second = AdaptiveRationalOptimizer::default();
        let chord = notes(&[
            64, 0, 12, 24, 36, 48, 60, 72, 84, 96, 108, 120, 125, 126, 127,
        ]);
        let tuned = optimizer.tune(&chord);
        assert_eq!(tuned, second.tune(&chord));
        assert_eq!(
            optimizer.last_solution().unwrap().score,
            second.last_solution().unwrap().score
        );
        assert_eq!(tuned.len(), 15);
        assert!(
            tuned
                .iter()
                .all(|note| note.cents_offset.is_finite() && note.cents_offset.abs() <= 35.0)
        );
        assert!(optimizer.last_solution().unwrap().score.is_finite());
    }

    #[test]
    fn empty_candidate_window_still_produces_a_solution() {
        let mut optimizer = AdaptiveRationalOptimizer::new(OptimizerConfig {
            max_deviation_cents: 0.0,
            ..OptimizerConfig::default()
        })
        .unwrap();
        let tuned = optimizer.tune(&notes(&[60, 61, 67]));
        assert_eq!(tuned.len(), 3);
        assert!(tuned.iter().all(|note| note.cents_offset.is_finite()));
    }

    #[test]
    fn validates_search_limits_and_weights() {
        for config in [
            OptimizerConfig {
                beam_width: 0,
                ..OptimizerConfig::default()
            },
            OptimizerConfig {
                candidates_per_note: 0,
                ..OptimizerConfig::default()
            },
            OptimizerConfig {
                max_deviation_cents: f64::NAN,
                ..OptimizerConfig::default()
            },
            OptimizerConfig {
                weights: ScoreWeights {
                    movement: -1.0,
                    ..ScoreWeights::default()
                },
                ..OptimizerConfig::default()
            },
        ] {
            assert!(AdaptiveRationalOptimizer::new(config).is_err());
        }
    }

    #[test]
    fn movement_weight_is_based_on_explicit_audible_position() {
        let chord = notes(&[60, 64, 67]);
        let mut stable = fixed_optimizer();
        let mut mobile = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                movement: 0.0,
                ..ScoreWeights::default()
            },
            ..fixed_config()
        })
        .unwrap();
        let old = stable.tune(&chord);
        assert_eq!(mobile.tune(&chord), old);
        let stable_new = stable.tune(&chord[1..]);
        let mobile_new = mobile.tune(&chord[1..]);
        assert_eq!(stable_new[1].ratio, Ratio::new(6, 5).unwrap());
        assert_eq!(mobile_new[1].ratio, Ratio::new(6, 5).unwrap());
        let stable_move = (stable_new[1].cents_offset - old[2].cents_offset).abs();
        let mobile_move = (mobile_new[1].cents_offset - old[2].cents_offset).abs();
        assert!((stable_move - mobile_move).abs() < 1e-9);
        assert!(stable.last_solution().unwrap().costs.movement.is_finite());
    }

    #[test]
    fn beam_matches_exhaustive_oracle_for_a_small_chord_and_cost_terms() {
        let config = OptimizerConfig {
            candidates_per_note: 4,
            weights: ScoreWeights {
                roughness: 0.0,
                ..ScoreWeights::default()
            },
            ..fixed_config()
        };
        let mut generator = CandidateGenerator::new(config).unwrap();
        let lists: Vec<_> = [4, 7, 10]
            .iter()
            .map(|&d| generator.for_interval(d).candidates.clone())
            .collect();
        let mut minimum = f64::INFINITY;
        for a in &lists[0] {
            for b in &lists[1] {
                for c in &lists[2] {
                    let assignment = [
                        Candidate {
                            ratio: Ratio::ONE,
                            offset_cents: 0.0,
                        },
                        *a,
                        *b,
                        *c,
                    ];
                    let mut costs = Costs::default();
                    for (i, candidate) in assignment.iter().enumerate() {
                        costs.deviation += (candidate.offset_cents / 25.0).powi(2);
                        for prior in &assignment[..i] {
                            costs.harmonic += harmonic_complexity(
                                candidate.ratio.checked_div(prior.ratio).unwrap(),
                            );
                        }
                    }
                    minimum = minimum.min(costs.total(config.weights));
                }
            }
        }
        let mut optimizer = AdaptiveRationalOptimizer::new(config).unwrap();
        optimizer.tune(&notes(&[60, 64, 67, 70]));
        let solution = optimizer.last_solution().unwrap();
        assert!((solution.score - minimum).abs() < 1e-10);
        assert_eq!(solution.score, solution.costs.total(config.weights));
        assert_eq!(solution.costs.roughness, 0.0);
    }

    #[test]
    fn pair_cost_precomputation_is_deterministic_and_uses_actual_frequencies() {
        let model = SpectrumModel::new(RoughnessConfig::default()).unwrap();
        let left = Candidate {
            ratio: Ratio::ONE,
            offset_cents: 0.0,
        };
        let right = Candidate {
            ratio: Ratio::new(6, 5).unwrap(),
            offset_cents: Ratio::new(6, 5).unwrap().cents() - 300.0,
        };
        let a = model
            .spectrum(fundamental_frequency(64, left.offset_cents))
            .unwrap();
        let b = model
            .spectrum(fundamental_frequency(67, right.offset_cents))
            .unwrap();
        let first = pair_cost(left, right, Some((&a, &b)));
        let second = pair_cost(left, right, Some((&a, &b)));
        assert_eq!(first, second);
        assert!((first.harmonic - harmonic_complexity(right.ratio)).abs() < 1e-12);
        assert!((first.roughness - roughness(&a, &b)).abs() < 1e-12);
        assert!(first.roughness.is_finite() && first.roughness > 0.0);
        assert_eq!(pair_cost(left, right, None).roughness, 0.0);
    }

    #[test]
    fn release_scenario_reports_lower_roughness_for_six_fifths() {
        let model = SpectrumModel::new(RoughnessConfig::default()).unwrap();
        let e = model.spectrum(fundamental_frequency(64, 0.0)).unwrap();
        let f_e = fundamental_frequency(64, 0.0);
        let six_fifths = roughness(&e, &model.spectrum(f_e * 6.0 / 5.0).unwrap());
        let thirty_two_twenty_seven = roughness(&e, &model.spectrum(f_e * 32.0 / 27.0).unwrap());
        eprintln!("E4-G4 roughness: 6/5={six_fifths:.6}, 32/27={thirty_two_twenty_seven:.6}");
        assert!(six_fifths < thirty_two_twenty_seven);
    }

    #[test]
    fn zero_roughness_weight_preserves_v02_decisions_and_costs() {
        let mut optimizer = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                roughness: 0.0,
                ..ScoreWeights::default()
            },
            ..fixed_config()
        })
        .unwrap();
        let chord = notes(&[60, 64, 67, 70]);
        for count in 1..=4 {
            assert_eq!(
                ratios(&optimizer.tune(&chord[..count])),
                ["1/1", "5/4", "3/2", "7/4"][..count]
            );
            assert_eq!(optimizer.last_solution().unwrap().costs.roughness, 0.0);
        }
        let after_release = optimizer.tune(&chord[1..3]);
        assert_eq!(ratios(&after_release), ["1/1", "6/5"]);
        let costs = optimizer.last_solution().unwrap().costs;
        let weights = optimizer.config.weights;
        assert_eq!(
            optimizer.last_solution().unwrap().score,
            weights.harmonic * costs.harmonic
                + weights.deviation * costs.deviation
                + weights.movement * costs.movement
        );
    }

    #[test]
    fn roughness_weight_can_change_choice_between_admissible_ratios() {
        let chord = notes(&[60, 64, 67]);
        let mut baseline = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                roughness: 0.0,
                ..ScoreWeights::default()
            },
            ..fixed_config()
        })
        .unwrap();
        let mut rough = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                roughness: 200.0,
                ..ScoreWeights::default()
            },
            ..fixed_config()
        })
        .unwrap();
        baseline.tune(&chord);
        rough.tune(&chord);
        let old_choice = baseline.tune(&chord[1..]);
        let new_choice = rough.tune(&chord[1..]);
        assert_eq!(old_choice[1].ratio, Ratio::new(6, 5).unwrap());
        assert_eq!(new_choice[1].ratio, Ratio::new(6, 5).unwrap());
        let e_hz = fundamental_frequency(64, 0.0);
        let model = SpectrumModel::new(RoughnessConfig::default()).unwrap();
        let e = model.spectrum(e_hz).unwrap();
        let old_roughness = roughness(&e, &model.spectrum(e_hz * 32.0 / 27.0).unwrap());
        assert!(rough.last_solution().unwrap().costs.roughness < old_roughness);
        assert!(new_choice[1].cents_offset.abs() <= 35.0);
    }

    #[test]
    fn analytic_anchor_agrees_with_dense_grid_and_respects_bounds() {
        let weights = ScoreWeights::default();
        let mut stats = QuadraticStats::default();
        for (base, old) in [
            (0.0, Some(2.0)),
            (15.641287, Some(1.955001)),
            (-31.174094, None),
        ] {
            stats.add(base, old);
        }
        let bounds = AnchorBounds {
            lower: -35.0,
            upper: 35.0,
        }
        .restrict(15.641287, 35.0)
        .unwrap()
        .restrict(-31.174094, 35.0)
        .unwrap();
        let anchor = bounds.minimize(stats, weights);
        let (a, b, c) = stats.coefficients(weights);
        let score = |point: f64| a * point * point + b * point + c;
        let mut grid_best = f64::INFINITY;
        let mut point = bounds.lower;
        while point <= bounds.upper {
            grid_best = grid_best.min(score(point));
            point += 0.002;
        }
        grid_best = grid_best.min(score(bounds.upper));
        assert!(score(anchor) <= grid_best + 1e-7);
        assert!(
            (weights.deviation * stats.costs_at(anchor).0
                + weights.movement * stats.costs_at(anchor).1
                - score(anchor))
            .abs()
                < 1e-10
        );
        assert!(bounds.lower <= anchor && anchor <= bounds.upper);
    }

    #[test]
    fn anchor_clamps_and_zero_quadratic_prefers_zero() {
        let mut stats = QuadraticStats::default();
        stats.add(0.0, Some(80.0));
        let weights = ScoreWeights::default();
        assert_eq!(
            AnchorBounds {
                lower: -5.0,
                upper: 5.0
            }
            .minimize(stats, weights),
            5.0
        );
        stats = QuadraticStats::default();
        stats.add(0.0, Some(-80.0));
        assert_eq!(
            AnchorBounds {
                lower: -5.0,
                upper: 5.0
            }
            .minimize(stats, weights),
            -5.0
        );
        let flat = ScoreWeights {
            deviation: 0.0,
            movement: 0.0,
            ..weights
        };
        assert_eq!(
            AnchorBounds {
                lower: -5.0,
                upper: 5.0
            }
            .minimize(stats, flat),
            0.0
        );
        assert_eq!(
            AnchorBounds {
                lower: 2.0,
                upper: 5.0
            }
            .minimize(stats, flat),
            2.0
        );
        assert_eq!(
            AnchorBounds {
                lower: -5.0,
                upper: -2.0
            }
            .minimize(stats, flat),
            -2.0
        );
        assert!(
            AnchorBounds {
                lower: -1.0,
                upper: 1.0
            }
            .restrict(5.0, 1.0)
            .is_none()
        );
    }

    #[test]
    fn floating_reference_can_have_nonzero_absolute_pitch_without_changing_ratios() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let tuned = optimizer.tune(&notes(&[60, 64, 67]));
        assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2"]);
        let anchor = optimizer.last_solution().unwrap().anchor_cents;
        assert!((tuned[0].cents_offset - anchor).abs() < 1e-12);
        assert!(anchor.abs() > 0.01);
        for (note, base) in tuned.iter().zip([
            0.0,
            Ratio::new(5, 4).unwrap().cents() - 400.0,
            Ratio::new(3, 2).unwrap().cents() - 700.0,
        ]) {
            assert!((note.cents_offset - base - anchor).abs() < 1e-10);
        }
        assert!(
            ((tuned[2].cents_offset - tuned[1].cents_offset)
                - (Ratio::new(6, 5).unwrap().cents() - 300.0))
                .abs()
                < 1e-10
        );
    }

    #[test]
    fn floating_release_of_c_preserves_e_and_g_absolute_offsets() {
        let chord = notes(&[60, 64, 67]);
        let mut optimizer = AdaptiveRationalOptimizer::default();
        for count in 1..=3 {
            optimizer.tune(&chord[..count]);
        }
        let before = optimizer.last_solution().unwrap().notes.clone();
        let after_notes: Vec<_> = chord[1..]
            .iter()
            .zip(&before[1..])
            .map(|(note, tuned)| ActiveNote {
                current_offset_cents: Some(tuned.cents_offset),
                ..*note
            })
            .collect();
        let after = optimizer.tune(&after_notes);
        let solution = optimizer.last_solution().unwrap();
        assert_eq!(solution.reference_id, 1);
        assert_eq!(ratios(&after), ["1/1", "6/5"]);
        assert!((solution.anchor_cents - before[1].cents_offset).abs() < 2.0);
        for (old, new) in before[1..].iter().zip(&after) {
            assert!((old.cents_offset - new.cents_offset).abs() < 2.0);
        }
        assert!(solution.costs.movement < 0.08);
        assert!((after[0].cents_offset - solution.anchor_cents).abs() < 1e-12);
    }

    #[test]
    fn floating_rebases_harmonic_seventh_without_pitch_jump() {
        let chord = notes(&[60, 64, 67, 70]);
        let mut optimizer = AdaptiveRationalOptimizer::default();
        for count in 1..=4 {
            optimizer.tune(&chord[..count]);
        }
        let before = optimizer.last_solution().unwrap().notes.clone();
        assert_eq!(ratios(&before), ["1/1", "5/4", "3/2", "7/4"]);
        let after_notes: Vec<_> = chord[1..]
            .iter()
            .zip(&before[1..])
            .map(|(note, tuned)| ActiveNote {
                current_offset_cents: Some(tuned.cents_offset),
                ..*note
            })
            .collect();
        let after = optimizer.tune(&after_notes);
        assert_eq!(ratios(&after), ["1/1", "6/5", "7/5"]);
        for (old, new) in before[1..].iter().zip(&after) {
            assert!((old.cents_offset - new.cents_offset).abs() < 2.0);
        }
        assert!(optimizer.last_solution().unwrap().costs.movement < 0.12);
    }

    #[test]
    fn fixed_mode_reproduces_v03_and_new_session_does_not_drift() {
        let chord = notes(&[60, 64, 67]);
        let mut fixed = fixed_optimizer();
        for count in 1..=3 {
            fixed.tune(&chord[..count]);
        }
        let result = fixed.tune(&chord[1..]);
        assert_eq!(ratios(&result), ["1/1", "6/5"]);
        assert_eq!(result[0].cents_offset, 0.0);
        assert_eq!(fixed.last_solution().unwrap().anchor_cents, 0.0);

        let mut floating = AdaptiveRationalOptimizer::default();
        let seventh = notes(&[60, 64, 67, 70]);
        for count in 1..=4 {
            floating.tune(&seventh[..count]);
        }
        for start in 1..=4 {
            floating.tune(&seventh[start..]);
        }
        assert!(floating.last_solution().is_none());
        let fresh = floating.tune(&[ActiveNote {
            id: 10,
            input_note: 69,
            current_offset_cents: None,
        }]);
        assert_eq!(fresh[0].cents_offset, 0.0);
        assert_eq!(floating.last_solution().unwrap().anchor_cents, 0.0);
    }

    #[test]
    fn floating_duplicate_notes_remain_distinct_voices() {
        let chord = notes(&[60, 64, 64, 67]);
        let mut optimizer = AdaptiveRationalOptimizer::default();
        for count in 1..=4 {
            optimizer.tune(&chord[..count]);
        }
        let before = optimizer.last_solution().unwrap().notes.clone();
        assert_eq!(before[1].ratio, before[2].ratio);
        assert_ne!(before[1].id, before[2].id);
        let after = optimizer.tune(&chord[1..]);
        assert_eq!(after.len(), 3);
        assert_ne!(after[0].id, after[1].id);
        assert_eq!(after[0].ratio, after[1].ratio);
        assert!((after[0].cents_offset - after[1].cents_offset).abs() < 1e-10);
        assert_eq!(optimizer.previous.len(), 3);
    }

    #[test]
    fn floating_output_stays_within_deviation_and_anchor_limits() {
        let config = OptimizerConfig {
            max_deviation_cents: 20.0,
            anchor_max_cents: 5.0,
            ..OptimizerConfig::default()
        };
        let mut optimizer = AdaptiveRationalOptimizer::new(config).unwrap();
        let chord = notes(&[60, 64, 67, 72]);
        for count in 1..=4 {
            optimizer.tune(&chord[..count]);
        }
        let result = optimizer.last_solution().unwrap();
        assert!(result.anchor_cents.is_finite() && result.anchor_cents.abs() <= 5.0 + 1e-9);
        assert!(result.notes.iter().all(|note| note.cents_offset.is_finite()
            && note.cents_offset.abs() <= 20.0 + 1e-8));
    }
}
