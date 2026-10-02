//! Runtime palette banks and the per-scanline palette line buffer.
//!
//! Ports `Palette.cpp`/`Palette.hpp` exactly: eight palette banks of 256 colours, kept in both
//! RGB888 (`fullPalette32`) and RGB565 (`fullPalette`) form. `gfxLineBuffer` selects the bank
//! used for each output scanline; most draw routines fetch the bank per row so palette scrolls
//! and fades are visible.

use crate::SCREEN_HEIGHT;
use crate::framebuffer::rgb888_to_rgb565;

/// Number of palette banks (`PALETTE_COUNT`).
pub const PALETTE_BANKS: usize = 0x8;
/// Number of colours per bank (`PALETTE_COLOR_COUNT`).
pub const PALETTE_COLORS: usize = 0x100;
/// Sentinel palette id meaning "the active bank" (`-1`; upstream's `byte` parameter makes
/// `0xFF` equivalent, so both are accepted).
pub const ACTIVE_PALETTE: i32 = -1;
/// The byte-truncated form of [`ACTIVE_PALETTE`], as produced by the script ops.
pub const ACTIVE_PALETTE_BYTE: i32 = 0xFF;

/// Whether a palette id selects the active bank (`-1` or its byte truncation `0xFF`).
#[must_use]
pub const fn is_active_palette(palette_index: i32) -> bool {
    palette_index == ACTIVE_PALETTE || palette_index == ACTIVE_PALETTE_BYTE
}

/// Eight RGB565/RGB888 palette banks plus the per-line bank selection.
#[derive(Clone)]
pub struct PaletteState {
    /// RGB888 banks (`fullPalette32`).
    pub rgb: Box<[[[u8; 3]; PALETTE_COLORS]; PALETTE_BANKS]>,
    /// RGB565 banks (`fullPalette`).
    pub rgb565: Box<[[u16; PALETTE_COLORS]; PALETTE_BANKS]>,
    /// Per-scanline palette bank (`gfxLineBuffer`).
    pub line_buffer: [u8; SCREEN_HEIGHT],
}

