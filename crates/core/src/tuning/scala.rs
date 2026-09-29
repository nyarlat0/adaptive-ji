//! Scala scale and keyboard-map loading. The mapped pitches are baselines for
//! adaptive rational candidate search, not a replacement tuning strategy.

#[cfg(test)]
use std::{fs, path::Path};

const MAX_DEGREES: usize = 1024;

#[derive(Clone, Debug)]
pub struct ScalaScale {
    pub description: String,
    /// Degree zero (1/1) is implicit; the last entry is the period.
    pitches: Vec<f64>,
}

impl ScalaScale {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = data_lines(text);
        let description = lines.next().ok_or(".scl: missing description")?.to_owned();
        let count: usize = lines
            .next()
            .ok_or(".scl: missing scale size")?
            .parse()
            .map_err(|_| ".scl: invalid scale size")?;
        if !(1..=MAX_DEGREES).contains(&count) {
            return Err(format!(".scl: scale size must be in 1..={MAX_DEGREES}"));
        }
        let mut pitches = Vec::with_capacity(count);
        for index in 0..count {
            let line = lines
                .next()
                .ok_or_else(|| format!(".scl: missing pitch {} of {count}", index + 1))?;
            let token = line.split_whitespace().next().unwrap_or("");
            let cents = if token.contains('.') {
                token
                    .parse::<f64>()
                    .map_err(|_| format!(".scl: invalid cents at degree {}", index + 1))?
            } else {
                let (num, den) = token.split_once('/').unwrap_or((token, "1"));
                let num = num
                    .parse::<f64>()
                    .map_err(|_| format!(".scl: invalid ratio at degree {}", index + 1))?;
                let den = den
                    .parse::<f64>()
                    .map_err(|_| format!(".scl: invalid ratio at degree {}", index + 1))?;
                if !num.is_finite() || !den.is_finite() || num <= 0.0 || den <= 0.0 {
                    return Err(format!(
                        ".scl: ratio must be positive at degree {}",
                        index + 1
                    ));
                }
                1200.0 * (num / den).log2()
            };
            if !cents.is_finite() {
                return Err(format!(".scl: non-finite pitch at degree {}", index + 1));
            }
            pitches.push(cents);
        }
        if pitches[count - 1] <= 0.0 {
            return Err(".scl: final pitch (period) must be positive".into());
        }
        Ok(Self {
            description,
            pitches,
        })
    }

    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read scale {}: {error}", path.display()))?;
        Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn size(&self) -> usize {
        self.pitches.len()
    }

    pub fn period_cents(&self) -> f64 {
        *self.pitches.last().expect("validated scale has a period")
    }

    fn degree_cents(&self, degree: i32) -> f64 {
        let size = self.size() as i32;
        let period = degree.div_euclid(size);
        let position = degree.rem_euclid(size) as usize;
        let within = if position == 0 {
            0.0
        } else {
            self.pitches[position - 1]
        };
        f64::from(period) * self.period_cents() + within
    }
}

#[derive(Clone, Debug)]
pub struct KeyboardMapping {
    size: usize,
    first: u8,
    last: u8,
    middle: u8,
    reference: u8,
    reference_frequency: f64,
    formal_degree: i32,
    entries: Vec<Option<i32>>,
}

