//! Experimental, phrase-local memory of octave-equivalent rational relations.

use std::collections::BTreeMap;

use super::{ActiveNote, Ratio, TunedNote, VoiceId, rational::prime_factorization_without_two};

pub const MAX_TRACES_PER_PAIR: usize = 4;
const PRUNE_EPSILON: f64 = 1e-4;
const REINFORCEMENT: f64 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MemoryMode {
    #[default]
    Off,
    Relations,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationFingerprint(pub Vec<(u32, i32)>);

pub type PitchClassPair = (u8, u8);

#[derive(Clone, Debug, PartialEq)]
pub struct RelationTrace {
    pub fingerprint: RelationFingerprint,
    pub strength: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RelationMemory {
    map: BTreeMap<PitchClassPair, Vec<RelationTrace>>,
}

pub fn fingerprint(ratio: Ratio) -> RelationFingerprint {
    RelationFingerprint(
        prime_factorization_without_two(ratio)
            .into_iter()
            .map(|(prime, exponent)| (prime as u32, exponent))
            .collect(),
    )
}

pub fn relation_distance(left: &RelationFingerprint, right: &RelationFingerprint) -> f64 {
    let mut index_left = 0;
    let mut index_right = 0;
    let mut distance = 0.0;
    while index_left < left.0.len() || index_right < right.0.len() {
        match (left.0.get(index_left), right.0.get(index_right)) {
            (Some(&(lp, le)), Some(&(rp, re))) if lp == rp => {
                distance += f64::from(lp).log2() * f64::from((le - re).abs());
                index_left += 1;
                index_right += 1;
            }
            (Some(&(lp, le)), Some(&(rp, _))) if lp < rp => {
                distance += f64::from(lp).log2() * f64::from(le.abs());
                index_left += 1;
            }
            (Some(&(lp, _)), Some(&(rp, re))) if rp < lp => {
                distance += f64::from(rp).log2() * f64::from(re.abs());
                index_right += 1;
            }
            (Some(&(lp, le)), None) => {
                distance += f64::from(lp).log2() * f64::from(le.abs());
                index_left += 1;
            }
            (None, Some(&(rp, re))) => {
                distance += f64::from(rp).log2() * f64::from(re.abs());
                index_right += 1;
            }
            _ => unreachable!("fingerprints are sorted and unique"),
        }
    }
    distance
}

pub fn pitch_class_pair(a: u8, b: u8) -> PitchClassPair {
    let a = a % 12;
    let b = b % 12;
    (a.min(b), a.max(b))
}

pub fn pair_relation(
    left_pc: u8,
    left_ratio: Ratio,
    right_pc: u8,
    right_ratio: Ratio,
) -> (PitchClassPair, RelationFingerprint) {
    if left_pc <= right_pc {
        (
            pitch_class_pair(left_pc, right_pc),
            fingerprint(right_ratio.checked_div(left_ratio).expect("valid ratio")),
        )
    } else {
        (
            pitch_class_pair(left_pc, right_pc),
            fingerprint(left_ratio.checked_div(right_ratio).expect("valid ratio")),
        )
    }
}

impl RelationMemory {
    pub fn entries(&self) -> &BTreeMap<PitchClassPair, Vec<RelationTrace>> {
        &self.map
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn traces(&self, pair: PitchClassPair) -> &[RelationTrace] {
        self.map.get(&pair).map_or(&[], Vec::as_slice)
    }

    pub fn pair_cost(&self, pair: PitchClassPair, relation: &RelationFingerprint) -> f64 {
        let traces = self.traces(pair);
        if traces.is_empty() {
            return 0.0;
        }
        let total_strength: f64 = traces.iter().map(|trace| trace.strength).sum();
        if total_strength <= 0.0 {
            return 0.0;
        }
        let weighted = traces
            .iter()
            .map(|trace| trace.strength * relation_distance(relation, &trace.fingerprint).powi(2))
            .sum::<f64>()
            / total_strength;
        total_strength.min(1.0) * weighted
    }

    pub fn decay(&mut self, factor: f64) {
        for traces in self.map.values_mut() {
            for trace in traces.iter_mut() {
                trace.strength *= factor;
            }
            traces.retain(|trace| trace.strength >= PRUNE_EPSILON);
        }
        self.map.retain(|_, traces| !traces.is_empty());
    }

    pub fn record(&mut self, notes: &[ActiveNote], tuned: &[TunedNote], decay: f64) {
        self.decay(decay);
        let by_id: BTreeMap<VoiceId, &TunedNote> =
            tuned.iter().map(|note| (note.id, note)).collect();
        let mut unique = std::collections::BTreeSet::<(PitchClassPair, RelationFingerprint)>::new();
        for (index, left) in notes.iter().enumerate() {
            for right in &notes[index + 1..] {
                let (Some(left_tuned), Some(right_tuned)) =
                    (by_id.get(&left.id), by_id.get(&right.id))
                else {
                    continue;
                };
                let (pair, relation) = pair_relation(
                    left.input_note % 12,
                    left_tuned.ratio,
                    right.input_note % 12,
                    right_tuned.ratio,
                );
                unique.insert((pair, relation));
            }
        }
        for (pair, relation) in unique {
            let traces = self.map.entry(pair).or_default();
            if let Some(trace) = traces
                .iter_mut()
                .find(|trace| trace.fingerprint == relation)
            {
                trace.strength += (1.0 - trace.strength) * REINFORCEMENT;
            } else {
                traces.push(RelationTrace {
                    fingerprint: relation,
                    strength: REINFORCEMENT,
                });
                traces.sort_by(|a, b| {
                    b.strength
                        .total_cmp(&a.strength)
                        .then_with(|| a.fingerprint.cmp(&b.fingerprint))
                });
                traces.truncate(MAX_TRACES_PER_PAIR);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_octave_invariant_and_sparse() {
        assert_eq!(
            fingerprint(Ratio::new(3, 2).unwrap()),
            fingerprint(Ratio::new(3, 1).unwrap())
        );
        assert_eq!(
            fingerprint(Ratio::new(14, 11).unwrap()).0,
            vec![(7, 1), (11, -1)]
        );
    }

    #[test]
    fn distance_is_symmetric_and_zero_for_identity() {
        let a = fingerprint(Ratio::new(6, 5).unwrap());
        let b = fingerprint(Ratio::new(7, 5).unwrap());
        assert_eq!(relation_distance(&a, &a), 0.0);
        assert_eq!(relation_distance(&a, &b), relation_distance(&b, &a));
        assert!(relation_distance(&a, &b) > 0.0);
    }

    #[test]
    fn decay_and_trace_limit_are_bounded() {
        let mut memory = RelationMemory::default();
        let notes: Vec<_> = (0..5)
            .map(|id| ActiveNote {
                id,
                input_note: 60 + id as u8,
                current_offset_cents: None,
            })
            .collect();
        let tuned: Vec<_> = notes
            .iter()
            .map(|note| TunedNote {
                id: note.id,
                ratio: Ratio::new(1, 1).unwrap(),
                cents_offset: 0.0,
            })
            .collect();
        for _ in 0..10 {
            memory.record(&notes, &tuned, 1.0);
        }
        assert!(
            memory
                .map
                .values()
                .all(|traces| traces.len() <= MAX_TRACES_PER_PAIR)
        );
        memory.decay(0.0);
        assert!(memory.map.is_empty());
    }
}
