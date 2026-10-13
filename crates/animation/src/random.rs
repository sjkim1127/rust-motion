//! Remotion-compatible deterministic pseudo-random number generator (`random()`).
//!
//! Provides Mulberry32 PRNG and 32-bit Java `hashCode` computation, matching
//! Remotion's `random(seed)` function exactly.

use std::fmt;

/// A seed value for [`random`].
///
/// Can be constructed from strings (`&str`, `String`) or numbers (`f64`, `f32`, `i32`, `i64`, `u32`, `u64`, `usize`).
#[derive(Clone, Debug, PartialEq)]
pub enum RandomSeed {
    /// Textual seed.
    Str(String),
    /// Numeric seed.
    Num(f64),
}

impl fmt::Display for RandomSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RandomSeed::Str(s) => write!(f, "{}", s),
            RandomSeed::Num(n) => write!(f, "{}", n),
        }
    }
}

impl From<&str> for RandomSeed {
    fn from(s: &str) -> Self {
        RandomSeed::Str(s.to_string())
    }
}

impl From<String> for RandomSeed {
    fn from(s: String) -> Self {
        RandomSeed::Str(s)
    }
}

impl From<&String> for RandomSeed {
    fn from(s: &String) -> Self {
        RandomSeed::Str(s.clone())
    }
}

impl From<f64> for RandomSeed {
    fn from(n: f64) -> Self {
        RandomSeed::Num(n)
    }
}

impl From<f32> for RandomSeed {
    fn from(n: f32) -> Self {
        RandomSeed::Num(n as f64)
    }
}

impl From<i32> for RandomSeed {
    fn from(n: i32) -> Self {
        RandomSeed::Num(n as f64)
    }
}

impl From<i64> for RandomSeed {
    fn from(n: i64) -> Self {
        RandomSeed::Num(n as f64)
    }
}

impl From<u32> for RandomSeed {
    fn from(n: u32) -> Self {
        RandomSeed::Num(n as f64)
    }
}

impl From<u64> for RandomSeed {
    fn from(n: u64) -> Self {
        RandomSeed::Num(n as f64)
    }
}

impl From<usize> for RandomSeed {
    fn from(n: usize) -> Self {
        RandomSeed::Num(n as f64)
    }
}

/// Computes 32-bit Java-compatible string hash code using UTF-16 code units.
///
/// Matches Remotion's `hashCode(str)` exactly:
/// `hash = ((hash << 5) - hash + charCode) | 0`
pub fn hash_code(s: &str) -> i32 {
    let mut hash: i32 = 0;
    for code_unit in s.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(code_unit as i32);
    }
    hash
}

/// Remotion Mulberry32 32-bit pseudo-random generator.
///
/// Takes a 32-bit integer seed and returns a deterministic floating-point number in `[0.0, 1.0)`.
#[inline]
pub fn mulberry32(a: i64) -> f64 {
    let a_u32 = a as u32;
    let t0 = a_u32.wrapping_add(0x6D2B79F5);
    let t1 = (t0 ^ (t0 >> 15)).wrapping_mul(t0 | 1);
    let t2 = t1 ^ t1.wrapping_add((t1 ^ (t1 >> 7)).wrapping_mul(t1 | 61));
    let result = t2 ^ (t2 >> 14);
    (result as f64) / 4294967296.0
}

/// Deterministic pseudo-random number generator matching Remotion's `random(seed)`.
///
/// - For string seeds: `mulberry32(hashCode(seed))`
/// - For numeric seeds: `mulberry32(seed * 10_000_000_000)`
///
/// # Example
/// ```rust
/// use rust_motion_animation::random;
///
/// let r1 = random("seed-1");
/// let r2 = random("seed-1");
/// assert_eq!(r1, r2);
/// assert!(r1 >= 0.0 && r1 < 1.0);
/// ```
#[inline]
pub fn random(seed: impl Into<RandomSeed>) -> f64 {
    match seed.into() {
        RandomSeed::Str(s) => mulberry32(hash_code(&s) as i64),
        RandomSeed::Num(n) => mulberry32((n * 10_000_000_000.0) as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_deterministic() {
        let r1 = random("remotion-seed");
        let r2 = random("remotion-seed");
        assert_eq!(r1, r2);
        assert!((0.0..1.0).contains(&r1));

        let n1 = random(42);
        let n2 = random(42);
        assert_eq!(n1, n2);
        assert!((0.0..1.0).contains(&n1));
        assert_ne!(r1, n1);
    }

    #[test]
    fn test_hash_code_known_values() {
        assert_eq!(hash_code(""), 0);
        assert_eq!(hash_code("a"), 97);
        assert_eq!(hash_code("hello"), 99162322);
    }
}
