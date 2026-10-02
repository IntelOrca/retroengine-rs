//! Trigonometry lookup tables and wrappers, ported from `RSDKv4/Math.cpp`/`Math.hpp`
//! (RSDKModding/RSDKv4-Decompilation @ a7f5195).
//!
//! Upstream fills the tables once at startup with libm `sin`/`atan2`; this port computes the
//! same values in Rust `f64`. Results are deterministic for a given binary, which is what the
//! M3 state hashes require.

use std::f64::consts::PI;

/// Addressable sin/cos angle count for the 512-step tables.
pub const SIN512_COUNT: usize = 0x200;
/// Addressable sin/cos angle count for the 256-step tables.
pub const SIN256_COUNT: usize = 0x100;

/// The `sinM7`/`sin512`/`sin256`/`arcTan256` lookup tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTables {
    /// `sin512LookupTable`.
    pub sin512: [i32; SIN512_COUNT],
    /// `cos512LookupTable`.
    pub cos512: [i32; SIN512_COUNT],
    /// `sin256LookupTable`.
    pub sin256: [i32; SIN256_COUNT],
    /// `cos256LookupTable`.
    pub cos256: [i32; SIN256_COUNT],
    atan: Vec<u8>,
}

impl Default for MathTables {
    fn default() -> Self {
        Self::new()
    }
}

impl MathTables {
    /// Computes every table exactly like `CalculateTrigAngles`.
    #[must_use]
    pub fn new() -> Self {
        let mut sin512 = [0i32; SIN512_COUNT];
        let mut cos512 = [0i32; SIN512_COUNT];
        for index in 0..SIN512_COUNT {
            let fraction = index as f64 / 256.0;
            sin512[index] = ((fraction * PI).sin() * 512.0) as i32;
            cos512[index] = ((fraction * PI).cos() * 512.0) as i32;
        }
        cos512[0x00] = 0x200;
        cos512[0x80] = 0;
        cos512[0x100] = -0x200;
        cos512[0x180] = 0;
        sin512[0x00] = 0;
        sin512[0x80] = 0x200;
        sin512[0x100] = 0;
        sin512[0x180] = -0x200;

        let mut sin256 = [0i32; SIN256_COUNT];
        let mut cos256 = [0i32; SIN256_COUNT];
        for index in 0..SIN256_COUNT {
            sin256[index] = sin512[index * 2] >> 1;
            cos256[index] = cos512[index * 2] >> 1;
        }

        let mut atan = vec![0u8; 0x100 * 0x100];
        for y in 0..0x100usize {
            for x in 0..0x100usize {
                let angle = (y as f64).atan2(x as f64) as f32;
                atan[x * 0x100 + y] = (angle * 40.743_664_f32) as u8;
            }
        }

        Self {
            sin512,
            cos512,
            sin256,
            cos256,
            atan,
        }
    }

    /// `Sin512`.
    #[must_use]
    pub fn sin512(&self, angle: i32) -> i32 {
        let angle = if angle < 0 { 0x200 - angle } else { angle };
        let index = (angle & 0x1FF) as usize;
        self.sin512.get(index).copied().unwrap_or(0)
    }

    /// `Cos512`.
    #[must_use]
    pub fn cos512(&self, angle: i32) -> i32 {
        let angle = if angle < 0 { 0x200 - angle } else { angle };
        let index = (angle & 0x1FF) as usize;
        self.cos512.get(index).copied().unwrap_or(0)
    }

    /// `Sin256`.
    #[must_use]
    pub fn sin256(&self, angle: i32) -> i32 {
        let angle = if angle < 0 { 0x100 - angle } else { angle };
        let index = (angle & 0xFF) as usize;
        self.sin256.get(index).copied().unwrap_or(0)
    }

    /// `Cos256`.
    #[must_use]
    pub fn cos256(&self, angle: i32) -> i32 {
        let angle = if angle < 0 { 0x100 - angle } else { angle };
        let index = (angle & 0xFF) as usize;
        self.cos256.get(index).copied().unwrap_or(0)
    }

    /// `ArcTanLookup(X, Y)`.
    #[must_use]
    pub fn atan2(&self, x: i32, y: i32) -> i32 {
        let mut short_x = x.abs();
        let mut short_y = y.abs();
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
                .and_then(|index| self.atan.get(index).copied())
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
    fn atan2_cardinals() {
        let tables = MathTables::new();
        assert_eq!(tables.atan2(1, 0), 0);
        assert_eq!(tables.atan2(-1, 0), 0x80);
        assert_eq!(tables.atan2(1, 1), 0x20);
        assert_eq!(tables.atan2(-1, 1), 0x60);
        assert_eq!(tables.atan2(0, 0), 0x80);
        assert_eq!(tables.atan2(1, -1), 0xE0);
    }
}