impl KeyboardMapping {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = data_lines(text);
        let mut next = |field: &str| -> Result<&str, String> {
            lines.next().ok_or_else(|| format!(".kbm: missing {field}"))
        };
        let size = parse_field::<usize>(next("map size")?, "map size")?;
        if size > MAX_DEGREES {
            return Err(format!(".kbm: map size must be <= {MAX_DEGREES}"));
        }
        let first = parse_field::<u8>(next("first MIDI note")?, "first MIDI note")?;
        let last = parse_field::<u8>(next("last MIDI note")?, "last MIDI note")?;
        let middle = parse_field::<u8>(next("middle MIDI note")?, "middle MIDI note")?;
        let reference = parse_field::<u8>(next("reference MIDI note")?, "reference MIDI note")?;
        if first > 127 || last > 127 || middle > 127 || reference > 127 || first > last {
            return Err(".kbm: MIDI notes must be 0..=127 with first <= last".into());
        }
        let reference_frequency =
            parse_field::<f64>(next("reference frequency")?, "reference frequency")?;
        if !reference_frequency.is_finite() || reference_frequency <= 0.0 {
            return Err(".kbm: reference frequency must be finite and positive".into());
        }
        let formal_degree =
            parse_field::<i32>(next("formal octave degree")?, "formal octave degree")?;
        if formal_degree.unsigned_abs() > 1_000_000 {
            return Err(".kbm: formal octave degree magnitude must be <= 1000000".into());
        }
        let mut entries = Vec::with_capacity(size);
        for line in lines.take(size) {
            let entry = if line.eq_ignore_ascii_case("x") {
                None
            } else {
                let degree = parse_field::<i32>(line, "mapping degree")?;
                if degree.unsigned_abs() > 1_000_000 {
                    return Err(".kbm: mapping degree magnitude must be <= 1000000".into());
                }
                Some(degree)
            };
            entries.push(entry);
        }
        // Scala allows trailing omitted entries; they are unmapped.
        entries.resize(size, None);
        let mapping = Self {
            size,
            first,
            last,
            middle,
            reference,
            reference_frequency,
            formal_degree,
            entries,
        };
        if mapping.degree(reference).is_none() {
            return Err(".kbm: reference note is unmapped or outside the retuned range".into());
        }
        Ok(mapping)
    }

    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("cannot read mapping {}: {error}", path.display()))?;
        Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    fn degree(&self, note: u8) -> Option<i32> {
        if note < self.first || note > self.last {
            return None;
        }
        let distance = i32::from(note) - i32::from(self.middle);
        if self.size == 0 {
            return Some(distance);
        }
        let size = self.size as i32;
        let cycle = distance.div_euclid(size);
        let index = distance.rem_euclid(size) as usize;
        self.entries[index].map(|degree| degree + cycle * self.formal_degree)
    }
}

fn parse_field<T: std::str::FromStr>(line: &str, name: &str) -> Result<T, String> {
    line.split_whitespace()
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| format!(".kbm: invalid {name}"))
}

fn data_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|line| line.split('!').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
}

/// MIDI number (0..127) or scientific pitch name such as C4 / F#3 / Bb2.
pub fn parse_root_note(text: &str) -> Result<u8, String> {
    if let Ok(note) = text.parse::<u8>() {
        return (note <= 127)
            .then_some(note)
            .ok_or_else(|| "root note must be MIDI 0..=127".into());
    }
    let upper = text.trim().to_ascii_uppercase();
    let mut chars = upper.chars();
    let letter = chars.next().ok_or("root note is empty")?;
    let pitch = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err("root note must be MIDI 0..127 or a note like C4/F#3/Bb2".into()),
    };
    let rest = &upper[letter.len_utf8()..];
    let (accidental, octave) = match rest.as_bytes().first() {
        Some(b'#') => (1, &rest[1..]),
        Some(b'B') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i16 = octave
        .parse()
        .map_err(|_| "root note needs an octave, for example C4".to_string())?;
    let midi = (octave + 1) * 12 + pitch + accidental;
    u8::try_from(midi)
        .ok()
        .filter(|note| *note <= 127)
        .ok_or_else(|| "root note must be in MIDI 0..=127".into())
}

#[derive(Clone, Debug)]
pub struct ScalaTuning {
    pub scale: ScalaScale,
    /// None means an unmapped (`x`) or unrepresentable key. Outside a KBM
    /// retune range, MIDI is left at its original 12-TET pitch.
    targets: [Option<ScalaTarget>; 128],
    /// Keep requested pitches even when they cannot be emitted, for diagnostics.
    baseline_offsets: [Option<f64>; 128],
    unreachable_keys: usize,
}

#[derive(Clone, Copy, Debug)]
struct ScalaTarget {
    output_note: u8,
    cents_offset: f64,
}

