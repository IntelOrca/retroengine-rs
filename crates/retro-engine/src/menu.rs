//! Legacy v4 text-menu operations (`SetupTextMenu`/`AddTextMenuEntry`/`EditTextMenuEntry` and
//! `DrawTextMenu`).
//!
//! Upstream lives in `Storage/Legacy/TextLegacy.cpp:174-238` and
//! `Graphics/Legacy/DrawingLegacy.cpp:3068-3245`, dispatched from `ScriptLegacyv4.cpp:4385-4390,
//! 4660-4680`. The fixed C arrays are `Vec`s here, so every upstream unchecked index is guarded;
//! the shipped scripts only ever use in-range rows, so the visible output is unchanged.
//!
//! Upstream quirks kept intact:
//!
//! * `FUNC_ADDMENUENTRY` writes the script's highlight operand *before* `AddTextMenuEntry`, which
//!   immediately overwrites it with `false` (`ScriptLegacyv4.cpp:4667-4672`,
//!   `TextLegacy.cpp:179-196`); [`TextMenu::add_entry`] therefore ignores its `highlight`.
//! * `EditTextMenuEntry` reads `entryStart[rowID]` and writes `entryHighlight[rowCount]` (not
//!   `rowID`); [`TextMenu::edit_entry`] reproduces both, guarded.
//! * `DrawTextMenuEntry` shifts odd-length centred entries by four pixels
//!   (`- (((entrySize % 2) & (alignment == 2)) * 4)`).
//! * `DrawStageTextEntry` draws its last glyph without the highlight row.
//! * `DrawTextMenu` back-scans for `selection2` only when `selectionCount == 3`, draws nothing
//!   for any other `selectionCount`, and only reads `visibleRowCount`.

use retro_render::RenderState;

use crate::state::{TEXT_DATA_COUNT, TEXT_ENTRY_COUNT, TextMenu};

/// `MENU_ALIGN_LEFT` (`TextLegacy.hpp:33-37`).
pub const MENU_ALIGN_LEFT: i32 = 0;
/// `MENU_ALIGN_RIGHT`.
pub const MENU_ALIGN_RIGHT: i32 = 1;
/// `MENU_ALIGN_CENTER`.
pub const MENU_ALIGN_CENTER: i32 = 2;

impl TextMenu {
    /// `FUNC_SETUPMENU` (`ScriptLegacyv4.cpp:4660-4666`): `SetupTextMenu` plus the
    /// `selectionCount`/`alignment` writes.
    pub fn setup(&mut self, row_count: i32, selection_count: i32, alignment: i32) {
        // `SetupTextMenu` only resets the write position and row count
        // (`TextLegacy.cpp:174-178`); the fixed C arrays keep their old contents.
        self.text_data_pos = 0;
        self.row_count = row_count;
        self.selection_count = selection_count;
        self.alignment = alignment;
    }

    /// `FUNC_ADDMENUENTRY` (`ScriptLegacyv4.cpp:4667-4672`): `add_entry` receives the script text
    /// and the `operands[2]` highlight.
    pub fn add_entry(&mut self, text: &str, highlight: i32) {
        // `FUNC_ADDMENUENTRY` pre-writes `entryHighlight[rowCount] = operands[2]`, then
        // `AddTextMenuEntry` overwrites it with false (`TextLegacy.cpp:185`), so the operand has
        // no lasting effect.
        let _ = highlight;
        let row = usize::try_from(self.row_count).unwrap_or(usize::MAX);
        let in_range = row < TEXT_ENTRY_COUNT;
        if in_range {
            self.ensure_row(row);
            self.entry_start[row] = self.text_data_pos as i32;
            self.entry_size[row] = 0;
            self.entry_highlight[row] = 0;
        }
        // `AddTextMenuEntry` indexes `textData[textDataPos++]` directly; the cap only replaces
        // upstream's out-of-bounds write.
        for byte in text.bytes() {
            let position = self.text_data_pos;
            if position < TEXT_DATA_COUNT {
                if self.text_data.len() <= position {
                    self.text_data.resize(position + 1, 0);
                }
                self.text_data[position] = u16::from(byte);
            }
            self.text_data_pos += 1;
            if in_range {
                self.entry_size[row] = self.entry_size[row].wrapping_add(1);
            }
        }
        self.row_count = self.row_count.wrapping_add(1);
    }

