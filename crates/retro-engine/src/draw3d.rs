//! Engine-side 3D scene drawing for legacy v4 scripts (`FUNC_DRAW3DSCENE`).
//!
//! [`EngineState::draw_3d_scene`] is the port of `Draw3DScene(spriteSheetID)`
//! (`Graphics/Legacy/v4/Scene3DLegacyv4.cpp:315-494`): transform the vertex buffer, sort the
//! face draw list, then rasterise every face by its `FACE_FLAG_*` branch. Culling is `z > 0` for
//! the `*_3D`/`FADED` flags, `z >= 0` for `*_2D` and `a.z > 0` only for `TEXTURED_C`,
//! `TEXTURED_C_BLEND` and `3DSPRITE`. The `3DSPRITE` branch resolves the object's script frame
//! and sheet through [`EngineState::script_frame_for_type`]/[`EngineState::object_sheet_for_type`]
//! and draws through the regular scaled/rotated/rotozoom sprite paths.

use retro_render::faces;
use retro_render::scene3d::{Face, Scene3DState, Vertex};
use retro_render::{
    FACE_BUFFER_SIZE, FACE_FLAG_3DSPRITE, FACE_FLAG_COLORED_2D, FACE_FLAG_COLORED_3D,
    FACE_FLAG_FADED, FACE_FLAG_TEXTURED_2D, FACE_FLAG_TEXTURED_3D, FACE_FLAG_TEXTURED_C,
    FACE_FLAG_TEXTURED_C_BLEND,
};

use crate::state::{EngineState, ScriptFrame};

/// `SCREEN_CENTERX` (fixed at `424 / 2` in `DrawingLegacy.cpp`).
const SCREEN_CENTER_X: i32 = 212;
/// `SCREEN_CENTERY` (`SCREEN_YSIZE / 2`).
const SCREEN_CENTER_Y: i32 = 120;

/// `FX_SCALE`.
const FX_SCALE: i32 = 0;
/// `FX_ROTATE`.
const FX_ROTATE: i32 = 1;
/// `FX_ROTOZOOM`.
const FX_ROTOZOOM: i32 = 2;

/// `vertexBuffer[index]`, or `None` when the script index is out of range.
fn vertex(scene: &Scene3DState, index: i32) -> Option<Vertex> {
    usize::try_from(index)
        .ok()
        .and_then(|index| scene.vertex_buffer.get(index))
        .copied()
}

/// `vertexBufferT[index]`, or `None` when the script index is out of range.
fn transformed_vertex(scene: &Scene3DState, index: i32) -> Option<Vertex> {
    usize::try_from(index)
        .ok()
        .and_then(|index| scene.vertex_buffer_t.get(index))
        .copied()
}

/// `SCREEN_CENTERX + projectionX * x / z`, `SCREEN_CENTERY - projectionY * y / z`.
fn project(scene: &Scene3DState, point: &Vertex) -> (i32, i32) {
    (
        SCREEN_CENTER_X.wrapping_add(scene.projection_x.wrapping_mul(point.x) / point.z),
        SCREEN_CENTER_Y.wrapping_sub(scene.projection_y.wrapping_mul(point.y) / point.z),
    )
}

/// A projected quad for the `z > 0` flags (`TEXTURED_3D`, `COLORED_3D`, `FADED`).
fn projected_quad(scene: &Scene3DState, face: &Face) -> Option<[Vertex; 4]> {
    let indices = [face.a, face.b, face.c, face.d];
    let mut quad = [Vertex::default(); 4];
    for (slot, index) in quad.iter_mut().zip(indices) {
        let point = transformed_vertex(scene, index)?;
        if point.z <= 0 {
            return None;
        }
        let (x, y) = project(scene, &point);
        slot.x = x;
        slot.y = y;
    }
    Some(quad)
}

/// A projected quad with the `vertexBuffer` u/v copied in (`TEXTURED_3D`).
fn projected_textured_quad(scene: &Scene3DState, face: &Face) -> Option<[Vertex; 4]> {
    let mut quad = projected_quad(scene, face)?;
    let indices = [face.a, face.b, face.c, face.d];
    for (slot, index) in quad.iter_mut().zip(indices) {
        let uv = vertex(scene, index)?;
        slot.u = uv.u;
        slot.v = uv.v;
    }
    Some(quad)
}

