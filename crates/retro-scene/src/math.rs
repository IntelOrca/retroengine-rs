//! Trigonometry accessors for the engine script host.
//!
//! The lookup tables are generated into `retro-core` by `tools/gen_math_tables.sh` from the
//! exact `RSDKv4/Math.cpp` formulas (`CalculateTrigAngles`). This module keeps the script-facing
//! [`MathTables`] API but is a thin delegate, so there is a single source of truth for the
//! tables (bit-exact on every host, not just this one).

use retro_core::math;

/// Addressable sin/cos angle count for the 512-step tables.
pub const SIN512_COUNT: usize = 0x200;
/// Addressable sin/cos angle count for the 256-step tables.
pub const SIN256_COUNT: usize = 0x100;

/// Script-facing trigonometry accessors.
///
/// The historical implementation owned the tables; it now delegates to
/// [`retro_core::math`]. The unit type keeps the call sites unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MathTables;

impl MathTables {
    /// Creates the accessor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// `Sin512`.
    #[must_use]
    pub fn sin512(&self, angle: i32) -> i32 {
        math::sin_512(angle)
    }

    /// `Cos512`.
    #[must_use]
    pub fn cos512(&self, angle: i32) -> i32 {
        math::cos_512(angle)
    }

    /// `Sin256`.
    #[must_use]
    pub fn sin256(&self, angle: i32) -> i32 {
        math::sin_256(angle)
    }

    /// `Cos256`.
    #[must_use]
    pub fn cos256(&self, angle: i32) -> i32 {
        math::cos_256(angle)
    }

    /// `ArcTanLookup(X, Y)`; returns the byte angle (`0..=255`).
    #[must_use]
    pub fn atan2(&self, x: i32, y: i32) -> i32 {
        math::arc_tan(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinal_angles_match_upstream_overrides() {
        let tables = MathTables::new();
        assert_eq!(tables.sin512(0), 0);
        assert_eq!(tables.sin512(0x80), 0x200);
        assert_eq!(tables.sin512(0x100), 0);
        assert_eq!(tables.sin512(0x180), -0x200);
        assert_eq!(tables.cos512(0), 0x200);
        assert_eq!(tables.cos512(0x80), 0);
        assert_eq!(tables.cos512(0x100), -0x200);
        assert_eq!(tables.cos512(0x180), 0);
    }

    #[test]
    fn sin256_is_half_the_512_table() {
        let tables = MathTables::new();
        for angle in 0..0x100i32 {
            assert_eq!(tables.sin256(angle), tables.sin512(angle * 2) >> 1);
        }
    }

    #[test]
    fn negative_angles_wrap_like_upstream() {
        let tables = MathTables::new();
        assert_eq!(tables.sin512(-0x40), tables.sin512(0x240));
        assert_eq!(tables.sin256(-0x20), tables.sin256(0x120));
    }

    #[test]
    fn atan2_matches_retro_core() {
        let tables = MathTables::new();
        assert_eq!(tables.atan2(1, 1), 0x20);
        assert_eq!(tables.atan2(-1, 1), 0x60);
        assert_eq!(tables.atan2(0, 0), 0x80);
        for y in [0i32, 1, 17, 255, 4096] {
            for x in [0i32, 1, 17, 255, 4096] {
                assert_eq!(tables.atan2(x, y), retro_core::math::arc_tan(x, y));
            }
        }
    }
}
