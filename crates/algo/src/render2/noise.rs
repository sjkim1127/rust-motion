//! Seeded random numbers, value noise and the spine curve shared by the renderers.

// ---------------------------------------------------------------------------
// Randomness
// ---------------------------------------------------------------------------

/// Small deterministic PRNG (SplitMix64).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u32, salt: u64) -> Rng {
        Rng(((seed as u64) << 32)
            ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ 0x2545_F491_4F6C_DD1D)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [a, b).
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    /// Uniform in [-1, 1).
    pub fn sym(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
}

/// Smooth 1-D value noise in [-1, 1].
pub(super) fn noise1(x: f32, seed: u64) -> f32 {
    let h = |i: i64| -> f32 {
        let mut r = Rng((i as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ seed);
        r.sym()
    };
    let i = x.floor();
    let f = x - i;
    let i = i as i64;
    let t = f * f * (3.0 - 2.0 * f);
    h(i) + (h(i + 1) - h(i)) * t
}

/// Two octaves of [`noise1`].
pub(super) fn fbm1(x: f32, seed: u64) -> f32 {
    (noise1(x, seed) + 0.5 * noise1(x * 2.13 + 7.1, seed ^ 0xA5A5)) / 1.5
}

// ---------------------------------------------------------------------------
// Polylines
// ---------------------------------------------------------------------------

/// A polyline parameterized by arc length.
pub(super) struct Spine {
    pts: Vec<(f32, f32)>,
    cum: Vec<f32>,
}

impl Spine {
    pub(super) fn new(pts: Vec<(f32, f32)>) -> Spine {
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            cum.push(cum.last().copied().unwrap_or(0.0) + l);
        }
        Spine { pts, cum }
    }
    pub(super) fn len(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }
    /// Point and unit tangent at arc length `s`.
    pub(super) fn at(&self, s: f32) -> ((f32, f32), (f32, f32)) {
        let n = self.pts.len();
        if n == 0 {
            return ((0.0, 0.0), (1.0, 0.0));
        }
        if n == 1 {
            return (self.pts[0], (0.0, -1.0));
        }
        let s = s.clamp(0.0, self.len());
        let k = match self
            .cum
            .binary_search_by(|v| v.partial_cmp(&s).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let (a, b) = (self.pts[k], self.pts[k + 1]);
        let seg = (self.cum[k + 1] - self.cum[k]).max(1e-6);
        let t = ((s - self.cum[k]) / seg).clamp(0.0, 1.0);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        ((a.0 + dx * t, a.1 + dy * t), (dx / l, dy / l))
    }
}