/// A screen-space quad for the `z >= 0` flags (`TEXTURED_2D`, `COLORED_2D`).
fn flat_quad(scene: &Scene3DState, face: &Face) -> Option<[Vertex; 4]> {
    let indices = [face.a, face.b, face.c, face.d];
    let mut quad = [Vertex::default(); 4];
    for (slot, index) in quad.iter_mut().zip(indices) {
        let point = transformed_vertex(scene, index)?;
        if point.z < 0 {
            return None;
        }
        slot.x = point.x;
        slot.y = point.y;
    }
    Some(quad)
}

/// A screen-space quad with the `vertexBuffer` u/v copied in (`TEXTURED_2D`).
fn flat_textured_quad(scene: &Scene3DState, face: &Face) -> Option<[Vertex; 4]> {
    let mut quad = flat_quad(scene, face)?;
    let indices = [face.a, face.b, face.c, face.d];
    for (slot, index) in quad.iter_mut().zip(indices) {
        let uv = vertex(scene, index)?;
        slot.u = uv.u;
        slot.v = uv.v;
    }
    Some(quad)
}

/// The `TEXTURED_C`/`TEXTURED_C_BLEND` quad: `vertexBufferT[a]` is the projected centre,
/// `vertexBuffer[b].u/v` the world-space pixel extent and `vertexBuffer[c].u/v` the UV extent.
fn centered_quad(scene: &Scene3DState, face: &Face) -> Option<[Vertex; 4]> {
    let center_point = transformed_vertex(scene, face.a)?;
    if center_point.z <= 0 {
        return None;
    }
    let extent = vertex(scene, face.b)?;
    let center_uv = vertex(scene, face.a)?;
    let z = center_point.z;
    let x_minus = SCREEN_CENTER_X.wrapping_add(
        scene
            .projection_x
            .wrapping_mul(center_point.x.wrapping_sub(extent.u))
            / z,
    );
    let x_plus = SCREEN_CENTER_X.wrapping_add(
        scene
            .projection_x
            .wrapping_mul(center_point.x.wrapping_add(extent.u))
            / z,
    );
    let y_plus = SCREEN_CENTER_Y.wrapping_sub(
        scene
            .projection_y
            .wrapping_mul(center_point.y.wrapping_add(extent.v))
            / z,
    );
    let y_minus = SCREEN_CENTER_Y.wrapping_sub(
        scene
            .projection_y
            .wrapping_mul(center_point.y.wrapping_sub(extent.v))
            / z,
    );
    let u_minus = center_uv.u.wrapping_sub(extent.u);
    let u_plus = center_uv.u.wrapping_add(extent.u);
    let v_minus = center_uv.v.wrapping_sub(extent.v);
    let v_plus = center_uv.v.wrapping_add(extent.v);
    Some([
        Vertex {
            x: x_minus,
            y: y_plus,
            u: u_minus,
            v: v_minus,
            ..Vertex::default()
        },
        Vertex {
            x: x_plus,
            y: y_plus,
            u: u_plus,
            v: v_minus,
            ..Vertex::default()
        },
        Vertex {
            x: x_minus,
            y: y_minus,
            u: u_minus,
            v: v_plus,
            ..Vertex::default()
        },
        Vertex {
            x: x_plus,
            y: y_minus,
            u: u_plus,
            v: v_plus,
            ..Vertex::default()
        },
    ])
}

