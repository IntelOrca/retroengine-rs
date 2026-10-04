//! Engine-side 3D scene drawing for legacy v4 scripts (`FUNC_DRAW3DSCENE`).
//!
//! The op arms in [`crate::host`] route here; `draw_3d_scene` is an inert placeholder until
//! M9b fills the flag switch. The two object-type resolvers used by the `FACE_FLAG_3DSPRITE`
//! branch are already real.

use crate::state::{EngineState, ScriptFrame};

impl EngineState {
    /// `FUNC_DRAW3DSCENE`: `TransformVertexBuffer` + `Sort3DDrawList` + the face-flag switch in
    /// `Draw3DScene(sheetID)` (`Scene3DLegacyv4.cpp:315-494`); M9b fills the body.
    pub fn draw_3d_scene(&mut self, sheet_id: i32) {
        // M9b: transform, sort, then dispatch flags 0-7 (3DSPRITE uses
        // `script_frame_for_type`/`object_sheet_for_type`).
        let _ = sheet_id;
    }

    /// `scriptFrames[objectScriptList[type].frameListOffset + index]`; the per-type frame list
    /// replaces the upstream base offset.
    #[must_use]
    pub fn script_frame_for_type(&self, type_id: i32, index: i32) -> Option<ScriptFrame> {
        usize::try_from(type_id)
            .ok()
            .and_then(|type_id| self.object_frames.get(type_id))
            .and_then(|frames| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| frames.get(index))
            })
            .copied()
    }

    /// `objectScriptList[type].spriteSheetID` resolved to the engine's render surface id.
    #[must_use]
    pub fn object_sheet_for_type(&self, type_id: i32) -> i32 {
        usize::try_from(type_id)
            .ok()
            .and_then(|type_id| self.objects.get(type_id))
            .map_or(0, |entry| entry.sprite_sheet_id)
    }
}
