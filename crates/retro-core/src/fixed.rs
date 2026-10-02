//! 16.16 fixed-point helpers matching RSDK's `TO_FIXED`/`FROM_FIXED` macros.

/// Number of fractional bits in a 16.16 fixed-point value.
pub const FRAC_BITS: u32 = 16;

/// The 16.16 representation of `1.0`.
pub const ONE: i32 = 1 << FRAC_BITS;

/// Converts an integer to 16.16 fixed-point (wrapping like `x << 16`).
#[inline]
#[must_use]
pub const fn to_fixed(value: i32) -> i32 {
    value.wrapping_shl(FRAC_BITS)
}

/// Converts a 16.16 fixed-point value to an integer, truncating toward negative infinity.
#[inline]
#[must_use]
pub const fn from_fixed(value: i32) -> i32 {
    value >> FRAC_BITS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_rsdk() {
        assert_eq!(FRAC_BITS, 16);
        assert_eq!(ONE, 0x1_0000);
    }

    #[test]
    fn to_fixed_shifts_left() {
        assert_eq!(to_fixed(0), 0);
        assert_eq!(to_fixed(1), 0x1_0000);
        assert_eq!(to_fixed(3), 0x3_0000);
        assert_eq!(to_fixed(-1), -0x1_0000);
        assert_eq!(to_fixed(-3), -0x3_0000);
    }

    #[test]
    fn from_fixed_shifts_right_arithmetically() {
        assert_eq!(from_fixed(0), 0);
        assert_eq!(from_fixed(0x1_0000), 1);
        assert_eq!(from_fixed(-0x1_0000), -1);
    }

    #[test]
    fn from_fixed_truncates_toward_negative_infinity() {
        assert_eq!(from_fixed(0x1_7FFF), 1);
        assert_eq!(from_fixed(-1), -1);
        assert_eq!(from_fixed(-0x1), -1);
        assert_eq!(from_fixed(-0x1_8000), -2);
    }

    #[test]
    fn round_trips_representable_values() {
        for value in [0, 1, -1, 1234, -1234, i16::MAX as i32, i16::MIN as i32] {
            assert_eq!(from_fixed(to_fixed(value)), value);
        }
    }

    #[test]
    fn to_fixed_wraps_on_overflow_like_c() {
        assert_eq!(to_fixed(i32::MAX), -ONE);
    }

    #[test]
    fn usable_in_const_context() {
        const TWO: i32 = to_fixed(2);
        const HALF: i32 = from_fixed(ONE / 2);
        assert_eq!(TWO, 0x2_0000);
        assert_eq!(HALF, 0);
    }
}
