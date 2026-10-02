//! Deterministic engine RNG.
//!
//! Upstream uses libc `rand()` seeded with `srand(time(NULL))`. To keep runs reproducible while
//! matching the reference stream, the engine uses [`retro_core::rng::GlibcRand`] (a KAT-validated
//! clone of glibc's TYPE_3 generator) with a fixed default seed and a `--seed` override. The
//! generator state is part of the state hash.

pub use retro_core::rng::GlibcRand;

/// Default seed used when `--seed` is not given.
///
/// Upstream seeds from the wall clock, so any fixed value is a deliberate M3 divergence; this
/// one is arbitrary but stable across runs and platforms.
pub const DEFAULT_SEED: u32 = 0x5EED_1234;

/// Advances the glibc generator and applies upstream's `rand() % max` with a zero-max guard.
///
/// Upstream's `FUNC_RAND` computes `rand() % scriptEng.operands[1]` without a guard; a
/// non-positive bound would be undefined there. This returns `0` instead.
pub fn glibc_rand_range(rng: &mut GlibcRand, max: i32) -> i32 {
    if max <= 0 {
        return 0;
    }
    rng.next() % max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut first = GlibcRand::new(7);
        let mut second = GlibcRand::new(7);
        for _ in 0..16 {
            assert_eq!(first.next(), second.next());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut first = GlibcRand::new(1);
        let mut second = GlibcRand::new(2);
        assert_ne!(
            (0..8).map(|_| first.next()).collect::<Vec<_>>(),
            (0..8).map(|_| second.next()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn range_is_bounded_and_guards_non_positive() {
        // Known-answer: glibc `srand(1); rand() % 10` for the first values.
        let mut rng = GlibcRand::new(1);
        let expected = [
            1_804_289_383 % 10,
            846_930_886 % 10,
            1_681_692_777 % 10,
            1_714_636_915 % 10,
        ];
        for value in expected {
            assert_eq!(glibc_rand_range(&mut rng, 10), value);
        }
        assert_eq!(glibc_rand_range(&mut rng, 0), 0);
        assert_eq!(glibc_rand_range(&mut rng, -3), 0);
    }

    #[test]
    fn default_seed_is_stable() {
        let mut rng = GlibcRand::new(DEFAULT_SEED);
        assert_eq!(rng.next(), 611_332_365);
    }
}
