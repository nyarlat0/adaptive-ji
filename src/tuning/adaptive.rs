use std::{cmp::Ordering, collections::BTreeMap};

use super::{
    ActiveNote, Ratio, TunedNote, TuningStrategy, VoiceId,
    candidates::{Candidate, CandidateGenerator},
    rational::harmonic_complexity,
    roughness::{RoughnessConfig, Spectrum, SpectrumModel, fundamental_frequency, roughness},
};

pub const MAX_VOICES: usize = 15;

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
    pub max_deviation_cents: f64,
    pub candidates_per_note: usize,
    pub beam_width: usize,
    pub weights: ScoreWeights,
    pub roughness: RoughnessConfig,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            max_deviation_cents: 35.0,
            candidates_per_note: 8,
            beam_width: 2048,
            weights: ScoreWeights::default(),
            roughness: RoughnessConfig::default(),
        }
    }
}

impl OptimizerConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.roughness.validate()?;
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

pub fn movement_cost(new_offset: f64, previous_offset: Option<f64>) -> f64 {
    previous_offset.map_or(0.0, |old| ((new_offset - old) / 10.0).powi(2))
}

#[derive(Clone, Debug)]
pub struct AdaptiveSolution {
    pub reference_id: VoiceId,
    pub reference_note: u8,
    pub notes: Vec<TunedNote>,
    pub costs: Costs,
    pub score: f64,
}