    /// `FUNC_EDITMENUENTRY` (`ScriptLegacyv4.cpp:4673-4678`): `EditTextMenuEntry` for `row_id`,
    /// then `entryHighlight[row_id] = highlight`.
    pub fn edit_entry(&mut self, text: &str, row_id: i32, highlight: i32) {
        let row = usize::try_from(row_id).unwrap_or(usize::MAX);
        if row < TEXT_ENTRY_COUNT {
            // `EditTextMenuEntry` reads `entryStart[rowID]` unbounded (`TextLegacy.cpp:218`);
            // out-of-range rows are ignored here instead of reading stale memory.
            let entry_pos = self.entry_start.get(row).copied().unwrap_or(0);
            self.entry_size[row] = 0;
            // Upstream's stray write is `entryHighlight[rowCount]`, not `entryHighlight[rowID]`.
            let count = usize::try_from(self.row_count).unwrap_or(usize::MAX);
            if count < TEXT_ENTRY_COUNT {
                self.ensure_row(count);
                self.entry_highlight[count] = 0;
            }
            // The replacement is written in place at `entryPos`, advancing while `textDataPos`
            // is left untouched; longer replacements overwrite later rows' characters exactly
            // like upstream.
            if let Ok(mut position) = usize::try_from(entry_pos) {
                for byte in text.bytes() {
                    if position >= TEXT_DATA_COUNT {
                        break;
                    }
                    if self.text_data.len() <= position {
                        self.text_data.resize(position + 1, 0);
                    }
                    self.text_data[position] = u16::from(byte);
                    position += 1;
                    self.entry_size[row] = self.entry_size[row].wrapping_add(1);
                }
            }
            // `FUNC_EDITMENUENTRY` then writes the highlight over the row (`uint8` truncation).
            self.entry_highlight[row] = highlight as u8;
        }
    }

    /// `FUNC_DRAWMENU` (`ScriptLegacyv4.cpp:4385-4390`): sets `textMenuSurfaceNo` to the
    /// object's sheet, then `DrawTextMenu(menu, x, y)` (`DrawingLegacy.cpp:3120-3245`).
    pub fn draw(&mut self, render: &mut RenderState, x: i32, y: i32, sheet: i32) {
        let end = if self.visible_row_count > 0 {
            self.visible_row_count.wrapping_add(self.visible_row_offset)
        } else {
            self.visible_row_offset = 0;
            self.row_count
        };

        if self.selection_count == 3 {
            self.selection2 = -1;
            // `for (i = 0; i < selection1 + 1; ++i) if (entryHighlight[i]) selection2 = i;`
            let scan_end = self.selection1.min(TEXT_ENTRY_COUNT as i32 - 1);
            for row in 0..=scan_end {
                let highlighted = usize::try_from(row)
                    .ok()
                    .and_then(|row| self.entry_highlight.get(row))
                    .copied()
                    .unwrap_or(0);
                if highlighted != 0 {
                    self.selection2 = row;
                }
            }
        }

        // Upstream iterates `visibleRowOffset..cnt`; the cap only guards the fixed arrays (a
        // script-set count beyond them reads stale memory upstream).
        let first = self.visible_row_offset;
        let rows = end.saturating_sub(first).clamp(0, TEXT_ENTRY_COUNT as i32);
        let mut y_pos = y;
        for offset in 0..rows {
            let row = first.saturating_add(offset);
            match self.alignment {
                MENU_ALIGN_RIGHT => {
                    let entry_x = x - self.entry_size_at(row).wrapping_shl(3);
                    self.draw_row(render, sheet, row, entry_x, y_pos);
                }
                MENU_ALIGN_CENTER => {
                    let entry_x = x - self.entry_size_at(row).wrapping_shr(1).wrapping_shl(3);
                    self.draw_row(render, sheet, row, entry_x, y_pos);
                }
                MENU_ALIGN_LEFT => self.draw_row(render, sheet, row, x, y_pos),
                _ => {}
            }
            y_pos = y_pos.wrapping_add(8);
        }
    }

