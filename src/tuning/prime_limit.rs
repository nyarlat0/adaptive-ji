//! Supported prime-exponent axes for the rational candidate lattice.

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash, clap::ValueEnum)]
pub enum PrimeLimit {
    #[value(name = "5")]
    Five,
    #[default]
    #[value(name = "7")]
    Seven,
    #[value(name = "11")]
    Eleven,
}

impl PrimeLimit {
    pub const fn axes(self) -> &'static [(u64, i32)] {
        match self {
            Self::Five => &[(3, 4), (5, 3)],
            Self::Seven => &[(3, 4), (5, 3), (7, 2)],
            Self::Eleven => &[(3, 4), (5, 3), (7, 2), (11, 1)],
        }
    }

    pub const fn raw_lattice_size(self) -> usize {
        match self {
            Self::Five => 9 * 7,
            Self::Seven => 9 * 7 * 5,
            Self::Eleven => 9 * 7 * 5 * 3,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Five => "5",
            Self::Seven => "7",
            Self::Eleven => "11",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_have_expected_sizes() {
        assert_eq!(PrimeLimit::Five.raw_lattice_size(), 63);
        assert_eq!(PrimeLimit::Seven.raw_lattice_size(), 315);
        assert_eq!(PrimeLimit::Eleven.raw_lattice_size(), 945);
        assert_eq!(PrimeLimit::Five.axes().len(), 2);
        assert_eq!(PrimeLimit::Seven.axes().len(), 3);
        assert_eq!(PrimeLimit::Eleven.axes().len(), 4);
    }
}
