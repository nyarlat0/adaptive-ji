/// MIDI channels in this crate use wire numbering: 0 = master, 1..=15 = members.
pub const CENTER: u16 = 8192;

/// Invalid ranges or NaN offsets produce a neutral bend. Infinities saturate.
pub fn cents_to_pitch_bend(cents: f64, range_semitones: f64) -> u16 {
    if !range_semitones.is_finite() || range_semitones <= 0.0 || cents.is_nan() {
        return CENTER;
    }
    (8192.0 + cents / (range_semitones * 100.0) * 8192.0)
        .round()
        .clamp(0.0, 16383.0) as u16
}

pub fn encode_pitch_bend(channel: u8, value: u16) -> [u8; 3] {
    let value = value.min(16383);
    [
        0xe0 | (channel & 0x0f),
        (value & 0x7f) as u8,
        (value >> 7) as u8,
    ]
}

/// A range exactly representable by RPN 0 (semitones plus cents).
#[derive(Clone, Copy, Debug)]
pub struct BendRange {
    cents: u16,
}

impl BendRange {
    pub fn semitones(self) -> f64 {
        f64::from(self.cents) / 100.0
    }

    pub fn rpn(self, channel: u8) -> [[u8; 3]; 6] {
        let cc = 0xb0 | (channel & 0x0f);
        [
            [cc, 101, 0],
            [cc, 100, 0],
            [cc, 6, (self.cents / 100) as u8],
            [cc, 38, (self.cents % 100) as u8],
            [cc, 101, 127],
            [cc, 100, 127],
        ]
    }
}

impl std::str::FromStr for BendRange {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let error = "bend range must be 0.01..=127.99 semitones, in steps of 0.01";
        let semitones: f64 = value.parse().map_err(|_| error.to_owned())?;
        let cents = semitones * 100.0;
        if !cents.is_finite()
            || !(1.0..=12799.0).contains(&cents)
            || (cents - cents.round()).abs() > 1e-7
        {
            return Err(error.to_owned());
        }
        Ok(Self {
            cents: cents.round() as u16,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_and_scaling() {
        assert_eq!(cents_to_pitch_bend(0.0, 2.0), CENTER);
        assert_eq!(cents_to_pitch_bend(100.0, 2.0), 12288);
        assert_eq!(cents_to_pitch_bend(-100.0, 2.0), 4096);
        assert_eq!(cents_to_pitch_bend(-13.686286, 2.0), 7631);
        assert_eq!(cents_to_pitch_bend(1.955001, 2.0), 8272);
        assert_eq!(cents_to_pitch_bend(100.0, 4.0), 10240);
    }

    #[test]
    fn clamps_and_handles_invalid_numbers() {
        assert_eq!(cents_to_pitch_bend(-200.0, 2.0), 0);
        assert_eq!(cents_to_pitch_bend(200.0, 2.0), 16383);
        assert_eq!(cents_to_pitch_bend(9999.0, 2.0), 16383);
        assert_eq!(cents_to_pitch_bend(-9999.0, 2.0), 0);
        assert_eq!(cents_to_pitch_bend(f64::NAN, 2.0), CENTER);
        assert_eq!(cents_to_pitch_bend(10.0, 0.0), CENTER);
    }

    #[test]
    fn wire_encoding_is_lsb_first() {
        assert_eq!(encode_pitch_bend(1, CENTER), [0xe1, 0, 64]);
        assert_eq!(encode_pitch_bend(15, 16383), [0xef, 127, 127]);
        assert_eq!(encode_pitch_bend(2, 7631), [0xe2, 79, 59]);
    }

    #[test]
    fn validates_range_and_encodes_rpn() {
        let range: BendRange = "2.5".parse().unwrap();
        assert_eq!(range.semitones(), 2.5);
        assert_eq!(
            range.rpn(1),
            [
                [0xb1, 101, 0],
                [0xb1, 100, 0],
                [0xb1, 6, 2],
                [0xb1, 38, 50],
                [0xb1, 101, 127],
                [0xb1, 100, 127],
            ]
        );
        for invalid in ["0", "-2", "NaN", "inf", "128", "2.001", "abc"] {
            assert!(invalid.parse::<BendRange>().is_err());
        }
    }
}