    /// `DrawTextMenu`'s per-row selection switch (`DrawingLegacy.cpp:3125-3216`).
    fn draw_row(&self, render: &mut RenderState, sheet: i32, row: i32, x: i32, y: i32) {
        match self.selection_count {
            1 => {
                let highlight = if row == self.selection1 { 128 } else { 0 };
                self.draw_entry(render, sheet, row, x, y, highlight);
            }
            2 => {
                let highlight = if row == self.selection1 || row == self.selection2 {
                    128
                } else {
                    0
                };
                self.draw_entry(render, sheet, row, x, y, highlight);
            }
            3 => {
                let highlight = if row == self.selection1 { 128 } else { 0 };
                self.draw_entry(render, sheet, row, x, y, highlight);
                if row == self.selection2 && row != self.selection1 {
                    self.draw_stage_entry(render, sheet, row, x, y, 128);
                }
            }
            _ => {}
        }
    }

    /// `DrawTextMenuEntry` (`DrawingLegacy.cpp:3068-3076`).
    fn draw_entry(
        &self,
        render: &mut RenderState,
        sheet: i32,
        row_id: i32,
        x_pos: i32,
        y_pos: i32,
        text_highlight: i32,
    ) {
        let Some(row) = usize::try_from(row_id).ok() else {
            return;
        };
        let Some(&start) = self.entry_start.get(row) else {
            return;
        };
        let size = self.entry_size.get(row).copied().unwrap_or(0);
        // `- (((entrySize % 2) & (alignment == 2)) * 4)`: odd-length centred entries are shifted
        // one half glyph left.
        let centered = ((size % 2) & i32::from(self.alignment == MENU_ALIGN_CENTER)) * 4;
        for index in 0..size {
            let Some(character) = start
                .checked_add(index)
                .and_then(|position| usize::try_from(position).ok())
                .and_then(|position| self.text_data.get(position))
                .copied()
            else {
                continue;
            };
            render.draw_sprite(
                sheet,
                x_pos + (index << 3) - centered,
                y_pos,
                8,
                8,
                i32::from((character & 0xF) << 3),
                i32::from((character >> 4) << 3) + text_highlight,
            );
        }
    }

    /// `DrawStageTextEntry` (`DrawingLegacy.cpp:3078-3094`): the last glyph is always drawn
    /// without the highlight row.
    fn draw_stage_entry(
        &self,
        render: &mut RenderState,
        sheet: i32,
        row_id: i32,
        x_pos: i32,
        y_pos: i32,
        text_highlight: i32,
    ) {
        let Some(row) = usize::try_from(row_id).ok() else {
            return;
        };
        let Some(&start) = self.entry_start.get(row) else {
            return;
        };
        let size = self.entry_size.get(row).copied().unwrap_or(0);
        let last = size - 1;
        for index in 0..size {
            let Some(character) = start
                .checked_add(index)
                .and_then(|position| usize::try_from(position).ok())
                .and_then(|position| self.text_data.get(position))
                .copied()
            else {
                continue;
            };
            let highlight = if index == last { 0 } else { text_highlight };
            render.draw_sprite(
                sheet,
                x_pos + (index << 3),
                y_pos,
                8,
                8,
                i32::from((character & 0xF) << 3),
                i32::from((character >> 4) << 3) + highlight,
            );
        }
    }

    /// `entrySize[row]` with upstream's stale/zero fallback for out-of-range rows.
    fn entry_size_at(&self, row: i32) -> i32 {
        usize::try_from(row)
            .ok()
            .and_then(|row| self.entry_size.get(row))
            .copied()
            .unwrap_or(0)
    }

