//! Sample generation for the path tracer.
//!
//! The integrator draws all of its random decisions (pixel jitter, lens, BSDF
//! directions, light sampling, Russian roulette) from a [`Sampler`] rather than
//! a raw PRNG. This lets us use a low-discrepancy (quasi-Monte-Carlo) sequence,
//! which converges with markedly less noise than white noise for the same
//! sample count.
//!
//! The construction is **padded, hash-based Owen-scrambled Sobol** (Burley
//! 2020): every sampling decision gets its own independently Owen-scrambled copy
//! of the base-2 Sobol (0,2)-sequence. Because every dimension is base 2 (unlike
//! Halton, whose high prime bases are badly under-sampled at low sample counts),
//! each decision is well stratified at any sample count and to any path depth,
//! and the per-decision scramble decorrelates them. It remains unbiased and is
//! deterministic per `(seed, x, y, sample_index)`.

use crate::Float;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

pub struct Sampler {
    rng: SmallRng,
    sample_index: u32,
    decision: u32,
    scramble: u64,
    qmc: bool,
}

impl Sampler {
    /// A quasi-Monte-Carlo sampler for sample `sample_index` of pixel `(x, y)`.
    pub fn pixel(seed: u64, x: usize, y: usize, sample_index: u32) -> Sampler {
        let scramble = hash(
            seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (y as u64).wrapping_mul(0xD1B5_4A32_D192_ED03),
        );
        let rng_seed = hash(scramble ^ (sample_index as u64).wrapping_add(0xA076_1D65));
        Sampler {
            rng: SmallRng::seed_from_u64(rng_seed),
            sample_index,
            decision: 0,
            scramble,
            qmc: true,
        }
    }

    /// A plain pseudo-random sampler (used in unit tests and as a fallback).
    pub fn random(seed: u64) -> Sampler {
        Sampler {
            rng: SmallRng::seed_from_u64(seed),
            sample_index: 0,
            decision: 0,
            scramble: 0,
            qmc: false,
        }
    }

    /// Like [`Sampler::pixel`] but using pure pseudo-random samples (white
    /// noise). Used to A/B the low-discrepancy sampler and as the `--no-qmc`
    /// path; still deterministic per `(seed, x, y, sample_index)`.
    pub fn pixel_random(seed: u64, x: usize, y: usize, sample_index: u32) -> Sampler {
        let mut s = Sampler::pixel(seed, x, y, sample_index);
        s.qmc = false;
        s
    }

    /// Next sample in `[0, 1)` for the next decision.
    #[inline]
    pub fn next_1d(&mut self) -> Float {
        if !self.qmc {
            return self.rng.gen::<Float>();
        }
        let d = self.decision;
        self.decision += 1;
        let idx = owen_scramble(self.sample_index, self.seed_for(d, 0));
        to_float(owen_scramble(sobol_x(idx), self.seed_for(d, 1)))
    }

    /// Next 2D sample in `[0, 1)²` — one Owen-scrambled Sobol (0,2) pair.
    #[inline]
    pub fn next_2d(&mut self) -> (Float, Float) {
        if !self.qmc {
            return (self.rng.gen::<Float>(), self.rng.gen::<Float>());
        }
        let d = self.decision;
        self.decision += 1;
        let idx = owen_scramble(self.sample_index, self.seed_for(d, 0));
        let x = to_float(owen_scramble(sobol_x(idx), self.seed_for(d, 1)));
        let y = to_float(owen_scramble(sobol_y(idx), self.seed_for(d, 2)));
        (x, y)
    }

    /// Distinct 32-bit scramble seed for a decision/role.
    #[inline]
    fn seed_for(&self, decision: u32, role: u32) -> u32 {
        hash(
            self.scramble
                ^ (decision as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (role as u64).wrapping_mul(0x2545_F491_4F6C_DD1D),
        ) as u32
    }
}

/// First Sobol dimension: van der Corput base 2 (bit reversal).
#[inline]
fn sobol_x(i: u32) -> u32 {
    i.reverse_bits()
}

/// Second Sobol dimension of the (0,2)-sequence.
#[inline]
fn sobol_y(mut i: u32) -> u32 {
    let mut r = 0u32;
    let mut v = 0x8000_0000u32;
    while i != 0 {
        if i & 1 == 1 {
            r ^= v;
        }
        v ^= v >> 1;
        i >>= 1;
    }
    r
}

/// Hash-based nested-uniform (Owen) scramble of a 32-bit value (Burley 2020).
#[inline]
fn owen_scramble(x: u32, seed: u32) -> u32 {
    let mut x = x.reverse_bits();
    x = x.wrapping_add(seed);
    x ^= x.wrapping_mul(0x6c50_b47c);
    x ^= x.wrapping_mul(0xb82f_1e52);
    x ^= x.wrapping_mul(0xc7af_e638);
    x ^= x.wrapping_mul(0x8d22_f6e6);
    x.reverse_bits()
}

/// Map the top 24 bits of a 32-bit word to a float in `[0, 1)`.
#[inline]
fn to_float(x: u32) -> Float {
    (x >> 8) as Float * (1.0 / (1u32 << 24) as Float)
}

/// SplitMix64 finalizer.
#[inline]
fn hash(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_in_unit_interval() {
        let mut s = Sampler::pixel(0, 3, 7, 5);
        for _ in 0..200 {
            let u = s.next_1d();
            assert!((0.0..1.0).contains(&u), "sample {u} out of range");
            let (a, b) = s.next_2d();
            assert!((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b));
        }
    }

    #[test]
    fn first_decision_is_well_stratified() {
        // The first 1D decision across sample_index 0..n should cover [0,1)
        // far more evenly than white noise: every length-1/8 bin ~ n/8.
        let n = 64usize;
        let mut counts = [0u32; 8];
        for i in 0..n {
            let mut s = Sampler::pixel(42, 1, 1, i as u32);
            let u = s.next_1d();
            counts[((u * 8.0) as usize).min(7)] += 1;
        }
        for c in counts {
            assert!((5..=11).contains(&c), "bin count {c} not well stratified");
        }
    }

    #[test]
    fn two_d_pair_fills_the_square() {
        // The first 2D decision across sample_index 0..n should hit every cell
        // of a coarse grid (a property of the (0,2)-sequence).
        let n = 256usize;
        let mut grid = [[0u32; 4]; 4];
        for i in 0..n {
            let mut s = Sampler::pixel(7, 2, 9, i as u32);
            let (x, y) = s.next_2d();
            grid[((x * 4.0) as usize).min(3)][((y * 4.0) as usize).min(3)] += 1;
        }
        for row in grid {
            for c in row {
                assert!(c > 0, "an entire grid cell was missed");
            }
        }
    }
}
