//! Deterministic engine RNG.
//!
//! Upstream uses libc `rand()`/`srand(time(NULL))`, which is neither portable nor deterministic.
//! The port uses an explicit xorshift64* generator seeded from the CLI (fixed default) so that
//! two runs with the same seed produce identical state hashes. The generator state is part of
//! the state hash.

/// Default seed used when `--seed` is not given.
pub const DEFAULT_SEED: u32 = 0x5EED_1234;

/// xorshift64* generator (Marsaglia / Vigna).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GameRng {
    state: u64,
}

impl GameRng {
    /// Creates a generator from a 32-bit seed. Zero seeds are remapped because xorshift is
    /// degenerate at zero.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let mut state = u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        if state == 0 {
            state = DEFAULT_SEED as u64;
        }
        Self { state }
    }

    /// The raw generator state, for hashing.
    #[must_use]
    pub fn state(self) -> u64 {
        self.state
    }

    /// Advances the generator and returns the next 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        (value.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// Upstream `rand() % max`; non-positive bounds return `0`.
    pub fn range(&mut self, max: i32) -> i32 {
        if max <= 0 {
            return 0;
        }
        (self.next_u32() % max as u32) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut first = GameRng::new(7);
        let mut second = GameRng::new(7);
        for _ in 0..16 {
            assert_eq!(first.next_u32(), second.next_u32());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut first = GameRng::new(1);
        let mut second = GameRng::new(2);
        assert_ne!(
            (0..8).map(|_| first.next_u32()).collect::<Vec<_>>(),
            (0..8).map(|_| second.next_u32()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn range_is_bounded_and_guards_non_positive() {
        let mut rng = GameRng::new(DEFAULT_SEED);
        for _ in 0..100 {
            let value = rng.range(10);
            assert!((0..10).contains(&value));
        }
        assert_eq!(rng.range(0), 0);
        assert_eq!(rng.range(-3), 0);
    }
}