impl ScalaTuning {
    pub fn new(
        scale: ScalaScale,
        mapping: Option<KeyboardMapping>,
        root: u8,
        bend_range_semitones: f64,
    ) -> Result<Self, String> {
        if !bend_range_semitones.is_finite() || bend_range_semitones <= 0.0 {
            return Err("Scala tuning requires a positive finite bend range".into());
        }
        let mut absolute_offsets = [None; 128];
        if let Some(mapping) = mapping {
            let reference_degree = mapping
                .degree(mapping.reference)
                .expect("validated reference");
            let reference_cents_from_a4 = 1200.0 * (mapping.reference_frequency / 440.0).log2();
            for note in 0..=127u8 {
                absolute_offsets[note as usize] = if note < mapping.first || note > mapping.last {
                    Some(0.0)
                } else {
                    mapping.degree(note).map(|degree| {
                        reference_cents_from_a4 + scale.degree_cents(degree)
                            - scale.degree_cents(reference_degree)
                            - 100.0 * (f64::from(note) - 69.0)
                    })
                };
            }
        } else {
            for note in 0..=127u8 {
                let degree = i32::from(note) - i32::from(root);
                absolute_offsets[note as usize] =
                    Some(scale.degree_cents(degree) - 100.0 * f64::from(degree));
            }
        }
        if absolute_offsets
            .iter()
            .flatten()
            .any(|offset| !offset.is_finite())
        {
            return Err("Scala mapping produced a non-finite MIDI offset".into());
        }
        let mut targets = [None; 128];
        let mut unreachable_keys = 0;
        for (note, offset) in absolute_offsets.iter().copied().enumerate() {
            let Some(offset) = offset else { continue };
            let target_midi = note as f64 + offset / 100.0;
            let output_note = target_midi.round().clamp(0.0, 127.0) as u8;
            let residual = (target_midi - f64::from(output_note)) * 100.0;
            if residual.abs() > bend_range_semitones * 100.0 + 1e-9 {
                unreachable_keys += 1;
            } else {
                targets[note] = Some(ScalaTarget {
                    output_note,
                    cents_offset: residual,
                });
            }
        }
        if targets.iter().all(Option::is_none) {
            return Err(format!(
                "no Scala keys fit the ±{bend_range_semitones} semitone bend range; increase --bend-range"
            ));
        }
        Ok(Self {
            scale,
            targets,
            baseline_offsets: absolute_offsets,
            unreachable_keys,
        })
    }

    pub fn offset(&self, note: u8) -> Option<f64> {
        self.targets[usize::from(note)].map(|target| target.cents_offset)
    }

    /// Requested Scala offset relative to the input key's 12-TET pitch, even
    /// outside the MIDI output range. `accepts` determines whether it can sound.
    pub fn baseline_offset(&self, note: u8) -> Option<f64> {
        self.baseline_offsets[usize::from(note)]
    }

    pub fn accepts(&self, note: u8) -> bool {
        self.targets[usize::from(note)].is_some()
    }

    pub fn output_note_for(&self, note: u8) -> Option<u8> {
        self.targets[usize::from(note)].map(|target| target.output_note)
    }

    pub fn unreachable_keys(&self) -> usize {
        self.unreachable_keys
    }

    /// Input MIDI ranges, not the connected keyboard's physical key range.
    pub fn playable_key_ranges(&self) -> String {
        key_ranges(|note| self.accepts(note))
    }

    pub fn unreachable_key_ranges(&self) -> String {
        key_ranges(|note| !self.accepts(note) && self.baseline_offset(note).is_some())
    }

    /// Reserve room for the adaptive optimizer to move away from the Scala
    /// baseline without silently clipping the MPE bend at the MIDI endpoints.
    pub fn with_adaptive_headroom(
        mut self,
        bend_range_semitones: f64,
        max_deviation_cents: f64,
    ) -> Result<Self, String> {
        for note in 0..128 {
            if let Some(target) = self.targets[note]
                && target.cents_offset.abs() + max_deviation_cents
                    > bend_range_semitones * 100.0 + 1e-9
            {
                self.targets[note] = None;
                self.unreachable_keys += 1;
            }
        }
        if self.targets.iter().all(Option::is_none) {
            return Err(format!(
                "no Scala keys leave ±{max_deviation_cents}c adaptive headroom within the ±{bend_range_semitones} semitone bend range; increase --bend-range"
            ));
        }
        Ok(self)
    }