impl EngineState {
    /// `FUNC_DRAW3DSCENE`: `TransformVertexBuffer` + `Sort3DDrawList` + `Draw3DScene(sheetID)`
    /// (`Scene3DLegacyv4.cpp:315-494`).
    pub fn draw_3d_scene(&mut self, sheet_id: i32) {
        self.scene3d.transform_vertex_buffer();
        self.scene3d.sort_draw_list();

        let count = usize::try_from(self.scene3d.face_count)
            .unwrap_or(0)
            .min(FACE_BUFFER_SIZE);
        for index in 0..count {
            let entry = self.scene3d.draw_list[index];
            let Ok(face_id) = usize::try_from(entry.face_id) else {
                continue;
            };
            let Some(face) = self.scene3d.face_buffer.get(face_id).copied() else {
                continue;
            };
            match face.flag {
                FACE_FLAG_TEXTURED_3D => {
                    if let Some(quad) = projected_textured_quad(&self.scene3d, &face) {
                        faces::draw_textured_face(&mut self.render, &quad, sheet_id);
                    }
                }
                FACE_FLAG_TEXTURED_2D => {
                    if let Some(quad) = flat_textured_quad(&self.scene3d, &face) {
                        faces::draw_textured_face(&mut self.render, &quad, sheet_id);
                    }
                }
                FACE_FLAG_COLORED_3D => {
                    if let Some(quad) = projected_quad(&self.scene3d, &face) {
                        faces::draw_face(&mut self.render, &quad, face.color);
                    }
                }
                FACE_FLAG_COLORED_2D => {
                    if let Some(quad) = flat_quad(&self.scene3d, &face) {
                        faces::draw_face(&mut self.render, &quad, face.color);
                    }
                }
                FACE_FLAG_FADED => {
                    if let Some(quad) = projected_quad(&self.scene3d, &face) {
                        let shifted = entry.depth.wrapping_sub(0x8000) >> 8;
                        let mut fog_strength = if shifted >= 0 { shifted } else { 0 };
                        if fog_strength > self.scene3d.fog_strength {
                            fog_strength = self.scene3d.fog_strength;
                        }
                        faces::draw_faded_face(
                            &mut self.render,
                            &quad,
                            face.color,
                            self.scene3d.fog_color as u32,
                            0xFF - fog_strength,
                        );
                    }
                }
                FACE_FLAG_TEXTURED_C => {
                    if let Some(quad) = centered_quad(&self.scene3d, &face) {
                        faces::draw_textured_face(&mut self.render, &quad, sheet_id);
                    }
                }
                FACE_FLAG_TEXTURED_C_BLEND => {
                    if let Some(quad) = centered_quad(&self.scene3d, &face) {
                        faces::draw_textured_face_blended(&mut self.render, &quad, sheet_id);
                    }
                }
                FACE_FLAG_3DSPRITE => {
                    let Some(anchor) = transformed_vertex(&self.scene3d, face.a) else {
                        continue;
                    };
                    if anchor.z <= 0 {
                        continue;
                    }
                    let (x_pos, y_pos) = project(&self.scene3d, &anchor);
                    let (Some(anchor_uv), Some(frame_uv), Some(args)) = (
                        vertex(&self.scene3d, face.a),
                        vertex(&self.scene3d, face.b),
                        vertex(&self.scene3d, face.c),
                    ) else {
                        continue;
                    };
                    let type_id = anchor_uv.u;
                    let Some(frame) = self.script_frame_for_type(type_id, frame_uv.u) else {
                        continue;
                    };
                    let sheet = self.object_sheet_for_type(type_id);
                    let direction = frame_uv.v as u8;
                    let pivot_x = frame.pivot_x.wrapping_neg();
                    let pivot_y = frame.pivot_y.wrapping_neg();
                    match anchor_uv.v {
                        FX_SCALE => self.render.draw_sprite_scaled(
                            sheet,
                            direction,
                            x_pos,
                            y_pos,
                            pivot_x,
                            pivot_y,
                            args.u,
                            args.u,
                            frame.width,
                            frame.height,
                            frame.spr_x,
                            frame.spr_y,
                        ),
                        FX_ROTATE => self.render.draw_sprite_rotated(
                            sheet,
                            direction,
                            x_pos,
                            y_pos,
                            pivot_x,
                            pivot_y,
                            frame.spr_x,
                            frame.spr_y,
                            frame.width,
                            frame.height,
                            args.v,
                        ),
                        FX_ROTOZOOM => self.render.draw_sprite_rotozoom(
                            sheet,
                            direction,
                            x_pos,
                            y_pos,
                            pivot_x,
                            pivot_y,
                            frame.spr_x,
                            frame.spr_y,
                            frame.width,
                            frame.height,
                            args.v,
                            args.u,
                        ),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
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
