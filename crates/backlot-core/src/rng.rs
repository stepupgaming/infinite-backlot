//! Deterministic randomness helpers.
//!
//! Every random decision in episode authoring must be reproducible from a seed
//! so that committed episodes can be replayed identically. We use `StdRng`
//! seeded from an explicit `u64`; no global entropy is used during authoring.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use std::fmt::Write as _;

/// A reproducible random source.
#[derive(Clone, Debug)]
pub struct SeededRng {
    inner: StdRng,
    pub seed: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self {
            inner: StdRng::seed_from_u64(seed),
            seed,
        }
    }

    /// Derive a child source so sub-systems get independent but reproducible streams.
    pub fn derive(&self, salt: u64) -> Self {
        Self::new(self.seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(salt))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.inner.gen()
    }

    pub fn next_f32(&mut self) -> f32 {
        self.inner.gen_range(0.0..1.0)
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        if hi <= lo {
            lo
        } else {
            self.inner.gen_range(lo..hi)
        }
    }

    pub fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        items.choose(&mut self.inner).cloned()
    }

    /// Pick `n` distinct items (or fewer if the slice is smaller).
    pub fn pick_n<T: Clone>(&mut self, items: &[T], n: usize) -> Vec<T> {
        let mut pool = items.to_vec();
        pool.shuffle(&mut self.inner);
        pool.truncate(n);
        pool
    }

    /// Weighted pick: `weights` must align with `items`.
    pub fn weighted_pick<T: Clone>(&mut self, items: &[T], weights: &[f32]) -> Option<T> {
        if items.is_empty() || items.len() != weights.len() {
            return None;
        }
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            return self.pick(items);
        }
        let mut threshold = self.inner.gen_range(0.0..total);
        for (item, w) in items.iter().zip(weights.iter()) {
            threshold -= w;
            if threshold <= 0.0 {
                return Some(item.clone());
            }
        }
        items.last().cloned()
    }
}

/// Build a short, readable identifier, e.g. `ep_000123`.
pub fn serial_id(prefix: &str, n: u64, width: usize) -> String {
    let mut s = String::new();
    let _ = write!(s, "{}_{:0width$}", prefix, n, width = width);
    s
}