impl PaletteState {
    /// Creates eight zeroed banks with every scanline selecting bank 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rgb: Box::new([[[0, 0, 0]; PALETTE_COLORS]; PALETTE_BANKS]),
            rgb565: Box::new([[0; PALETTE_COLORS]; PALETTE_BANKS]),
            line_buffer: [0; SCREEN_HEIGHT],
        }
    }

    /// `SetPaletteEntry`: writes one colour into `palette_index`, or into the active bank when
    /// `palette_index` is [`ACTIVE_PALETTE`].
    pub fn set_entry(&mut self, palette_index: i32, index: usize, r: u8, g: u8, b: u8) {
        if index >= PALETTE_COLORS {
            return;
        }
        if is_active_palette(palette_index) {
            let bank = usize::from(self.line_buffer.first().copied().unwrap_or(0));
            self.set_bank_entry(bank, index, r, g, b);
        } else if let Ok(bank) = usize::try_from(palette_index) {
            self.set_bank_entry(bank, index, r, g, b);
        }
    }

    /// Writes one colour into a concrete bank, ignoring out-of-range banks.
    pub fn set_bank_entry(&mut self, bank: usize, index: usize, r: u8, g: u8, b: u8) {
        if bank >= PALETTE_BANKS || index >= PALETTE_COLORS {
            return;
        }
        self.rgb[bank][index] = [r, g, b];
        self.rgb565[bank][index] = rgb888_to_rgb565(r, g, b);
    }

    /// `SetPaletteEntryPacked`: writes a packed `0xRRGGBB` colour.
    pub fn set_entry_packed(&mut self, palette_index: i32, index: usize, color: u32) {
        self.set_entry(
            palette_index,
            index,
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
        );
    }

    /// `GetPaletteEntryPacked`: reads a packed `0xRRGGBB` colour, or `0` when out of range.
    #[must_use]
    pub fn get_entry_packed(&self, palette_index: i32, index: usize) -> u32 {
        let Some(bank) = self.bank(palette_index) else {
            return 0;
        };
        let Some(color) = self.rgb.get(bank).and_then(|bank| bank.get(index)) else {
            return 0;
        };
        (u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2])
    }

    /// Resolves a palette id (`-1` means active) to a bank index.
    #[must_use]
    pub fn bank(&self, palette_index: i32) -> Option<usize> {
        let bank = if is_active_palette(palette_index) {
            i32::from(self.line_buffer.first().copied().unwrap_or(0))
        } else {
            palette_index
        };
        usize::try_from(bank)
            .ok()
            .filter(|bank| *bank < PALETTE_BANKS)
    }

    /// `CopyPalette`: copies `count` colours between banks.
    pub fn copy_palette(
        &mut self,
        source_palette: i32,
        src_start: usize,
        destination_palette: i32,
        dest_start: usize,
        count: usize,
    ) {
        let (Some(source), Some(destination)) =
            (self.bank(source_palette), self.bank(destination_palette))
        else {
            return;
        };
        for offset in 0..count {
            let (Some(src), Some(dst)) = (
                src_start.checked_add(offset),
                dest_start.checked_add(offset),
            ) else {
                break;
            };
            if src >= PALETTE_COLORS || dst >= PALETTE_COLORS {
                break;
            }
            self.rgb565[destination][dst] = self.rgb565[source][src];
            self.rgb[destination][dst] = self.rgb[source][src];
        }
    }

    /// `RotatePalette`: rotates `start..=end` in `pal_id`, right (towards higher indices) when
    /// `right` is set.
    pub fn rotate_palette(
        &mut self,
        pal_id: i32,
        start_index: usize,
        end_index: usize,
        right: bool,
    ) {
        let Some(bank) = self.bank(pal_id) else {
            return;
        };
        if start_index >= end_index || end_index >= PALETTE_COLORS {
            return;
        }
        if right {
            let first_color = self.rgb565[bank][end_index];
            let first_color32 = self.rgb[bank][end_index];
            for i in (start_index + 1..=end_index).rev() {
                self.rgb565[bank][i] = self.rgb565[bank][i - 1];
                self.rgb[bank][i] = self.rgb[bank][i - 1];
            }
            self.rgb565[bank][start_index] = first_color;
            self.rgb[bank][start_index] = first_color32;
        } else {
            let first_color = self.rgb565[bank][start_index];
            let first_color32 = self.rgb[bank][start_index];
            for i in start_index..end_index {
                self.rgb565[bank][i] = self.rgb565[bank][i + 1];
                self.rgb[bank][i] = self.rgb[bank][i + 1];
            }
            self.rgb565[bank][end_index] = first_color;
            self.rgb[bank][end_index] = first_color32;
        }
    }

    /// `SetPaletteFade` (rev01+): fades `dest` between `src_a` and `src_b` by `blend_amount`.
    ///
    /// Upstream iterates `startIndex..=endIndex` inclusive on the destination; that quirk is
    /// preserved here.
    pub fn set_palette_fade(
        &mut self,
        dest_palette: i32,
        src_a: i32,
        src_b: i32,
        blend_amount: u16,
        start_index: usize,
        end_index: usize,
    ) {
        let (Some(dest), Some(a), Some(b)) =
            (self.bank(dest_palette), self.bank(src_a), self.bank(src_b))
        else {
            return;
        };
        let blend_amount = blend_amount.min(0xFF) as u32;
        if start_index >= end_index || end_index >= PALETTE_COLORS {
            return;
        }
        let blend_a = 0xFF - blend_amount;
        for index in start_index..=end_index {
            let source_b = self.rgb[b][index];
            let source_a = self.rgb[a][index];
            let r = ((u32::from(source_b[0]) * blend_amount + blend_a * u32::from(source_a[0]))
                >> 8) as u8;
            let g = ((u32::from(source_b[1]) * blend_amount + blend_a * u32::from(source_a[1]))
                >> 8) as u8;
            let bl = ((u32::from(source_b[2]) * blend_amount + blend_a * u32::from(source_a[2]))
                >> 8) as u8;
            self.rgb[dest][index] = [r, g, bl];
            self.rgb565[dest][index] = rgb888_to_rgb565(r, g, bl);
        }
    }

    /// `SetActivePalette`: points scanlines `start_line..end_line` at `bank`.
    pub fn set_active_palette(&mut self, bank: i32, start_line: i32, end_line: i32) {
        let Ok(bank) = usize::try_from(bank) else {
            return;
        };
        if bank >= PALETTE_BANKS {
            return;
        }
        let start = start_line.max(0) as usize;
        let end = (end_line.max(0) as usize).min(SCREEN_HEIGHT);
        for line in start.min(SCREEN_HEIGHT)..end {
            self.line_buffer[line] = bank as u8;
        }
    }

    /// Fills `bank` entries starting at `start` from an RGB888 table, stopping at either end.
    pub fn set_bank_entries(&mut self, bank: i32, start: usize, colors: &[[u8; 3]]) {
        for (offset, color) in colors.iter().enumerate() {
            let Some(index) = start.checked_add(offset) else {
                break;
            };
            if index >= PALETTE_COLORS {
                break;
            }
            self.set_entry(bank, index, color[0], color[1], color[2]);
        }
    }

    /// The bank selected for scanline `line`.
    #[must_use]
    pub fn line_bank(&self, line: usize) -> usize {
        usize::from(self.line_buffer.get(line).copied().unwrap_or(0))
    }

    /// The RGB565 bank selected for scanline `line`.
    #[must_use]
    pub fn line_colors(&self, line: usize) -> &[u16; PALETTE_COLORS] {
        &self.rgb565[self.line_bank(line)]
    }

    /// The bank selected by scanline 0 (`activePalette`).
    #[must_use]
    pub fn active_colors(&self) -> &[u16; PALETTE_COLORS] {
        self.line_colors(0)
    }
}

