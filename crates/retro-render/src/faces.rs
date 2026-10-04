//! Legacy v4 3D face rasterization: scan-edge setup and the four draw primitives.
//!
//! This is the frozen M9 interface; every body is an inert placeholder that M9b fills from
//! `Graphics/Legacy/DrawingLegacy.cpp:2322-2948`. `FaceLines` mirrors the upstream globals
//! `faceLineStart/End/U/V`, which are fully re-cleared per face and therefore local per call.

use crate::scene3d::Vertex;
use crate::state::RenderState;

/// Per-face scanline edge tables (`Legacy::faceLineStart/End/U/V`).
pub struct FaceLines {
    /// Left x per scanline.
    pub start: [i32; 240],
    /// Right x per scanline.
    pub end: [i32; 240],
    /// Left u per scanline.
    pub start_u: [i32; 240],
    /// Right u per scanline.
    pub end_u: [i32; 240],
    /// Left v per scanline.
    pub start_v: [i32; 240],
    /// Right v per scanline.
    pub end_v: [i32; 240],
}

/// `ProcessScanEdge` (`Scene3DLegacy.cpp:22-110`); M9b fills the body.
pub fn process_scan_edge(lines: &mut FaceLines, a: &Vertex, b: &Vertex) {
    // M9b.
    let _ = (lines, a, b);
}

/// `ProcessScanEdgeUV` (`Scene3DLegacy.cpp:22-110`); M9b fills the body.
pub fn process_scan_edge_uv(lines: &mut FaceLines, a: &Vertex, b: &Vertex) {
    // M9b.
    let _ = (lines, a, b);
}

/// `DrawFace` (`DrawingLegacy.cpp:2322-2458`); M9b fills the body.
pub fn draw_face(render: &mut RenderState, quad: &[Vertex; 4], color: u32) {
    // M9b: `counter + 1` while clip; alpha is `(color & 0x7F000000) >> 23`.
    let _ = (render, quad, color);
}

/// `DrawTexturedFace` (`DrawingLegacy.cpp:2459-2584`); M9b fills the body.
pub fn draw_textured_face(render: &mut RenderState, quad: &[Vertex; 4], sheet_id: i32) {
    // M9b: loops `posDifference + 1` (upstream off-by-one) and samples
    // `pixels[(v << width_shift) + u]` with index 0 transparent.
    let _ = (render, quad, sheet_id);
}

/// `DrawFadedFace` (`DrawingLegacy.cpp:2704-2817`); M9b fills the body.
pub fn draw_faded_face(
    render: &mut RenderState,
    quad: &[Vertex; 4],
    color: u32,
    fog_color: u32,
    alpha: i32,
) {
    // M9b: `fogStr = clamp((depth - 0x8000) >> 8, 0, fogStrength)`, alpha `0xFF - fogStr`.
    let _ = (render, quad, color, fog_color, alpha);
}

/// `DrawTexturedFaceBlended` (`DrawingLegacy.cpp:2818-2948`); M9b fills the body.
pub fn draw_textured_face_blended(render: &mut RenderState, quad: &[Vertex; 4], sheet_id: i32) {
    // M9b: loops `posDifference` and blends `((pal & 0xF7BC) >> 1) + ((dst & 0xF7BC) >> 1)`.
    let _ = (render, quad, sheet_id);
}