#[derive(Clone, Copy)]
struct BeamEntry {
    choices: [u8; MAX_VOICES],
    costs: Costs,
    score: f64,
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
        config.validate()?;
        Ok(Self {
            config,
            generator: CandidateGenerator::new(config)?,
            spectrum_model: SpectrumModel::new(config.roughness)?,
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
        let candidates: Vec<Vec<Candidate>> = ordered
            .iter()
            .map(|note| {
                if note.id == reference.id {
                    vec![Candidate {
                        ratio: Ratio::ONE,
                        offset_cents: 0.0,
                    }]
                } else {
                    self.generator
                        .for_interval(i16::from(note.input_note) - i16::from(reference.input_note))
                        .candidates
                        .clone()
                }
            })
            .collect();

        // Cache all unary terms and all unordered pair matrices. No gcd/log2 in
        // the beam expansion; each edge's complexity has the same weight.
        let unary: Vec<Vec<Costs>> = ordered
            .iter()
            .zip(&candidates)
            .map(|(note, list)| {
                list.iter()
                    .map(|candidate| Costs {
                        harmonic: 0.0,
                        deviation: (candidate.offset_cents / 25.0).powi(2),
                        movement: movement_cost(
                            candidate.offset_cents,
                            self.previous.get(&note.id).copied(),
                        ),
                        roughness: 0.0,
                    })
                    .collect()
            })
            .collect();
        // Zero weight preserves the v0.2 search and avoids all spectrum/pair exp work.
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
        }];
        let mut expanded =
            Vec::with_capacity(self.config.beam_width * self.config.candidates_per_note);
        for (j, list) in candidates.iter().enumerate() {
            expanded.clear();
            for entry in &beam {
                for (choice, terms) in unary[j].iter().enumerate() {
                    let mut next = *entry;
                    next.choices[j] = choice as u8;
                    next.costs.deviation += terms.deviation;
                    next.costs.movement += terms.movement;
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
                        cents_offset: candidate.offset_cents,
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
            "ADAPTIVE score={:.6} reference={} voice={} cost: harmonic={:.6} deviation={:.6} movement={:.6} roughness={:.6}",
            solution.score,
            crate::voice::note_name(solution.reference_note),
            solution.reference_id,
            solution.costs.harmonic,
            solution.costs.deviation,
            solution.costs.movement,
            solution.costs.roughness
        );
        // Replace, rather than accumulate, history: released IDs never affect a new voice.
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
            })
            .collect()
    }

    fn ratios(tuned: &[TunedNote]) -> Vec<String> {
        tuned.iter().map(|note| note.ratio.to_string()).collect()
    }

    #[test]
    fn fresh_major_triad_is_four_five_six() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let tuned = optimizer.tune(&notes(&[60, 64, 67]));
        assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2"]);
        for (note, expected) in tuned.iter().zip([0.0, -13.686286, 1.955001]) {
            assert!((note.cents_offset - expected).abs() < 1e-6);
        }
        assert_eq!(optimizer.last_solution().unwrap().costs.movement, 0.0);
    }

    #[test]
    fn fresh_harmonic_seventh_is_four_five_six_seven() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let tuned = optimizer.tune(&notes(&[60, 64, 67, 70]));
        assert_eq!(ratios(&tuned), ["1/1", "5/4", "3/2", "7/4"]);
        assert!((tuned[3].cents_offset + 31.174094).abs() < 1e-6);
    }

    #[test]
    fn sequential_note_on_history_also_reaches_harmonic_seventh() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
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
        let mut optimizer = AdaptiveRationalOptimizer::default();
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
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let chord = [
            ActiveNote {
                id: 8,
                input_note: 60,
            },
            ActiveNote {
                id: 2,
                input_note: 64,
            },
            ActiveNote {
                id: 9,
                input_note: 67,
            },
        ];
        let tuned = optimizer.tune(&chord);
        assert_eq!(optimizer.last_solution().unwrap().reference_id, 2);
        assert_eq!(ratios(&tuned), ["4/5", "1/1", "6/5"]);
        assert_eq!(tuned[1].cents_offset, 0.0);
    }

    #[test]
    fn movement_terms_use_voice_history_and_absolute_offsets_after_reference_change() {
        assert_eq!(movement_cost(30.0, None), 0.0);
        assert_eq!(movement_cost(20.0, Some(0.0)), 4.0);
        let mut optimizer = AdaptiveRationalOptimizer::default();
        let chord = notes(&[60, 64, 67]);
        let old = optimizer.tune(&chord);
        let new = optimizer.tune(&chord[1..]);
        let expected: f64 = new
            .iter()
            .zip(&old[1..])
            .map(|(new, old)| ((new.cents_offset - old.cents_offset) / 10.0).powi(2))
            .sum();
        let solution = optimizer.last_solution().unwrap();
        assert_eq!(solution.reference_id, 1);
        assert_eq!(new[0].cents_offset, 0.0);
        assert!((solution.costs.movement - expected).abs() < 1e-10);
        assert!(solution.costs.movement > 0.0);
        assert_eq!(optimizer.previous.len(), 2);
        optimizer.tune(&[]);
        assert!(optimizer.previous.is_empty());
        assert!(optimizer.last_solution().is_none());
    }

    #[test]
    fn duplicate_notes_keep_independent_ids_and_new_voice_has_no_movement_cost() {
        let mut optimizer = AdaptiveRationalOptimizer::default();
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
        assert_eq!(first, second);
        assert_eq!(
            a.last_solution().unwrap().score,
            b.last_solution().unwrap().score
        );
        let held = a.tune(&chord);
        assert_eq!(held, first);
        assert_eq!(a.last_solution().unwrap().costs.movement, 0.0);
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
    fn movement_weight_prevents_an_unnecessary_large_jump_on_release() {
        let chord = notes(&[60, 64, 67]);
        let mut stable = AdaptiveRationalOptimizer::default();
        let mut mobile = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                movement: 0.0,
                ..ScoreWeights::default()
            },
            ..OptimizerConfig::default()
        })
        .unwrap();
        let old = stable.tune(&chord);
        assert_eq!(mobile.tune(&chord), old);
        let stable_new = stable.tune(&chord[1..]);
        let mobile_new = mobile.tune(&chord[1..]);
        assert_eq!(stable_new[1].ratio, Ratio::new(32, 27).unwrap());
        assert_eq!(mobile_new[1].ratio, Ratio::new(6, 5).unwrap());
        let stable_move = (stable_new[1].cents_offset - old[2].cents_offset).abs();
        let mobile_move = (mobile_new[1].cents_offset - old[2].cents_offset).abs();
        assert!(stable_move < mobile_move);
        assert!(
            stable.last_solution().unwrap().costs.movement
                < mobile.last_solution().unwrap().costs.movement
        );
    }

    #[test]
    fn beam_matches_exhaustive_oracle_for_a_small_chord_and_cost_terms() {
        let config = OptimizerConfig {
            candidates_per_note: 4,
            weights: ScoreWeights {
                roughness: 0.0,
                ..ScoreWeights::default()
            },
            ..OptimizerConfig::default()
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
            ..OptimizerConfig::default()
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
        assert_eq!(ratios(&after_release), ["1/1", "32/27"]);
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
            ..OptimizerConfig::default()
        })
        .unwrap();
        let mut rough = AdaptiveRationalOptimizer::new(OptimizerConfig {
            weights: ScoreWeights {
                roughness: 200.0,
                ..ScoreWeights::default()
            },
            ..OptimizerConfig::default()
        })
        .unwrap();
        baseline.tune(&chord);
        rough.tune(&chord);
        let old_choice = baseline.tune(&chord[1..]);
        let new_choice = rough.tune(&chord[1..]);
        assert_eq!(old_choice[1].ratio, Ratio::new(32, 27).unwrap());
        assert_eq!(new_choice[1].ratio, Ratio::new(6, 5).unwrap());
        let e_hz = fundamental_frequency(64, 0.0);
        let model = SpectrumModel::new(RoughnessConfig::default()).unwrap();
        let e = model.spectrum(e_hz).unwrap();
        let old_roughness = roughness(&e, &model.spectrum(e_hz * 32.0 / 27.0).unwrap());
        assert!(rough.last_solution().unwrap().costs.roughness < old_roughness);
        assert!(new_choice[1].cents_offset.abs() <= 35.0);
    }
}
