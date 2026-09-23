use super::{ActiveNote, Ratio, TunedNote, TuningStrategy};

const RATIOS: [(u32, u32); 12] = [
    (1, 1),
    (16, 15),
    (9, 8),
    (6, 5),
    (5, 4),
    (4, 3),
    (45, 32),
    (3, 2),
    (8, 5),
    (5, 3),
    (9, 5),
    (15, 8),
];

#[derive(Default)]
pub struct FiveLimitLowestRoot;

impl TuningStrategy for FiveLimitLowestRoot {
    fn tune(&mut self, active_notes: &[ActiveNote]) -> Vec<TunedNote> {
        let Some(root) = active_notes.iter().map(|note| note.input_note).min() else {
            return Vec::new();
        };
        active_notes
            .iter()
            .map(|note| {
                let interval = note.input_note - root;
                let (mut numerator, mut denominator) = RATIOS[usize::from(interval % 12)];
                numerator *= 1 << (interval / 12);
                // The table is reduced; octave multiplication can only add factors of two.
                while numerator.is_multiple_of(2) && denominator.is_multiple_of(2) {
                    numerator /= 2;
                    denominator /= 2;
                }
                let ratio = Ratio {
                    numerator,
                    denominator,
                };
                let ji_cents = 1200.0 * (f64::from(numerator) / f64::from(denominator)).log2();
                TunedNote {
                    id: note.id,
                    ratio,
                    cents_offset: ji_cents - f64::from(interval) * 100.0,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tune(notes: &[u8]) -> Vec<TunedNote> {
        FiveLimitLowestRoot.tune(
            &notes
                .iter()
                .enumerate()
                .map(|(id, &input_note)| ActiveNote {
                    id: id as u64,
                    input_note,
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn c_major_and_fourth() {
        let result = tune(&[60, 64, 67, 65]);
        for (note, expected) in result.iter().zip([0.0, -13.686286, 1.955001, -1.955001]) {
            assert!((note.cents_offset - expected).abs() < 0.000001);
        }
        assert_eq!(result[0].ratio.to_string(), "1/1");
        assert_eq!(result[1].ratio.to_string(), "5/4");
        assert_eq!(result[2].ratio.to_string(), "3/2");
    }

    #[test]
    fn octaves_and_unsorted_input() {
        let result = tune(&[88, 60, 72, 76]);
        assert_eq!(result[0].ratio.to_string(), "5/1");
        assert_eq!(result[2].ratio.to_string(), "2/1");
        assert_eq!(result[3].ratio.to_string(), "5/2");
        assert_eq!(result[2].cents_offset, 0.0);
        assert!((result[0].cents_offset - result[3].cents_offset).abs() < 1e-9);
    }

    #[test]
    fn empty_and_duplicate_notes() {
        assert!(tune(&[]).is_empty());
        let result = tune(&[60, 64, 64]);
        assert_ne!(result[1].id, result[2].id);
        assert_eq!(result[1].cents_offset, result[2].cents_offset);
    }

    #[test]
    fn full_midi_range_is_finite() {
        let result = tune(&[0, 127]);
        assert_eq!(result[1].ratio.to_string(), "1536/1");
        assert!((result[1].cents_offset - 1.955001).abs() < 1e-6);
    }
}
