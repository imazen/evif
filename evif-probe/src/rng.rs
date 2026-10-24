//! SplitMix64, so a probe run is reproducible from `(image id, seed)`.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Seed derived from a run seed and a string, so each image gets its own
    /// stream and adding an image doesn't reshuffle the others.
    pub fn for_key(seed: u64, key: &str) -> Self {
        let mut h = zenhash::Xxh3::new();
        h.update(&seed.to_le_bytes());
        h.update(key.as_bytes());
        Self(h.digest())
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Log-uniform in `[lo, hi)`; both positive.
    pub fn log_range(&mut self, lo: f64, hi: f64) -> f64 {
        (self.range(lo.ln(), hi.ln())).exp()
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.unit() * n as f64) as usize
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
