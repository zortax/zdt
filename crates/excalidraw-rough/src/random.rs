//! The generator a drawing's wobble comes from.
//!
//! rough.js draws with a Lehmer generator, and an element carries the seed it was made with, so
//! the same file draws the same way on every run and on every machine. The multiply is the one
//! detail that has to be exact: JavaScript's `Math.imul` is a 32-bit *signed* multiply that wraps,
//! and a generator that widened it would walk a different sequence from the first step.

/// The multiplier rough.js uses. Park and Miller's, with the modulus below.
const MULTIPLIER: i32 = 48271;

/// One drawing's wobble, from one seed.
#[derive(Clone, Copy, Debug)]
pub struct Random {
    /// The state, which is the last value drawn.
    seed: i32,
}

impl Random {
    /// The generator `seed` starts.
    ///
    /// A seed of zero is rough.js's word for "no seed", which draws differently every time. Every
    /// element carries a real one, so this answers a fixed sequence for zero rather than an
    /// unrepeatable one.
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self {
            seed: if seed == 0 { 1 } else { seed as i32 },
        }
    }

    /// The next number, in `[0, 1)`.
    #[allow(
        clippy::should_implement_trait,
        reason = "this is a generator, not an iterator"
    )]
    pub fn next(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER);
        f64::from((self.seed as u32) & 0x7fff_ffff) / 2_147_483_648.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence `Math.imul(48271, seed)` walks from one, read out of a browser.
    #[test]
    fn the_draws_match_the_javascript_generator() {
        let mut random = Random::new(1);
        let expected = [
            2.247_793_599_963_188_2e-5,
            0.085_032_448_638_230_56,
            0.601_328_216_027_468_4,
            0.714_315_861_929_208,
            0.740_971_184_801_310_3,
            0.420_061_544_049_531_2,
        ];
        for (at, expected) in expected.into_iter().enumerate() {
            let drawn = random.next();
            assert!(
                (drawn - expected).abs() < 1e-15,
                "draw {at} is {drawn}, and JavaScript says {expected}"
            );
        }
    }

    /// A seed large enough that the multiply wraps into the sign bit on the first step.
    #[test]
    fn a_wrapping_multiply_walks_the_javascript_sequence_too() {
        let mut random = Random::new(1_263_748_391);
        let expected = [
            0.455_452_535_767_108_2,
            0.149_354_014_080_017_8,
            0.467_613_656_539_469_96,
            0.178_814_816_754_311_32,
        ];
        for (at, expected) in expected.into_iter().enumerate() {
            let drawn = random.next();
            assert!(
                (drawn - expected).abs() < 1e-15,
                "draw {at} is {drawn}, and JavaScript says {expected}"
            );
        }
    }

    #[test]
    fn every_draw_is_inside_the_unit_interval() {
        let mut random = Random::new(1_263_748_391);
        for _ in 0..10_000 {
            let drawn = random.next();
            assert!((0.0..1.0).contains(&drawn), "{drawn} is outside [0, 1)");
        }
    }

    #[test]
    fn the_same_seed_walks_the_same_sequence() {
        let drawn = |seed| {
            let mut random = Random::new(seed);
            (0..8).map(|_| random.next()).collect::<Vec<_>>()
        };
        assert_eq!(drawn(884_517_263), drawn(884_517_263));
        assert_ne!(drawn(884_517_263), drawn(884_517_264));
    }
}
