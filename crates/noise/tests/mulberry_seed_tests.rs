//! Integration tests for Mulberry32 PRNG and Java-compatible hashCode string seeding.

use rust_motion_noise::{hash_code, hash_seed, mulberry32, random, seed_to_float, NoiseSeed};

#[test]
fn test_hash_code_utf16_parity() {
    assert_eq!(hash_code(""), 0);
    assert_eq!(hash_code("a"), 97);
    assert_eq!(hash_code("abc"), 96354);
    assert_eq!(hash_code("hello"), 99162322);
    assert_eq!(hash_code("my-seed"), 1462865394);

    // Emojis / multi-byte UTF-16 surrogate pairs
    let emoji_seed = "noise-✨-seed";
    let h1 = hash_code(emoji_seed);
    let h2 = hash_code(emoji_seed);
    assert_eq!(h1, h2);
}

#[test]
fn test_mulberry32_prng_distribution() {
    let seed_val = 123456789i64;
    let r1 = mulberry32(seed_val);
    let r2 = mulberry32(seed_val);

    assert_eq!(r1, r2);
    assert!((0.0..1.0).contains(&r1));

    // Distribution across multiple seeds
    let mut min = 1.0f64;
    let mut max = 0.0f64;
    for i in 0..1000 {
        let val = mulberry32(i as i64);
        assert!((0.0..1.0).contains(&val));
        if val < min {
            min = val;
        }
        if val > max {
            max = val;
        }
    }
    assert!(min < 0.1);
    assert!(max > 0.9);
}

#[test]
fn test_random_function_behavior() {
    let r_str = random("remotion-seed");
    assert!((0.0..1.0).contains(&r_str));

    let r_num = random(42.5);
    assert!((0.0..1.0).contains(&r_num));

    let r_int = random(100);
    assert!((0.0..1.0).contains(&r_int));
}

#[test]
fn test_legacy_hash_seed_and_seed_to_float() {
    let h1 = hash_seed("test-legacy");
    let h2 = hash_seed("test-legacy");
    assert_eq!(h1, h2);

    let f = seed_to_float(h1);
    assert!((0.0..1.0).contains(&f));
}

#[test]
fn test_noise_seed_display() {
    let s1 = NoiseSeed::Str("hello".to_string());
    assert_eq!(format!("{}", s1), "hello");

    let s2 = NoiseSeed::Num(42.5);
    assert_eq!(format!("{}", s2), "42.5");
}
