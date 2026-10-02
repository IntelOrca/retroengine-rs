//! RSDK v4-compatible sine/cosine lookup tables and accessors.

#[path = "math_tables.rs"]
mod tables;

pub use tables::{
    ARC_TAN_256_LOOKUP, COS_256_LOOKUP, COS_512_LOOKUP, COS_M7_LOOKUP, SIN_256_LOOKUP,
    SIN_512_LOOKUP, SIN_M7_LOOKUP,
};

/// `Sin256` equivalent: 256 entries per full turn, scaled by 256.
#[inline]
#[must_use]
pub fn sin_256(angle: i32) -> i32 {
    SIN_256_LOOKUP[(normalize(angle, 0x100)) as usize]
}

/// `Cos256` equivalent: 256 entries per full turn, scaled by 256.
#[inline]
#[must_use]
pub fn cos_256(angle: i32) -> i32 {
    COS_256_LOOKUP[(normalize(angle, 0x100)) as usize]
}

/// `Sin512` equivalent: 512 entries per full turn, scaled by 512.
#[inline]
#[must_use]
pub fn sin_512(angle: i32) -> i32 {
    SIN_512_LOOKUP[(normalize(angle, 0x200)) as usize]
}

/// `Cos512` equivalent: 512 entries per full turn, scaled by 512.
#[inline]
#[must_use]
pub fn cos_512(angle: i32) -> i32 {
    COS_512_LOOKUP[(normalize(angle, 0x200)) as usize]
}

/// `SinM7` equivalent: 512 entries per full turn, scaled by 4096.
#[inline]
#[must_use]
pub fn sin_m7(angle: i32) -> i32 {
    SIN_M7_LOOKUP[(normalize(angle, 0x200)) as usize]
}

/// `CosM7` equivalent: 512 entries per full turn, scaled by 4096.
#[inline]
#[must_use]
pub fn cos_m7(angle: i32) -> i32 {
    COS_M7_LOOKUP[(normalize(angle, 0x200)) as usize]
}

/// `ArcTanLookup` equivalent: returns the byte angle (`0..=255`) of `(X, Y)`.
///
/// Ported from `ArcTanLookup` in `RSDKv4/Math.cpp`; the table stores
/// `atan2f(Y, X) * 40.743664f` at `Y + X * 0x100`.
#[inline]
#[must_use]
pub fn arc_tan(x: i32, y: i32) -> i32 {
    let mut short_x = x.wrapping_abs();
    let mut short_y = y.wrapping_abs();
    if short_x <= short_y {
        while short_y > 0xFF {
            short_x >>= 4;
            short_y >>= 4;
        }
    } else {
        while short_x > 0xFF {
            short_x >>= 4;
            short_y >>= 4;
        }
    }
    let lookup = |x: i32, y: i32| {
        let index = x.wrapping_mul(0x100).wrapping_add(y);
        usize::try_from(index)
            .ok()
            .and_then(|index| ARC_TAN_256_LOOKUP.get(index).copied())
            .map(i32::from)
            .unwrap_or(0)
    };
    if x <= 0 {
        if y <= 0 {
            (lookup(short_x, short_y) - 0x80) & 0xFF
        } else {
            (-0x80 - lookup(short_x, short_y)) & 0xFF
        }
    } else if y <= 0 {
        (-lookup(short_x, short_y)) & 0xFF
    } else {
        lookup(short_x, short_y)
    }
}