    pub fn max_abs_offset(&self) -> f64 {
        self.targets
            .iter()
            .flatten()
            .map(|target| target.cents_offset.abs())
            .fold(0.0, f64::max)
    }
}

fn key_ranges(included: impl Fn(u8) -> bool) -> String {
    let mut ranges = Vec::new();
    let mut start = None;
    // The sentinel closes a range ending at MIDI 127.
    for note in 0..=128u16 {
        if note < 128 && included(note as u8) {
            start.get_or_insert(note);
        } else if let Some(first) = start.take() {
            let last = note - 1;
            ranges.push(if first == last {
                first.to_string()
            } else {
                format!("{first}..{last}")
            });
        }
    }
    if ranges.is_empty() {
        "none".into()
    } else {
        ranges.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = "! comment\nSimple\n3\n100.0\n3/2\n2/1\n";

    #[test]
    fn scl_parses_cents_ratios_and_negative_degrees() {
        let scale = ScalaScale::parse(SIMPLE).unwrap();
        assert_eq!(scale.description, "Simple");
        assert_eq!(scale.size(), 3);
        assert!((scale.degree_cents(2) - 701.955).abs() < 0.001);
        assert!((scale.degree_cents(3) - 1200.0).abs() < 1e-9);
        assert!((scale.degree_cents(-1) + 498.045).abs() < 0.001);
        assert!(ScalaScale::parse("Bad\n1\n0/1").is_err());
        assert!(ScalaScale::parse("Bad\n1\nNaN").is_err());
        assert!(ScalaScale::parse("Bad\n2\n100.0").is_err());
    }

    #[test]
    fn default_mapping_uses_root_and_scale_period() {
        let scale = ScalaScale::parse(SIMPLE).unwrap();
        let tuning = ScalaTuning::new(scale, None, 60, 2.0).unwrap();
        assert_eq!(tuning.offset(60), Some(0.0));
        assert_eq!(tuning.output_note_for(61), Some(61));
        assert_eq!(tuning.offset(61), Some(0.0));
        assert_eq!(tuning.output_note_for(62), Some(67));
        assert!((tuning.offset(62).unwrap() - 1.955).abs() < 0.001);
        assert_eq!(tuning.output_note_for(63), Some(72));
        assert_eq!(tuning.output_note_for(59), Some(55));
        assert!((tuning.offset(59).unwrap() - 1.955).abs() < 0.001);
        let shifted = ScalaTuning::new(ScalaScale::parse(SIMPLE).unwrap(), None, 62, 2.0).unwrap();
        assert_eq!(shifted.offset(62), Some(0.0));
    }

    #[test]
    fn kbm_honors_reference_frequency_gaps_and_formal_degree() {
        let map = KeyboardMapping::parse("3\n60\n72\n60\n60\n261.625565\n3\n0\nx\n2\n").unwrap();
        assert_eq!(map.degree(60), Some(0));
        assert_eq!(map.degree(61), None);
        assert_eq!(map.degree(63), Some(3));
        assert_eq!(map.degree(59), None);
        let tuning =
            ScalaTuning::new(ScalaScale::parse(SIMPLE).unwrap(), Some(map), 60, 2.0).unwrap();
        assert!(tuning.offset(60).unwrap().abs() < 0.001);
        assert!(!tuning.accepts(61));
        assert_eq!(tuning.offset(59), Some(0.0));
        assert!(tuning.offset(63).unwrap().abs() < 0.001);
        assert!(KeyboardMapping::parse("2\n60\n72\n60\n60\n440.0\n2\nx\n1").is_err());
    }

    #[test]
    fn zero_size_kbm_is_linear_and_fractional_frequency_is_respected() {
        let map = KeyboardMapping::parse("0\n0\n127\n69\n69\n442.0\n3").unwrap();
        assert_eq!(map.degree(68), Some(-1));
        let tuning =
            ScalaTuning::new(ScalaScale::parse(SIMPLE).unwrap(), Some(map), 60, 2.0).unwrap();
        assert!((tuning.offset(69).unwrap() - 7.851415).abs() < 0.001);
    }

    #[test]
    fn imported_files_and_note_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scales");
        let byz = ScalaScale::load(&root.join("albionbyz.scl")).unwrap();
        assert_eq!(byz.size(), 12);
        let byz = ScalaTuning::new(byz, None, 60, 2.0).unwrap();
        assert_eq!(byz.offset(60), Some(0.0));
        assert!((byz.offset(72).unwrap()).abs() < 1e-8);
        let scl = ScalaScale::load(&root.join("lim_13_operator.scl")).unwrap();
        let kbm = KeyboardMapping::load(&root.join("lim_13_operator.kbm")).unwrap();
        let tuning = ScalaTuning::new(scl, Some(kbm), 60, 2.0).unwrap();
        assert!((tuning.offset(66).unwrap() + 48.671).abs() < 0.02);
        assert_eq!(tuning.unreachable_keys(), 0);
        assert_eq!(parse_root_note("C4").unwrap(), 60);
        assert_eq!(parse_root_note("F#3").unwrap(), 54);
        assert_eq!(parse_root_note("Bb2").unwrap(), 46);
        assert_eq!(parse_root_note("69").unwrap(), 69);
        assert!(parse_root_note("128").is_err());
    }

    #[test]
    fn non_octave_scale_remaps_output_notes_and_reports_unreachable_edges() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scales");
        let scl = ScalaScale::load(&root.join("13_equal_div_3_1.scl")).unwrap();
        let kbm = KeyboardMapping::load(&root.join("13_equal_div_3_1.kbm")).unwrap();
        let tuning = ScalaTuning::new(scl, Some(kbm), 60, 2.0).unwrap();
        assert_eq!(tuning.output_note_for(60), Some(60));
        assert_eq!(tuning.output_note_for(65), Some(67));
        assert!(tuning.offset(65).unwrap().abs() <= 50.0);
        assert!(tuning.unreachable_keys() > 0);
        assert!(!tuning.accepts(127));
        assert!(tuning.max_abs_offset() <= 200.0);
    }

    #[test]
    fn tritave_map_keeps_middle_keyboard_range_with_two_semitone_bend() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scales");
        let scl = ScalaScale::load(&root.join("13_equal_div_3_1.scl")).unwrap();
        let kbm = KeyboardMapping::load(&root.join("13_equal_div_3_1.kbm")).unwrap();
        let baseline = ScalaTuning::new(scl, Some(kbm), 60, 2.0).unwrap();
        // 107 fits its baseline pitch, but needs more bend room to guarantee JI.
        assert!(baseline.accepts(107));
        let tuning = baseline.with_adaptive_headroom(2.0, 50.0).unwrap();
        assert_eq!(tuning.playable_key_ranges(), "18..106");
        assert_eq!(tuning.unreachable_key_ranges(), "0..17, 107..127");
        assert_eq!(tuning.unreachable_keys(), 39);
        for note in 36..=96 {
            assert!(tuning.accepts(note), "middle keyboard note {note}");
            assert!(tuning.offset(note).unwrap().abs() + 50.0 <= 200.0);
        }
        // Preserve 3:1 per 13 keys, including requested pitches beyond MIDI's
        // output range; diagnostics must distinguish these from KBM holes.
        let pitch = |note: u8| f64::from(note) * 100.0 + tuning.baseline_offset(note).unwrap();
        assert!((pitch(73) - pitch(60) - 1200.0 * 3.0f64.log2()).abs() < 1e-6);
        assert!(pitch(0) < -200.0);
        assert!(pitch(127) > 12900.0);
        assert!(!tuning.accepts(107));
        assert!(tuning.baseline_offset(107).is_some());
    }
}
