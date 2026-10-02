//! RSDK v4-compatible sine/cosine lookup tables and accessors.

#[path = "math_tables.rs"]
mod tables;

pub use tables::{
    COS_256_LOOKUP, COS_512_LOOKUP, COS_M7_LOOKUP, SIN_256_LOOKUP, SIN_512_LOOKUP, SIN_M7_LOOKUP,
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
