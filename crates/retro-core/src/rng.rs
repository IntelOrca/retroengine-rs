//! Random number generators: a glibc `rand()` (TYPE_3) clone and RSDKv5's seeded LCG.

/// Clone of glibc's `rand()`/`random()` generator (additive feedback, TYPE_3).
///
/// Matches the output stream produced by `srand(seed); rand()` on glibc.
#[derive(Clone, Debug)]
pub struct GlibcRand {
    state: [u32; DEG],
    fptr: usize,
    rptr: usize,
}

const DEG: usize = 31;
const SEP: usize = 3;

/// Number of state words in a [`GlibcRand`] snapshot.
pub const GLIBC_STATE_WORDS: usize = DEG;

impl GlibcRand {
    /// Seeds the generator exactly like glibc's `srandom`.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let seed = if seed == 0 { 1 } else { seed };
        let mut state = [0u32; DEG];
        state[0] = seed;
        let mut word = seed as i32 as i64;
        for slot in state.iter_mut().skip(1) {
            let hi = word / 127_773;
            let lo = word % 127_773;
            word = 16_807 * lo - 2836 * hi;
            if word < 0 {
                word += 2_147_483_647;
            }
            *slot = word as u32;
        }

        let mut rng = Self {
            state,
            fptr: SEP,
            rptr: 0,
        };
        for _ in 0..10 * DEG {
            rng.next();
        }
        rng
    }

    /// Produces the next value in the sequence, in the range `0..=i32::MAX`.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        let value = self.state[self.fptr].wrapping_add(self.state[self.rptr]);
        self.state[self.fptr] = value;
        self.fptr = (self.fptr + 1) % DEG;
        self.rptr = (self.rptr + 1) % DEG;
        (value >> 1) as i32
    }

    /// Returns the raw feedback state, for hashing and snapshots.
    #[must_use]
    pub fn state(&self) -> &[u32; GLIBC_STATE_WORDS] {
        &self.state
    }

    /// Returns the `(front, rear)` ring-buffer pointers, for hashing and snapshots.
    #[must_use]
    pub fn pointers(&self) -> (usize, usize) {
        (self.fptr, self.rptr)
    }
}

/// RSDKv5's seeded LCG with the 3-round xor output mix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandSeeded {
    seed: i32,
}

impl RandSeeded {
    /// Creates a generator with the given seed.
    #[must_use]
    pub const fn new(seed: i32) -> Self {
        Self { seed }
    }

    /// Returns the current seed.
    #[must_use]
    pub const fn seed(&self) -> i32 {
        self.seed
    }

    /// Replaces the current seed.
    pub fn set_seed(&mut self, seed: i32) {
        self.seed = seed;
    }

    /// Advances the LCG and returns the mixed 30-bit result.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        let seed1 = (self.seed as u32)
            .wrapping_mul(0x41c6_4e6d)
            .wrapping_add(0x3039);
        let seed2 = seed1.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        let seed3 = seed2.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        self.seed = seed3 as i32;
        (((((seed1 >> 16) & 0x7FF) << 10) ^ ((seed2 >> 16) & 0x7FF)) << 10
            ^ ((seed3 >> 16) & 0x7FF)) as i32
    }

    /// Advances the generator and maps the result into `[min, max)` like RSDK's `RandSeeded`.
    ///
    /// Unlike the upstream C macro, this never panics or overflows: when `min >= max` it returns
    /// `min` (upstream would divide by zero for `min == max` and rely on undefined signed
    /// overflow for extreme spans), and the span is computed in 64-bit wrapping arithmetic.
    #[inline]
    pub fn next_in(&mut self, min: i32, max: i32) -> i32 {
        let result = self.next();
        if min >= max {
            return min;
        }
        let span = (max as i64).wrapping_sub(min as i64) as u64;
        let offset = (result as u32 as u64) % span;
        min.wrapping_add(offset as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glibc_matches_srand_one_known_answers() {
        let mut rng = GlibcRand::new(1);
        let expected = [
            1_804_289_383,
            846_930_886,
            1_681_692_777,
            1_714_636_915,
            1_957_747_793,
            424_238_335,
            719_885_386,
            1_649_760_492,
            596_516_649,
            1_189_641_421,
        ];
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(rng.next(), value, "mismatch at output {index}");
        }
    }

    #[test]
    fn glibc_seed_zero_behaves_like_seed_one() {
        let mut zero = GlibcRand::new(0);
        let mut one = GlibcRand::new(1);
        for _ in 0..8 {
            assert_eq!(zero.next(), one.next());
        }
    }

    #[test]
    fn glibc_is_deterministic_and_seed_sensitive() {
        let mut a = GlibcRand::new(42);
        let mut b = GlibcRand::new(42);
        let mut c = GlibcRand::new(43);
        assert_eq!(a.next(), b.next());
        assert_ne!(a.next(), c.next());
    }

    #[test]
    fn glibc_values_are_non_negative_31_bit() {
        let mut rng = GlibcRand::new(1234);
        for _ in 0..64 {
            let value = rng.next();
            assert!((0..=i32::MAX).contains(&value));
        }
    }

    #[test]
    fn glibc_state_snapshot_tracks_the_stream() {
        let mut first = GlibcRand::new(7);
        let mut second = GlibcRand::new(7);
        assert_eq!(first.state(), second.state());
        assert_eq!(first.pointers(), second.pointers());
        let before = *first.state();
        first.next();
        assert_ne!(*first.state(), before);
        assert_ne!(first.state(), second.state());
        second.next();
        assert_eq!(first.state(), second.state());
        assert_eq!(first.pointers(), second.pointers());
    }

    #[test]
    fn rand_seeded_matches_rsdk_v5_known_answers() {
        let mut rng = RandSeeded::new(0);
        assert_eq!(rng.next(), 1_013_508);
        assert_eq!(rng.seed(), 2_802_067_423u32 as i32);
        assert_eq!(rng.next(), 1_715_907_103);
        assert_eq!(rng.next(), 1_791_259_482);
    }

    #[test]
    fn rand_seeded_seed_one_known_answers() {
        let mut rng = RandSeeded::new(1);
        assert_eq!(rng.next(), 477_757_313);
        assert_eq!(rng.seed(), 662_824_084);
        assert_eq!(rng.next(), 1_186_277_883);
        assert_eq!(rng.next(), 506_719_060);
    }

    #[test]
    fn rand_seeded_next_in_matches_rsdk_v5() {
        let mut rng = RandSeeded::new(1);
        assert_eq!(rng.next_in(0, 100), 13);
        assert_eq!(rng.seed(), 662_824_084);
    }

    #[test]
    fn rand_seeded_next_in_handles_reversed_and_equal_bounds() {
        let mut rng = RandSeeded::new(1);
        assert_eq!(rng.next_in(100, 0), 100);
        assert_eq!(rng.next_in(5, 5), 5);
        assert_eq!(rng.next_in(i32::MAX, i32::MIN), i32::MAX);
    }

    #[test]
    fn rand_seeded_next_in_handles_extreme_bounds() {
        let mut rng = RandSeeded::new(1);
        assert_eq!(rng.next_in(i32::MIN, i32::MAX), -1_669_726_335);
        let mut rng = RandSeeded::new(1);
        let value = rng.next_in(0, i32::MAX);
        assert_eq!(value, 477_757_313);
        let mut rng = RandSeeded::new(1);
        let value = rng.next_in(i32::MIN, 0);
        assert!(value < 0);
    }
}