impl Default for PaletteState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_entry_writes_active_bank() {
        let mut palette = PaletteState::new();
        palette.set_active_palette(3, 0, SCREEN_HEIGHT as i32);
        palette.set_entry(ACTIVE_PALETTE, 5, 255, 0, 0);
        assert_eq!(palette.rgb[3][5], [255, 0, 0]);
        assert_eq!(palette.rgb565[3][5], 0xF800);
        assert_eq!(palette.rgb[0][5], [0, 0, 0]);
    }

    #[test]
    fn active_palette_sentinel_accepts_minus_one_and_ff() {
        let mut palette = PaletteState::new();
        palette.set_active_palette(4, 0, SCREEN_HEIGHT as i32);
        palette.set_entry(ACTIVE_PALETTE, 1, 1, 2, 3);
        palette.set_entry(ACTIVE_PALETTE_BYTE, 2, 4, 5, 6);
        palette.set_entry_packed(ACTIVE_PALETTE_BYTE, 3, 0x0A_0B_0C);
        assert_eq!(palette.rgb[4][1], [1, 2, 3]);
        assert_eq!(palette.rgb[4][2], [4, 5, 6]);
        assert_eq!(palette.get_entry_packed(ACTIVE_PALETTE, 3), 0x0A_0B_0C);
        assert_eq!(palette.get_entry_packed(ACTIVE_PALETTE_BYTE, 3), 0x0A_0B_0C);
        assert!(palette.get_entry_packed(5, 1) == 0, "bank 5 stays empty");
        assert_eq!(palette.bank(ACTIVE_PALETTE_BYTE), Some(4));
        assert_eq!(palette.bank(ACTIVE_PALETTE), Some(4));
    }

    #[test]
    fn active_palette_packs_and_reads_back() {
        let mut palette = PaletteState::new();
        palette.set_entry_packed(2, 7, 0x12_34_56);
        assert_eq!(palette.get_entry_packed(2, 7), 0x12_34_56);
        assert_eq!(palette.rgb[2][7], [0x12, 0x34, 0x56]);
        assert_eq!(palette.get_entry_packed(3, 7), 0);
        assert_eq!(palette.get_entry_packed(2, 999), 0);
    }

    #[test]
    fn copy_palette_copies_rgb_and_rgb565() {
        let mut palette = PaletteState::new();
        palette.set_entry(0, 10, 1, 2, 3);
        palette.set_entry(0, 11, 4, 5, 6);
        palette.copy_palette(0, 10, 1, 20, 2);
        assert_eq!(palette.rgb[1][20], [1, 2, 3]);
        assert_eq!(palette.rgb[1][21], [4, 5, 6]);
        assert_eq!(palette.rgb565[1][20], rgb888_to_rgb565(1, 2, 3));
    }

    #[test]
    fn rotate_palette_moves_entries_both_ways() {
        let mut palette = PaletteState::new();
        for index in 0..4 {
            palette.set_entry(0, index, index as u8, 0, 0);
        }
        palette.rotate_palette(0, 0, 3, false);
        assert_eq!(
            (0..4)
                .map(|index| palette.rgb[0][index][0])
                .collect::<Vec<_>>(),
            [1, 2, 3, 0]
        );
        palette.rotate_palette(0, 0, 3, true);
        assert_eq!(
            (0..4)
                .map(|index| palette.rgb[0][index][0])
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn palette_fade_matches_upstream_arithmetic() {
        let mut palette = PaletteState::new();
        palette.set_entry(0, 0, 0, 0, 0);
        palette.set_entry(1, 0, 255, 255, 255);
        palette.set_palette_fade(2, 0, 1, 0x80, 0, 1);
        // 0x80/0x100 of white = 127 (truncated); index 1 is the inclusive tail and both
        // sources are black there.
        assert_eq!(palette.rgb[2][0], [127, 127, 127]);
        assert_eq!(palette.rgb[2][1], [0, 0, 0]);
        assert_eq!(palette.rgb565[2][0], rgb888_to_rgb565(127, 127, 127));
    }

    #[test]
    fn palette_fade_clamps_blend_amount() {
        let mut palette = PaletteState::new();
        palette.set_entry(1, 5, 10, 20, 30);
        palette.set_palette_fade(0, 0, 1, 0xFFFF, 5, 6);
        assert_eq!(palette.rgb[0][5], [9, 19, 29]);
    }

    #[test]
    fn set_active_palette_clamps_lines() {
        let mut palette = PaletteState::new();
        palette.set_active_palette(1, -5, 3);
        assert_eq!(palette.line_buffer[0], 1);
        assert_eq!(palette.line_buffer[2], 1);
        assert_eq!(palette.line_buffer[3], 0);
        palette.set_active_palette(9, 0, 3);
        assert_eq!(palette.line_buffer[0], 1);
        palette.set_active_palette(1, 0, i32::MAX);
        assert_eq!(palette.line_buffer[SCREEN_HEIGHT - 1], 1);
    }

    #[test]
    fn set_bank_entries_stops_at_the_bank_end() {
        let mut palette = PaletteState::new();
        let colors = vec![[1, 2, 3]; 4];
        palette.set_bank_entries(0, PALETTE_COLORS - 2, &colors);
        assert_eq!(palette.rgb[0][PALETTE_COLORS - 1], [1, 2, 3]);
    }
}
