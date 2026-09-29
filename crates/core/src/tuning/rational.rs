//! Positive, reduced rational intervals. Arithmetic checks overflow after cancellation.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio {
    num: u64,
    den: u64,
}

pub fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Ratio {
    pub const ONE: Self = Self { num: 1, den: 1 };

    pub fn new(num: u64, den: u64) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let divisor = gcd(num, den);
        Some(Self {
            num: num / divisor,
            den: den / divisor,
        })
    }

    pub fn numerator(self) -> u64 {
        self.num
    }
    pub fn denominator(self) -> u64 {
        self.den
    }

    pub fn checked_mul(self, other: Self) -> Option<Self> {
        let a = gcd(self.num, other.den);
        let b = gcd(other.num, self.den);
        Some(Self {
            num: (self.num / a).checked_mul(other.num / b)?,
            den: (self.den / b).checked_mul(other.den / a)?,
        })
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        self.checked_mul(other.reciprocal())
    }

    pub fn reciprocal(self) -> Self {
        Self {
            num: self.den,
            den: self.num,
        }
    }

    /// Multiply by 2^exponent, cancelling powers of two before checking overflow.
    pub fn scale_pow2(self, exponent: i32) -> Option<Self> {
        if exponent < 0 {
            return self
                .reciprocal()
                .scale_up(exponent.unsigned_abs())
                .map(Self::reciprocal);
        }
        self.scale_up(exponent as u32)
    }

    fn scale_up(self, exponent: u32) -> Option<Self> {
        let cancelled = self.den.trailing_zeros().min(exponent);
        let factor = 1_u64.checked_shl(exponent - cancelled)?;
        Some(Self {
            num: self.num.checked_mul(factor)?,
            den: self.den >> cancelled,
        })
    }

    pub fn cents(self) -> f64 {
        1200.0 * ((self.num as f64).log2() - (self.den as f64).log2())
    }

    pub fn odd_parts(self) -> (u64, u64) {
        (
            self.num >> self.num.trailing_zeros(),
            self.den >> self.den.trailing_zeros(),
        )
    }
}

/// Octave-invariant modified Tenney height, without overflowing odd_num * odd_den.
pub fn harmonic_complexity(ratio: Ratio) -> f64 {
    let (num, den) = ratio.odd_parts();
    (num as f64).log2() + (den as f64).log2()
}

/// Prime exponents of the odd part: numerator exponents are positive and
/// denominator exponents are negative. Powers of two are deliberately absent.
pub fn prime_factorization_without_two(ratio: Ratio) -> BTreeMap<u64, i32> {
    fn add_factors(mut value: u64, sign: i32, factors: &mut BTreeMap<u64, i32>) {
        value >>= value.trailing_zeros();
        let mut prime = 3_u64;
        while prime <= value / prime {
            while value.is_multiple_of(prime) {
                *factors.entry(prime).or_default() += sign;
                value /= prime;
            }
            prime += 2;
        }
        if value > 1 {
            *factors.entry(value).or_default() += sign;
        }
    }

    let mut factors = BTreeMap::new();
    add_factors(ratio.numerator(), 1, &mut factors);
    add_factors(ratio.denominator(), -1, &mut factors);
    factors.retain(|_, exponent| *exponent != 0);
    factors
}

impl std::fmt::Display for Ratio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduction_and_invalid_values() {
        assert_eq!(Ratio::new(12, 8), Ratio::new(3, 2));
        assert_eq!(Ratio::new(7, 7), Some(Ratio::ONE));
        assert!(Ratio::new(0, 1).is_none());
        assert!(Ratio::new(1, 0).is_none());
        assert_eq!(gcd(0, 12), 12);
    }

    #[test]
    fn multiply_divide_and_cancel_before_overflow() {
        let third = Ratio::new(5, 4).unwrap();
        let fifth = Ratio::new(3, 2).unwrap();
        assert_eq!(third.checked_mul(fifth), Ratio::new(15, 8));
        assert_eq!(fifth.checked_div(third), Ratio::new(6, 5));
        let large = Ratio::new(u64::MAX, 2).unwrap();
        assert_eq!(large.checked_mul(large.reciprocal()), Some(Ratio::ONE));
        assert!(large.checked_mul(large).is_none());
    }

    #[test]
    fn octave_scaling_and_cents() {
        let third = Ratio::new(5, 4).unwrap();
        assert_eq!(third.scale_pow2(2), Ratio::new(5, 1));
        assert_eq!(third.scale_pow2(-1), Ratio::new(5, 8));
        assert_eq!(Ratio::ONE.cents(), 0.0);
        assert!((third.cents() - 386.313713865).abs() < 1e-8);
        assert!((third.reciprocal().cents() + third.cents()).abs() < 1e-8);
        assert!(Ratio::ONE.scale_pow2(i32::MIN).is_none());
        assert!(Ratio::new(u64::MAX, 1).unwrap().scale_pow2(1).is_none());
    }

    #[test]
    fn strips_twos_and_computes_modified_tenney_height() {
        for (num, den, odd_num, odd_den) in [
            (1, 1, 1, 1),
            (3, 2, 3, 1),
            (5, 4, 5, 1),
            (7, 4, 7, 1),
            (6, 5, 3, 5),
            (16, 15, 1, 15),
        ] {
            let ratio = Ratio::new(num, den).unwrap();
            assert_eq!(ratio.odd_parts(), (odd_num, odd_den));
            let expected = ((odd_num * odd_den) as f64).log2();
            assert!((harmonic_complexity(ratio) - expected).abs() < 1e-12);
            assert!((harmonic_complexity(ratio.scale_pow2(-3).unwrap()) - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn undecimal_ratios_reduce_have_expected_cents_complexity_and_factors() {
        for (num, den, cents, factors) in [
            (11, 8, 551.318, [(11, 1)].into_iter().collect()),
            (11, 7, 782.492, [(7, -1), (11, 1)].into_iter().collect()),
            (14, 11, 417.508, [(7, 1), (11, -1)].into_iter().collect()),
            (20, 11, 1034.996, [(5, 1), (11, -1)].into_iter().collect()),
        ] {
            let ratio = Ratio::new(num, den).unwrap();
            assert!((ratio.cents() - cents).abs() < 1.0);
            assert_eq!(prime_factorization_without_two(ratio), factors);
        }
        assert!((harmonic_complexity(Ratio::new(11, 8).unwrap()) - 11_f64.log2()).abs() < 1e-12);
    }
}