#[inline]
fn normalize(angle: i32, size: i32) -> i32 {
    let angle = if angle < 0 {
        size.wrapping_sub(angle)
    } else {
        angle
    };
    angle & (size - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_expected_lengths() {
        assert_eq!(SIN_M7_LOOKUP.len(), 0x200);
        assert_eq!(COS_M7_LOOKUP.len(), 0x200);
        assert_eq!(SIN_512_LOOKUP.len(), 0x200);
        assert_eq!(COS_512_LOOKUP.len(), 0x200);
        assert_eq!(SIN_256_LOOKUP.len(), 0x100);
        assert_eq!(COS_256_LOOKUP.len(), 0x100);
    }

    #[test]
    fn cardinal_angles_256() {
        assert_eq!(sin_256(0), 0);
        assert_eq!(sin_256(0x40), 0x100);
        assert_eq!(sin_256(0x80), 0);
        assert_eq!(sin_256(0xC0), -0x100);
        assert_eq!(cos_256(0), 0x100);
        assert_eq!(cos_256(0x40), 0);
        assert_eq!(cos_256(0x80), -0x100);
        assert_eq!(cos_256(0xC0), 0);
    }

    #[test]
    fn cardinal_angles_512() {
        assert_eq!(sin_512(0), 0);
        assert_eq!(sin_512(0x80), 0x200);
        assert_eq!(sin_512(0x100), 0);
        assert_eq!(sin_512(0x180), -0x200);
        assert_eq!(cos_512(0), 0x200);
        assert_eq!(cos_512(0x80), 0);
        assert_eq!(cos_512(0x100), -0x200);
        assert_eq!(cos_512(0x180), 0);
    }

    #[test]
    fn cardinal_angles_m7() {
        assert_eq!(sin_m7(0), 0);
        assert_eq!(sin_m7(0x80), 0x1000);
        assert_eq!(sin_m7(0x100), 0);
        assert_eq!(sin_m7(0x180), -0x1000);
        assert_eq!(cos_m7(0), 0x1000);
        assert_eq!(cos_m7(0x80), 0);
        assert_eq!(cos_m7(0x100), -0x1000);
        assert_eq!(cos_m7(0x180), 0);
    }

    #[test]
    fn angles_wrap_around() {
        assert_eq!(sin_256(256), sin_256(0));
        assert_eq!(sin_256(0x40 + 0x100), sin_256(0x40));
        assert_eq!(cos_512(0x200 + 0x40), cos_512(0x40));
        assert_eq!(sin_m7(-0x80), sin_m7(0x80));
    }

    #[test]
    fn mixed_angles_match_tables() {
        for i in 0..0x100 {
            assert_eq!(sin_256(i), SIN_256_LOOKUP[i as usize]);
            assert_eq!(sin_512(i), SIN_512_LOOKUP[i as usize]);
            assert_eq!(sin_m7(i), SIN_M7_LOOKUP[i as usize]);
        }
    }

    #[test]
    fn arc_tan_cardinals() {
        assert_eq!(arc_tan(1, 0), 0);
        assert_eq!(arc_tan(-1, 0), 0x80);
        assert_eq!(arc_tan(1, 1), 0x20);
        assert_eq!(arc_tan(-1, 1), 0x60);
        assert_eq!(arc_tan(0, 0), 0x80);
        assert_eq!(arc_tan(1, -1), 0xE0);
        // Extreme inputs must not panic (upstream's `abs(INT_MIN)` is undefined behaviour).
        assert!((0..=255).contains(&arc_tan(i32::MIN, i32::MIN)));
        assert!((0..=255).contains(&arc_tan(i32::MAX, i32::MAX)));
        assert!((0..=255).contains(&arc_tan(i32::MIN, i32::MAX)));
    }

    #[test]
    fn arc_tan_matches_generated_table_formula() {
        for y in 0..0x100i32 {
            for x in 0..0x100i32 {
                let expected = ARC_TAN_256_LOOKUP[(y + x * 0x100) as usize];
                if x != 0 && y != 0 {
                    assert_eq!(arc_tan(x, y), i32::from(expected), "at ({x}, {y})");
                }
            }
        }
        // Downscaling large coordinates must land on the same table entry as the exact
        // `(x, y)` after the 4-bit shifts.
        assert_eq!(arc_tan(0x1234, 0x2345), arc_tan(0x12, 0x23));
    }

    #[test]
    fn extreme_angles_do_not_overflow() {
        assert_eq!(sin_256(i32::MIN), sin_256(0));
        assert_eq!(cos_256(i32::MIN), cos_256(0));
        assert_eq!(sin_512(i32::MIN), sin_512(0));
        assert_eq!(cos_512(i32::MIN), cos_512(0));
        assert_eq!(sin_m7(i32::MIN), sin_m7(0));
        assert_eq!(cos_m7(i32::MIN), cos_m7(0));
        assert_eq!(sin_256(i32::MAX), sin_256(i32::MAX & 0xFF));
        assert_eq!(
            sin_m7(i32::MIN + 1),
            sin_m7((0x200i32.wrapping_sub(i32::MIN + 1)) & 0x1FF)
        );
    }
}
