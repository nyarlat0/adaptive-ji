//! Generic prime-limit validation and effective prime axes.

use std::{fmt, str::FromStr};

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "u32", into = "u32")]
pub struct PrimeLimit {
    max_prime: u32,
}

impl TryFrom<u32> for PrimeLimit {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<PrimeLimit> for u32 {
    fn from(value: PrimeLimit) -> Self {
        value.max_prime()
    }
}

pub const MAX_SUPPORTED_PRIME: u32 = 997;

impl PrimeLimit {
    pub fn new(max_prime: u32) -> Result<Self, String> {
        if !(3..=MAX_SUPPORTED_PRIME).contains(&max_prime) {
            return Err(format!("prime limit must be in 3..={MAX_SUPPORTED_PRIME}"));
        }
        if !is_prime(max_prime) {
            return Err(format!(
                "prime limit {max_prime} must itself be prime (try a neighboring prime)"
            ));
        }
        Ok(Self { max_prime })
    }

    pub const fn max_prime(self) -> u32 {
        self.max_prime
    }

    pub fn odd_primes(self) -> Vec<u32> {
        (3..=self.max_prime).filter(|&p| is_prime(p)).collect()
    }

    pub fn effective_primes(self, max_complexity: f64) -> Vec<u32> {
        self.odd_primes()
            .into_iter()
            .filter(|&p| f64::from(p).log2() <= max_complexity + 1e-12)
            .collect()
    }
}

impl Default for PrimeLimit {
    fn default() -> Self {
        Self { max_prime: 7 }
    }
}

impl FromStr for PrimeLimit {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = value
            .parse::<u32>()
            .map_err(|_| format!("prime limit must be a prime integer, got '{value}'"))?;
        Self::new(parsed)
    }
}

impl fmt::Display for PrimeLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.max_prime.fmt(f)
    }
}

const fn is_prime(value: u32) -> bool {
    if value < 2 {
        return false;
    }
    let mut divisor = 2;
    while divisor * divisor <= value {
        if value.is_multiple_of(divisor) {
            return false;
        }
        divisor += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_lists_arbitrary_prime_limits() {
        assert_eq!(PrimeLimit::new(13).unwrap().odd_primes(), [3, 5, 7, 11, 13]);
        assert!(PrimeLimit::new(12).is_err());
        assert!(PrimeLimit::new(2).is_err());
    }
    #[test]
    fn effective_primes_follow_complexity_budget() {
        let limit = PrimeLimit::new(101).unwrap();
        assert_eq!(limit.effective_primes(2.0), [3]);
        assert!(limit.effective_primes(12.0).contains(&101));
    }
}
