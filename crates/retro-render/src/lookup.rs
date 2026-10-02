//! The three software-renderer lookup tables from `GenerateBlendLookupTable` in `Drawing.cpp`.
//!
//! ```text
//! blendLookupTable[x + (0x20 * y)]    = y * x >> 8
//! subtractLookupTable[x + (0x20 * y)] = y * (0x1F - x) >> 8
//! tintLookupTable[i] = 0x841 * min((((i & 0x1F) + ((i & 0x7E0) >> 6) + ((i & 0xF800) >> 11)) / 3 + 6), 0x1F)
//! ```
//!
//! All arithmetic is integer (the division is C integer division), so the tables are exact.

/// Length of the blend and subtract tables (`0x20 * 0x100`).
pub const BLEND_TABLE_LEN: usize = 0x20 * 0x100;
/// Length of the tint table (`0x10000`).
pub const TINT_TABLE_LEN: usize = 0x10000;

/// The three lookup tables used by blend, subtract, tint and masked draw paths.
#[derive(Clone)]
pub struct LookupTables {
    /// `blendLookupTable`, indexed `x + 0x20 * y`.
    pub blend: Vec<u16>,
    /// `subtractLookupTable`, indexed `x + 0x20 * y`.
    pub subtract: Vec<u16>,
    /// `tintLookupTable`, indexed by RGB565 pixel.
    pub tint: Vec<u16>,
}

impl LookupTables {
    /// Generates the tables with the upstream formulas.
    #[must_use]
    pub fn new() -> Self {
        let mut blend = vec![0u16; BLEND_TABLE_LEN];
        let mut subtract = vec![0u16; BLEND_TABLE_LEN];
        for y in 0..0x100usize {
            for x in 0..0x20usize {
                blend[x + 0x20 * y] = ((y * x) >> 8) as u16;
                subtract[x + 0x20 * y] = ((y * (0x1F - x)) >> 8) as u16;
            }
        }
        let mut tint = vec![0u16; TINT_TABLE_LEN];
        for (i, slot) in tint.iter_mut().enumerate() {
            let value = i & 0x1F;
            let tint_value = (value + ((i & 0x7E0) >> 6) + ((i & 0xF800) >> 11)) / 3 + 6;
            *slot = 0x841 * tint_value.min(0x1F) as u16;
        }
        Self {
            blend,
            subtract,
            tint,
        }
    }

    /// `blendLookupTable[x + 0x20 * y]` with a slice bounds check.
    #[must_use]
    pub fn blend(&self, x: usize, y: usize) -> u16 {
        self.blend.get(x + 0x20 * y).copied().unwrap_or(0)
    }

    /// `subtractLookupTable[x + 0x20 * y]` with a slice bounds check.
    #[must_use]
    pub fn subtract(&self, x: usize, y: usize) -> u16 {
        self.subtract.get(x + 0x20 * y).copied().unwrap_or(0)
    }

    /// `tintLookupTable[pixel]` with a slice bounds check.
    #[must_use]
    pub fn tint(&self, pixel: u16) -> u16 {
        self.tint.get(pixel as usize).copied().unwrap_or(0)
    }
}

impl Default for LookupTables {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_known_answers() {
        let tables = LookupTables::new();
        // y * x >> 8.
        assert_eq!(tables.blend(0, 0), 0);
        assert_eq!(tables.blend(0x1F, 0xFF), (0xFF * 0x1F) >> 8);
        assert_eq!(tables.blend(0x10, 0x80), (0x80 * 0x10) >> 8);
        assert_eq!(tables.blend(0x1F, 0x40), (0x40 * 0x1F) >> 8); // 7
        assert_eq!(tables.blend(1, 0xFF), 0);
        assert_eq!(tables.blend(0x1F, 0xFF), 30);
    }

    #[test]
    fn subtract_known_answers() {
        let tables = LookupTables::new();
        assert_eq!(tables.subtract(0, 0xFF), (0xFF * 0x1F) >> 8); // 30
        assert_eq!(tables.subtract(0x1F, 0xFF), 0);
        assert_eq!(tables.subtract(0x0F, 0x80), (0x80 * (0x1F - 0x0F)) >> 8); // 10
        assert_eq!(tables.subtract(0x1F, 0x40), 0);
    }

    #[test]
    fn tint_known_answers() {
        let tables = LookupTables::new();
        // black: ((0+0+0)/3 + 6) = 6
        assert_eq!(tables.tint(0x0000), 0x841 * 6);
        // white: ((31+63+31)/3 + 6) = (125/3=41) + 6 = 47 -> clamped to 31
        assert_eq!(tables.tint(0xFFFF), 0x841 * 0x1F);
        // pure red 0xF800: ((0 + 0 + 31)/3 + 6) = 10+6 = 16
        assert_eq!(tables.tint(0xF800), 0x841 * 16);
        // pure green 0x07E0: ((0 + 31 + 0)/3 + 6) = 10+6 = 16
        assert_eq!(tables.tint(0x07E0), 0x841 * 16);
        // pure blue 0x001F: ((31 + 0 + 0)/3 + 6) = 10+6 = 16
        assert_eq!(tables.tint(0x001F), 0x841 * 16);
    }

    #[test]
    fn tables_have_upstream_lengths() {
        let tables = LookupTables::new();
        assert_eq!(tables.blend.len(), BLEND_TABLE_LEN);
        assert_eq!(tables.subtract.len(), BLEND_TABLE_LEN);
        assert_eq!(tables.tint.len(), TINT_TABLE_LEN);
    }

    #[test]
    fn out_of_range_indexes_are_zero() {
        let tables = LookupTables::new();
        assert_eq!(tables.blend(usize::MAX, 0), 0);
        assert_eq!(tables.tint(0), 0x841 * 6);
    }
}
