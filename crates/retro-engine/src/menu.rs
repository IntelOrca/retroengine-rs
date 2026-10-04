//! Legacy v4 text-menu operations (`SetupTextMenu`/`AddTextMenuEntry`/`EditTextMenuEntry` and
//! `DrawTextMenu`).
//!
//! Upstream lives in `Storage/Legacy/TextLegacy.cpp:174-238` and
//! `Graphics/Legacy/DrawingLegacy.cpp:3068-3245`, dispatched from `ScriptLegacyv4.cpp:4385-4390,
//! 4660-4680`. The M8 WP2 workstream fills in the bodies below; the signatures and the
//! [`MENU_ALIGN_*`] values are frozen by the M8 WP0 scaffolding so the host wiring and tests can
//! land independently.

use retro_render::RenderState;

use crate::state::TextMenu;

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
        // M8 WP2 fills this in.
        let _ = (row_count, selection_count, alignment);
    }

    /// `FUNC_ADDMENUENTRY` (`ScriptLegacyv4.cpp:4667-4672`): `add_entry` receives the script text
    /// and the `operands[2]` highlight.
    pub fn add_entry(&mut self, text: &str, highlight: i32) {
        // M8 WP2 fills this in (`entryHighlight[rowCount] = false` overwrite included).
        let _ = (text, highlight);
    }

    /// `FUNC_EDITMENUENTRY` (`ScriptLegacyv4.cpp:4673-4678`): `EditTextMenuEntry` for `row_id`,
    /// then `entryHighlight[row_id] = highlight`.
    pub fn edit_entry(&mut self, text: &str, row_id: i32, highlight: i32) {
        // M8 WP2 fills this in.
        let _ = (text, row_id, highlight);
    }

    /// `FUNC_DRAWMENU` (`ScriptLegacyv4.cpp:4385-4390`): sets `textMenuSurfaceNo` to the
    /// object's sheet, then `DrawTextMenu(menu, x, y)` (`DrawingLegacy.cpp:3120-3245`).
    pub fn draw(&mut self, render: &mut RenderState, x: i32, y: i32, sheet: i32) {
        // M8 WP2 fills this in.
        let _ = (render, x, y, sheet);
    }
}