    /// Grows the parallel row arrays to `row + 1` entries (the C arrays are fixed and zeroed).
    fn ensure_row(&mut self, row: usize) {
        if self.entry_start.len() <= row {
            self.entry_start.resize(row + 1, 0);
        }
        if self.entry_size.len() <= row {
            self.entry_size.resize(row + 1, 0);
        }
        if self.entry_highlight.len() <= row {
            self.entry_highlight.resize(row + 1, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_render::Surface;

    const RED: u16 = 0xF800;
    const GREEN: u16 = 0x07E0;

    /// A screen whose sheet 0 is 128x256: source rows 0..15 are palette 1 (red) and rows
    /// 16..31 are palette 2 (green), so a glyph's draw position reveals whether it was drawn
    /// with the `+128` highlight row.
    fn test_render() -> RenderState {
        let mut render = RenderState::new(424, 240);
        let mut pixels = vec![0u8; 128 * 256];
        for y in 0..128 {
            for x in 0..128 {
                pixels[y * 128 + x] = 1;
            }
        }
        for y in 128..256 {
            for x in 0..128 {
                pixels[y * 128 + x] = 2;
            }
        }
        render
            .surfaces
            .push(Surface::from_indexed(128, 256, pixels));
        render.palette.set_bank_entry(0, 1, 255, 0, 0);
        render.palette.set_bank_entry(0, 2, 0, 255, 0);
        render
    }

    fn menu_with(rows: &[&str], alignment: i32, selection_count: i32) -> TextMenu {
        let mut menu = TextMenu::default();
        menu.setup(0, selection_count, alignment);
        for row in rows {
            menu.add_entry(row, 0);
        }
        menu
    }

    /// The glyph at `(x, y)` is `'A'` (src 8, 32); `highlighted` selects the `+128` green band.
    fn pixel_at(render: &RenderState, x: i32, y: i32) -> u16 {
        render.framebuffer.get(x, y)
    }

    #[test]
    fn setup_writes_only_the_upstream_fields() {
        let mut menu = menu_with(&["A"], MENU_ALIGN_RIGHT, 2);
        menu.entry_highlight[0] = 7;
        menu.setup(3, 1, MENU_ALIGN_CENTER);
        assert_eq!(menu.row_count, 3);
        assert_eq!(menu.selection_count, 1);
        assert_eq!(menu.alignment, MENU_ALIGN_CENTER);
        assert_eq!(menu.text_data_pos, 0);
        // `SetupTextMenu` leaves the arrays alone.
        assert_eq!(menu.text_data, vec![u16::from(b'A')]);
        assert_eq!(menu.entry_highlight[0], 7);
    }

    #[test]
    fn add_entry_overwrites_the_script_highlight() {
        let mut menu = TextMenu::default();
        menu.setup(0, 0, MENU_ALIGN_LEFT);
        menu.add_entry("AB", 128);
        menu.add_entry("CDE", 1);
        assert_eq!(menu.row_count, 2);
        assert_eq!(menu.entry_start, vec![0, 2]);
        assert_eq!(menu.entry_size, vec![2, 3]);
        // `AddTextMenuEntry` always writes false over the operand highlight.
        assert_eq!(menu.entry_highlight, vec![0, 0]);
        assert_eq!(menu.text_data, vec![65, 66, 67, 68, 69]);
        assert_eq!(menu.text_data_pos, 5);
    }

    #[test]
    fn add_entry_past_the_fixed_array_does_not_grow_it() {
        let mut menu = TextMenu::default();
        menu.setup(0, 0, MENU_ALIGN_LEFT);
        menu.add_entry("A", 0);
        menu.row_count = TEXT_ENTRY_COUNT as i32;
        menu.add_entry("BC", 0);
        // The row counter still advances, but the fixed arrays stay capped.
        assert_eq!(menu.row_count, TEXT_ENTRY_COUNT as i32 + 1);
        assert_eq!(menu.entry_size.len(), 1);
        assert_eq!(menu.text_data_pos, 3);
    }

    #[test]
    fn edit_entry_rewrites_in_place_and_ignores_the_unbounded_row() {
        let mut menu = menu_with(&["AB", "CD"], MENU_ALIGN_LEFT, 1);
        menu.edit_entry("XY", 0, 128);
        assert_eq!(menu.entry_start, vec![0, 2, 0]);
        assert_eq!(
            menu.entry_size,
            vec![2, 2, 0],
            "row 0 was rewritten in place"
        );
        assert_eq!(menu.entry_highlight[0], 128);
        assert_eq!(menu.entry_highlight[2], 0, "the stray rowCount write");
        assert_eq!(menu.text_data, vec![88, 89, 67, 68]);
        // `textDataPos` is untouched by `EditTextMenuEntry`.
        assert_eq!(menu.text_data_pos, 4);

        // Out-of-range rows are dropped instead of reading stale memory.
        let before = menu.clone();
        menu.edit_entry("Q", -1, 1);
        menu.edit_entry("Q", TEXT_ENTRY_COUNT as i32, 1);
        menu.edit_entry("Q", i32::MAX, 1);
        assert_eq!(menu, before);
    }

    #[test]
    fn edit_entry_overwrites_following_rows_when_longer() {
        let mut menu = menu_with(&["AB", "CD"], MENU_ALIGN_LEFT, 1);
        menu.edit_entry("XYZ", 0, 0);
        assert_eq!(menu.entry_size, vec![3, 2, 0]);
        assert_eq!(menu.text_data, vec![88, 89, 90, 68]);
    }

    #[test]
    fn draw_positions_rows_for_all_three_alignments() {
        for (alignment, expected) in [
            (MENU_ALIGN_LEFT, [100, 100, 100]),
            (MENU_ALIGN_RIGHT, [76, 84, 92]),
            // Center: `XPos - (size >> 1 << 3)`, with the `-4` odd-length shift from
            // `DrawTextMenuEntry`: 3 chars -> 88, 2 chars -> 92, 1 char -> 96.
            (MENU_ALIGN_CENTER, [88, 92, 96]),
        ] {
            let mut menu = menu_with(&["AAA", "BB", "C"], alignment, 1);
            menu.selection1 = -1; // no highlighted row
            let mut render = test_render();
            menu.draw(&mut render, 100, 16, 0);
            for (row, x) in expected.iter().enumerate() {
                let y = 16 + 8 * row as i32;
                assert_eq!(
                    pixel_at(&render, *x, y),
                    RED,
                    "alignment {alignment} row {row} first glyph at {x}"
                );
                assert_eq!(
                    pixel_at(&render, *x - 1, y),
                    0,
                    "alignment {alignment} row {row} nothing left of {x}"
                );
            }
        }
    }

    #[test]
    fn draw_highlights_the_selection_rows_per_selection_count() {
        // selectionCount 1: only selection1.
        let mut menu = menu_with(&["AA", "BB", "CC"], MENU_ALIGN_LEFT, 1);
        menu.selection1 = 1;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(pixel_at(&render, 20, 16), RED);
        assert_eq!(pixel_at(&render, 20, 24), GREEN);
        assert_eq!(pixel_at(&render, 20, 32), RED);

        // selectionCount 2: selection1 and selection2.
        let mut menu = menu_with(&["AA", "BB", "CC"], MENU_ALIGN_LEFT, 2);
        menu.selection1 = 0;
        menu.selection2 = 2;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(pixel_at(&render, 20, 16), GREEN);
        assert_eq!(pixel_at(&render, 20, 24), RED);
        assert_eq!(pixel_at(&render, 20, 32), GREEN);
    }

    #[test]
    fn draw_nothing_for_a_zero_selection_count_and_unknown_alignment() {
        let mut menu = menu_with(&["AA", "BB"], MENU_ALIGN_LEFT, 0);
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(pixel_at(&render, 20, 16), 0);
        assert_eq!(pixel_at(&render, 20, 24), 0);

        let mut menu = menu_with(&["AA", "BB"], 9, 1);
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(pixel_at(&render, 20, 16), 0);
    }

    #[test]
    fn draw_back_scans_selection2_and_the_stage_entry_skips_the_last_glyph() {
        // selectionCount 3 with highlights on rows 1 and 2 and selection1 = 3: the back-scan
        // picks row 2. Row 2 is drawn again via `DrawStageTextEntry`, whose last glyph ignores
        // the highlight; row 3 is the plain selected row.
        let mut menu = menu_with(&["AA", "BB", "CC", "DD"], MENU_ALIGN_LEFT, 3);
        menu.selection1 = 3;
        menu.entry_highlight[1] = 1;
        menu.entry_highlight[2] = 1;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(menu.selection2, 2, "last highlighted row up to selection1");
        assert_eq!(pixel_at(&render, 20, 16), RED, "row 0");
        assert_eq!(
            pixel_at(&render, 20, 24),
            RED,
            "row 1 is highlighted but not selected"
        );
        assert_eq!(pixel_at(&render, 20, 32), GREEN, "row 2 first glyph");
        assert_eq!(
            pixel_at(&render, 28, 32),
            RED,
            "row 2 last glyph ignores the highlight"
        );
        assert_eq!(pixel_at(&render, 20, 40), GREEN, "row 3 first glyph");
        assert_eq!(pixel_at(&render, 28, 40), GREEN, "row 3 last glyph");

        // No highlighted entry: the back-scan resets selection2 to -1.
        let mut menu = menu_with(&["AA"], MENU_ALIGN_LEFT, 3);
        menu.selection1 = 0;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(menu.selection2, -1);
    }

    #[test]
    fn draw_reads_visible_row_count_and_resets_a_stale_offset() {
        // visibleRowCount > 0: rows `offset..offset + count` are drawn and the offset survives.
        let mut menu = menu_with(&["AA", "BB", "CC"], MENU_ALIGN_LEFT, 1);
        menu.selection1 = -1;
        menu.visible_row_count = 2;
        menu.visible_row_offset = 1;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        // The loop starts at the offset but `YPos` also starts at the given y, so row 1 is the
        // first line drawn.
        assert_eq!(pixel_at(&render, 20, 16), RED, "row 1 at the first line");
        assert_eq!(pixel_at(&render, 20, 24), RED, "row 2 at the second line");
        assert_eq!(pixel_at(&render, 20, 32), 0, "only two visible rows");
        assert_eq!(menu.visible_row_offset, 1);

        // visibleRowCount == 0: the stale offset is cleared before drawing every row.
        let mut menu = menu_with(&["AA", "BB"], MENU_ALIGN_LEFT, 1);
        menu.selection1 = -1;
        menu.visible_row_offset = 5;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(menu.visible_row_offset, 0);
        assert_eq!(pixel_at(&render, 20, 16), RED);
        assert_eq!(pixel_at(&render, 20, 24), RED);
    }

    #[test]
    fn draw_with_an_out_of_range_row_count_does_not_loop_forever() {
        let mut menu = menu_with(&["AA"], MENU_ALIGN_LEFT, 3);
        menu.selection1 = i32::MAX;
        menu.row_count = i32::MAX;
        let mut render = test_render();
        menu.draw(&mut render, 20, 16, 0);
        assert_eq!(menu.selection2, -1);
        assert_eq!(pixel_at(&render, 20, 16), RED);
    }

    #[test]
    fn stage_entry_skips_the_last_glyph_highlight() {
        let menu = menu_with(&["AA", "B"], MENU_ALIGN_LEFT, 0);
        let mut render = test_render();
        menu.draw_stage_entry(&mut render, 0, 0, 20, 16, 128);
        assert_eq!(pixel_at(&render, 20, 16), GREEN, "first glyph highlighted");
        assert_eq!(
            pixel_at(&render, 28, 16),
            RED,
            "last glyph ignores the highlight"
        );
        menu.draw_stage_entry(&mut render, 0, 1, 20, 24, 128);
        assert_eq!(
            pixel_at(&render, 20, 24),
            RED,
            "single glyph is also the last"
        );
    }
}
