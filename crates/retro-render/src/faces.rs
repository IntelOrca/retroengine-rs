//! Legacy v4 3D face rasterization: scan-edge setup and the four draw primitives.
//!
//! Line-by-line port of `Graphics/Legacy/Scene3DLegacy.cpp:22-110` (scan edges) and
//! `Graphics/Legacy/DrawingLegacy.cpp:2322-2948` (`DrawFace`, `DrawTexturedFace`,
//! `DrawFadedFace`, `DrawTexturedFaceBlended`). Upstream's globals
//! `faceLineStart/End/U/V` are fully re-cleared per face and live in [`FaceLines`] here.
//!
//! Quirks preserved: `bottom = max(y) + 1`, the `top > SCREEN_YSIZE - 1` early-out, C integer
//! division for the edge deltas with negative-top back-projection, `DrawFace`'s alpha
//! `(color & 0x7F000000) >> 23` (bit 23 is excluded, so alpha is always even and the `0xFF`
//! opaque branch is dead upstream), `DrawTexturedFace` looping `posDifference + 1` while
//! `DrawTexturedFaceBlended` loops `posDifference`, `((pal & 0xF7BC) >> 1) + ((dst & 0xF7BC) >> 1)`
//! blending, per-scanline palette banks and `(v << width_shift) + u` texture sampling with index
//! 0 transparent. Sampling outside the sheet returns index 0 here; upstream reads the shared
//! `graphicData` pool (only observable with malformed assets).

use crate::SCREEN_HEIGHT;
use crate::framebuffer::rgb888_to_rgb565;
use crate::lookup::LookupTables;
use crate::scene3d::Vertex;
use crate::state::RenderState;

/// `SCREEN_YSIZE`.
const SCREEN_YSIZE: i32 = SCREEN_HEIGHT as i32;

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

impl FaceLines {
    /// Fresh tables; callers re-initialise the rows they scan.
    fn zeroed() -> Self {
        Self {
            start: [0; 240],
            end: [0; 240],
            start_u: [0; 240],
            end_u: [0; 240],
            start_v: [0; 240],
            end_v: [0; 240],
        }
    }
}

/// `ProcessScanEdge` (`Scene3DLegacy.cpp:22-46`).
pub fn process_scan_edge(lines: &mut FaceLines, a: &Vertex, b: &Vertex) {
    if a.y == b.y {
        return;
    }
    let (mut top, mut bottom) = if a.y >= b.y {
        (b.y, a.y.wrapping_add(1))
    } else {
        (a.y, b.y.wrapping_add(1))
    };
    if top > SCREEN_YSIZE - 1 || bottom < 0 {
        return;
    }
    if bottom > SCREEN_YSIZE {
        bottom = SCREEN_YSIZE;
    }
    let mut full_x = a.x.wrapping_shl(16);
    let delta_x =
        b.x.wrapping_sub(a.x)
            .wrapping_shl(16)
            .wrapping_div(b.y.wrapping_sub(a.y));
    if top < 0 {
        full_x = full_x.wrapping_sub(top.wrapping_mul(delta_x));
        top = 0;
    }
    for i in top..bottom {
        let true_x = full_x >> 16;
        let row = i as usize;
        if let Some(start) = lines.start.get_mut(row)
            && true_x < *start
        {
            *start = true_x;
        }
        if let Some(end) = lines.end.get_mut(row)
            && true_x > *end
        {
            *end = true_x;
        }
        full_x = full_x.wrapping_add(delta_x);
    }
}

/// `ProcessScanEdgeUV` (`Scene3DLegacy.cpp:47-110`).
pub fn process_scan_edge_uv(lines: &mut FaceLines, a: &Vertex, b: &Vertex) {
    if a.y == b.y {
        return;
    }
    let (mut top, mut bottom) = if a.y >= b.y {
        (b.y, a.y.wrapping_add(1))
    } else {
        (a.y, b.y.wrapping_add(1))
    };
    if top > SCREEN_YSIZE - 1 || bottom < 0 {
        return;
    }
    if bottom > SCREEN_YSIZE {
        bottom = SCREEN_YSIZE;
    }
    let mut full_x = a.x.wrapping_shl(16);
    let mut full_u = a.u.wrapping_shl(16);
    let mut full_v = a.v.wrapping_shl(16);
    let delta_y = b.y.wrapping_sub(a.y);
    let delta_x = b.x.wrapping_sub(a.x).wrapping_shl(16).wrapping_div(delta_y);
    let delta_u = if a.u != b.u {
        b.u.wrapping_sub(a.u).wrapping_shl(16).wrapping_div(delta_y)
    } else {
        0
    };
    let delta_v = if a.v != b.v {
        b.v.wrapping_sub(a.v).wrapping_shl(16).wrapping_div(delta_y)
    } else {
        0
    };
    if top < 0 {
        full_x = full_x.wrapping_sub(top.wrapping_mul(delta_x));
        full_u = full_u.wrapping_sub(top.wrapping_mul(delta_u));
        full_v = full_v.wrapping_sub(top.wrapping_mul(delta_v));
        top = 0;
    }
    for i in top..bottom {
        let row = i as usize;
        let true_x = full_x >> 16;
        if let Some(start) = lines.start.get_mut(row)
            && true_x < *start
        {
            *start = true_x;
            lines.start_u[row] = full_u;
            lines.start_v[row] = full_v;
        }
        if let Some(end) = lines.end.get_mut(row)
            && true_x > *end
        {
            *end = true_x;
            lines.end_u[row] = full_u;
            lines.end_v[row] = full_v;
        }
        full_x = full_x.wrapping_add(delta_x);
        full_u = full_u.wrapping_add(delta_u);
        full_v = full_v.wrapping_add(delta_v);
    }
}

/// The shared quad early-outs and y-sort from the four upstream primitives.
///
/// Returns `(face_top, face_bottom)` with `lines` initialised over that range and the six
/// `ProcessScanEdge`/`ProcessScanEdgeUV` edges applied.
fn build_scan_lines(
    lines: &mut FaceLines,
    quad: &[Vertex; 4],
    linesize: i32,
    textured: bool,
) -> Option<(i32, i32)> {
    let v = quad;
    if (v[0].x < 0 && v[1].x < 0 && v[2].x < 0 && v[3].x < 0)
        || (v[0].x > linesize && v[1].x > linesize && v[2].x > linesize && v[3].x > linesize)
        || (v[0].y < 0 && v[1].y < 0 && v[2].y < 0 && v[3].y < 0)
        || (v[0].y > SCREEN_YSIZE
            && v[1].y > SCREEN_YSIZE
            && v[2].y > SCREEN_YSIZE
            && v[3].y > SCREEN_YSIZE)
        || (v[0].x == v[1].x && v[1].x == v[2].x && v[2].x == v[3].x)
        || (v[0].y == v[1].y && v[1].y == v[2].y && v[2].y == v[3].y)
    {
        return None;
    }

    let (mut vertex_a, mut vertex_b, mut vertex_c, mut vertex_d) = (0usize, 1, 2, 3);
    if v[1].y < v[0].y {
        vertex_a = 1;
        vertex_b = 0;
    }
    if v[2].y < v[vertex_a].y {
        let temp = vertex_a;
        vertex_a = 2;
        vertex_c = temp;
    }
    if v[3].y < v[vertex_a].y {
        let temp = vertex_a;
        vertex_a = 3;
        vertex_d = temp;
    }
    if v[vertex_c].y < v[vertex_b].y {
        std::mem::swap(&mut vertex_b, &mut vertex_c);
    }
    if v[vertex_d].y < v[vertex_b].y {
        std::mem::swap(&mut vertex_b, &mut vertex_d);
    }
    if v[vertex_d].y < v[vertex_c].y {
        std::mem::swap(&mut vertex_c, &mut vertex_d);
    }

    let face_top = v[vertex_a].y.max(0);
    let face_bottom = v[vertex_d].y.min(SCREEN_YSIZE);
    for row in face_top..face_bottom {
        let row = row as usize;
        lines.start[row] = 100000;
        lines.end[row] = -100000;
    }
    for (from, to) in [
        (vertex_a, vertex_b),
        (vertex_a, vertex_c),
        (vertex_a, vertex_d),
        (vertex_b, vertex_c),
        (vertex_c, vertex_d),
        (vertex_b, vertex_d),
    ] {
        if textured {
            process_scan_edge_uv(lines, &v[from], &v[to]);
        } else {
            process_scan_edge(lines, &v[from], &v[to]);
        }
    }
    Some((face_top, face_bottom))
}

/// `PACK_RGB888` on a packed `0x00RRGGBB` colour.
fn pack_rgb888(color: u32) -> u16 {
    rgb888_to_rgb565(
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    )
}

/// `blendLookupTable` component blend used by `DrawFace`/`DrawFadedFace`.
fn blend_pixel(
    lookup: &LookupTables,
    framebuffer: u16,
    color: u16,
    fbuffer_blend: usize,
    pixel_blend: usize,
) -> u16 {
    let blend = |index: usize| u32::from(lookup.blend.get(index).copied().unwrap_or(0));
    let r = ((blend(fbuffer_blend + (((framebuffer & 0xF800) >> 11) as usize))
        + blend(pixel_blend + (((color & 0xF800) >> 11) as usize)))
        << 11) as u16;
    let g = ((blend(fbuffer_blend + (((framebuffer & 0x7E0) >> 6) as usize))
        + blend(pixel_blend + (((color & 0x7E0) >> 6) as usize)))
        << 6) as u16;
    let b = (blend(fbuffer_blend + ((framebuffer & 0x1F) as usize))
        + blend(pixel_blend + ((color & 0x1F) as usize))) as u16;
    r | g | b
}

/// `DrawFace` (`DrawingLegacy.cpp:2322-2458`).
pub fn draw_face(render: &mut RenderState, quad: &[Vertex; 4], color: u32) {
    let mut alpha = (color & 0x7F00_0000) >> 23;
    if alpha < 1 {
        return;
    }
    if alpha > 0xFF {
        alpha = 0xFF;
    }
    let pitch = render.framebuffer.pitch() as i32;
    let mut lines = FaceLines::zeroed();
    let Some((mut face_top, face_bottom)) = build_scan_lines(&mut lines, quad, pitch, false) else {
        return;
    };

    let color16 = pack_rgb888(color);
    let fbuffer_blend = 0x20 * (0xFF - alpha) as usize;
    let pixel_blend = 0x20 * alpha as usize;
    let RenderState {
        framebuffer,
        lookup,
        ..
    } = render;
    let pitch = framebuffer.pitch();
    let pixels = framebuffer.pixels_mut();
    while face_top < face_bottom {
        let row = face_top as usize;
        let mut start_x = lines.start[row];
        let end_x = lines.end[row];
        if start_x < pitch as i32 && end_x > 0 {
            if start_x < 0 {
                start_x = 0;
            }
            let end_x = end_x.min(pitch as i32 - 1);
            let span_start = row * pitch + start_x as usize;
            let span_len = (end_x - start_x + 1) as usize;
            for offset in span_start..span_start + span_len {
                if let Some(pixel) = pixels.get_mut(offset) {
                    *pixel = if alpha == 0xFF {
                        color16
                    } else {
                        blend_pixel(lookup, *pixel, color16, fbuffer_blend, pixel_blend)
                    };
                }
            }
        }
        face_top += 1;
    }
}

/// `DrawTexturedFace` (`DrawingLegacy.cpp:2459-2584`) and
/// `DrawTexturedFaceBlended` (`DrawingLegacy.cpp:2818-2948`); `blended` selects the latter's
/// `posDifference` loop and average blend.
fn draw_textured_span(
    render: &mut RenderState,
    lines: &FaceLines,
    mut face_top: i32,
    face_bottom: i32,
    sheet_id: i32,
    blended: bool,
) {
    let RenderState {
        framebuffer,
        palette,
        surfaces,
        ..
    } = render;
    let Some(surface) = usize::try_from(sheet_id)
        .ok()
        .and_then(|index| surfaces.get(index))
        .filter(|surface| !surface.is_empty())
    else {
        return;
    };
    let pitch = framebuffer.pitch();
    let pixels = framebuffer.pixels_mut();
    while face_top < face_bottom {
        let row = face_top as usize;
        let colors = palette.line_colors(row);
        let start_x = lines.start[row];
        let end_x = lines.end[row];
        if start_x >= pitch as i32 || end_x <= 0 {
            face_top += 1;
            continue;
        }

        let mut start_x = start_x;
        let mut end_x = end_x;
        let mut u_pos = lines.start_u[row];
        let mut v_pos = lines.start_v[row];
        let pos_difference = end_x - start_x;
        let (buffered_u, buffered_v) = if end_x == start_x {
            (0, 0)
        } else {
            (
                lines.end_u[row]
                    .wrapping_sub(u_pos)
                    .wrapping_div(pos_difference),
                lines.end_v[row]
                    .wrapping_sub(v_pos)
                    .wrapping_div(pos_difference),
            )
        };
        if end_x > pitch as i32 - 1 {
            end_x = pitch as i32 - 1;
        }
        let mut counter = end_x - start_x;
        if start_x < 0 {
            counter = counter.wrapping_add(start_x);
            u_pos = u_pos.wrapping_sub(start_x.wrapping_mul(buffered_u));
            v_pos = v_pos.wrapping_sub(start_x.wrapping_mul(buffered_v));
            start_x = 0;
        }
        if !blended {
            counter = counter.wrapping_add(1);
        }
        let span_start = row * pitch + start_x as usize;
        for offset in span_start..span_start + counter.max(0) as usize {
            if u_pos < 0 {
                u_pos = 0;
            }
            if v_pos < 0 {
                v_pos = 0;
            }
            let sample = (v_pos >> 16)
                .wrapping_shl(surface.width_shift as u32)
                .wrapping_add(u_pos >> 16);
            let index = surface.pixel_at_offset(sample);
            if index > 0
                && let Some(pixel) = pixels.get_mut(offset)
            {
                let color = colors[usize::from(index)];
                *pixel = if blended {
                    ((color & 0xF7BC) >> 1) + ((*pixel & 0xF7BC) >> 1)
                } else {
                    color
                };
            }
            u_pos = u_pos.wrapping_add(buffered_u);
            v_pos = v_pos.wrapping_add(buffered_v);
        }
        face_top += 1;
    }
}

/// `DrawTexturedFace` (`DrawingLegacy.cpp:2459-2584`).
pub fn draw_textured_face(render: &mut RenderState, quad: &[Vertex; 4], sheet_id: i32) {
    let pitch = render.framebuffer.pitch() as i32;
    let mut lines = FaceLines::zeroed();
    let Some((face_top, face_bottom)) = build_scan_lines(&mut lines, quad, pitch, true) else {
        return;
    };
    draw_textured_span(render, &lines, face_top, face_bottom, sheet_id, false);
}

/// `DrawTexturedFaceBlended` (`DrawingLegacy.cpp:2818-2948`).
pub fn draw_textured_face_blended(render: &mut RenderState, quad: &[Vertex; 4], sheet_id: i32) {
    let pitch = render.framebuffer.pitch() as i32;
    let mut lines = FaceLines::zeroed();
    let Some((face_top, face_bottom)) = build_scan_lines(&mut lines, quad, pitch, true) else {
        return;
    };
    draw_textured_span(render, &lines, face_top, face_bottom, sheet_id, true);
}

/// `DrawFadedFace` (`DrawingLegacy.cpp:2704-2817`): blends `fogColor` into `color` instead of
/// reading the framebuffer, so the destination pixel is always overwritten.
pub fn draw_faded_face(
    render: &mut RenderState,
    quad: &[Vertex; 4],
    color: u32,
    fog_color: u32,
    alpha: i32,
) {
    let alpha = if alpha > 0xFF { 0xFF } else { alpha };
    if alpha < 1 {
        return;
    }
    let pitch = render.framebuffer.pitch() as i32;
    let mut lines = FaceLines::zeroed();
    let Some((mut face_top, face_bottom)) = build_scan_lines(&mut lines, quad, pitch, false) else {
        return;
    };

    let color16 = pack_rgb888(color);
    let fog_color16 = pack_rgb888(fog_color);
    let fbuffer_blend = 0x20 * (0xFF - alpha) as usize;
    let pixel_blend = 0x20 * alpha as usize;
    let RenderState {
        framebuffer,
        lookup,
        ..
    } = render;
    let pitch = framebuffer.pitch();
    let pixels = framebuffer.pixels_mut();
    while face_top < face_bottom {
        let row = face_top as usize;
        let mut start_x = lines.start[row];
        let end_x = lines.end[row];
        if start_x < pitch as i32 && end_x > 0 {
            if start_x < 0 {
                start_x = 0;
            }
            let end_x = end_x.min(pitch as i32 - 1);
            let span_start = row * pitch + start_x as usize;
            let span_len = (end_x - start_x + 1) as usize;
            for offset in span_start..span_start + span_len {
                if let Some(pixel) = pixels.get_mut(offset) {
                    *pixel = blend_pixel(lookup, fog_color16, color16, fbuffer_blend, pixel_blend);
                }
            }
        }
        face_top += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::Surface;

    fn vertex(x: i32, y: i32) -> Vertex {
        Vertex {
            x,
            y,
            ..Vertex::default()
        }
    }

    fn vertex_uv(x: i32, y: i32, u: i32, v: i32) -> Vertex {
        Vertex {
            x,
            y,
            u,
            v,
            ..Vertex::default()
        }
    }

    /// A rectangle whose four corners are `(x0,y0)`, `(x1,y0)`, `(x0,y1)`, `(x1,y1)`.
    fn quad(x0: i32, y0: i32, x1: i32, y1: i32) -> [Vertex; 4] {
        [
            vertex(x0, y0),
            vertex(x1, y0),
            vertex(x0, y1),
            vertex(x1, y1),
        ]
    }

    fn textured_quad(x0: i32, y0: i32, x1: i32, y1: i32) -> [Vertex; 4] {
        [
            vertex_uv(x0, y0, 0, 0),
            vertex_uv(x1, y0, x1 - x0, 0),
            vertex_uv(x0, y1, 0, y1 - y0),
            vertex_uv(x1, y1, x1 - x0, y1 - y0),
        ]
    }

    fn zero_lines() -> FaceLines {
        FaceLines::zeroed()
    }

    #[test]
    fn scan_edge_fills_a_vertical_span() {
        let mut lines = zero_lines();
        for row in 0..240 {
            lines.start[row] = 100000;
            lines.end[row] = -100000;
        }
        process_scan_edge(&mut lines, &vertex(5, 0), &vertex(5, 4));
        // `bottom = max(y) + 1`, so the bottom vertex's row is included.
        for row in 0..=4 {
            assert_eq!(lines.start[row], 5, "row {row} start");
            assert_eq!(lines.end[row], 5, "row {row} end");
        }
        assert_eq!(lines.start[5], 100000, "rows past the bottom are untouched");
    }

    #[test]
    fn scan_edge_diagonal_interpolates_and_back_projects_negative_top() {
        let mut lines = zero_lines();
        for row in 0..240 {
            lines.start[row] = 100000;
            lines.end[row] = -100000;
        }
        process_scan_edge(&mut lines, &vertex(0, 0), &vertex(4, 4));
        for row in 0..=4 {
            assert_eq!(lines.start[row], row as i32, "row {row}");
            assert_eq!(lines.end[row], row as i32, "row {row}");
        }

        let mut lines = zero_lines();
        for row in 0..240 {
            lines.start[row] = 100000;
            lines.end[row] = -100000;
        }
        process_scan_edge(&mut lines, &vertex(0, -2), &vertex(4, 2));
        // At y = 0..2 the edge sits at x = 2..4.
        assert_eq!(lines.start[0], 2);
        assert_eq!(lines.start[1], 3);
        assert_eq!(lines.start[2], 4);
        assert_eq!(lines.start[3], 100000, "bottom is exclusive");
    }

    #[test]
    fn scan_edge_uv_interpolates_both_coordinates() {
        let mut lines = zero_lines();
        for row in 0..240 {
            lines.start[row] = 100000;
            lines.end[row] = -100000;
        }
        process_scan_edge_uv(&mut lines, &vertex_uv(0, 0, 0, 0), &vertex_uv(4, 4, 8, 2));
        // Over 4 rows U advances 8 and V advances 2, so row i holds U = 2i, V = i/2.
        for row in 0..=4usize {
            let expected_u = (row as i32 * 2) << 16;
            let expected_v = (row as i32) << 15;
            assert_eq!(lines.start_u[row], expected_u, "row {row} u");
            assert_eq!(lines.start_v[row], expected_v, "row {row} v");
        }
    }

    #[test]
    fn scan_edge_early_outs_leave_the_tables_untouched() {
        let mut lines = zero_lines();
        process_scan_edge(&mut lines, &vertex(0, 5), &vertex(9, 5));
        assert_eq!(lines.start[5], 0, "flat edge writes nothing");

        process_scan_edge(&mut lines, &vertex(0, 240), &vertex(2, 260));
        assert_eq!(lines.start[239], 0, "top above the screen writes nothing");

        process_scan_edge(&mut lines, &vertex(0, -5), &vertex(2, -3));
        assert_eq!(lines.start[0], 0, "bottom above the screen writes nothing");
    }

    #[test]
    fn scan_edge_merges_into_start_and_end() {
        let mut lines = zero_lines();
        for row in 0..240 {
            lines.start[row] = 100000;
            lines.end[row] = -100000;
        }
        process_scan_edge(&mut lines, &vertex(2, 0), &vertex(2, 4));
        process_scan_edge(&mut lines, &vertex(6, 0), &vertex(6, 4));
        for row in 0..4 {
            assert_eq!(lines.start[row], 2);
            assert_eq!(lines.end[row], 6);
        }
    }

    #[test]
    fn draw_face_alpha_zero_and_culling_leave_the_framebuffer_untouched() {
        let mut render = RenderState::new(8, 8);
        render.framebuffer.clear(0x1234);
        draw_face(&mut render, &quad(0, 0, 4, 4), 0x00FF0000);
        draw_face(&mut render, &quad(20, 0, 24, 4), 0x40FF0000);
        draw_face(&mut render, &quad(0, 0, 4, 0), 0x40FF0000);
        assert!(
            render
                .framebuffer
                .pixels()
                .iter()
                .all(|pixel| *pixel == 0x1234)
        );
    }

    #[test]
    fn draw_face_blends_axis_aligned_quads() {
        let mut render = RenderState::new(8, 8);
        render.framebuffer.clear(0x07E0); // green
        draw_face(&mut render, &quad(1, 1, 3, 3), 0x40FF0000); // red, alpha 0x80
        // R = (0 + 0x80*31>>8) << 11 = 0x7800, G = (0x7F*31>>8 + 0) << 6 = 0x3C0, B = 0.
        // The raster loop is `faceTop < faceBottom`, so row 3 is not drawn.
        for y in 1..=2 {
            for x in 1..=3 {
                assert_eq!(render.framebuffer.get(x, y), 0x7BC0, "({x},{y})");
            }
        }
        assert_eq!(
            render.framebuffer.get(1, 3),
            0x07E0,
            "bottom row is excluded"
        );
        assert_eq!(render.framebuffer.get(0, 0), 0x07E0);
        assert_eq!(render.framebuffer.get(4, 4), 0x07E0);
    }

    #[test]
    fn draw_faded_face_blends_fog_into_the_face_color() {
        let mut render = RenderState::new(8, 8);
        render.framebuffer.clear(0xFFFF);
        draw_faded_face(&mut render, &quad(0, 0, 4, 4), 0x00FF0000, 0x000000FF, 0x80);
        // F = fog 0x001F, C = red 0xF800, alpha 0x80: R = 15 << 11, G = 0, B = 15.
        // The raster loop is `faceTop < faceBottom`, so row 4 is not drawn.
        for y in 0..=3 {
            for x in 0..=4 {
                assert_eq!(render.framebuffer.get(x, y), 0x780F, "({x},{y})");
            }
        }
        assert_eq!(
            render.framebuffer.get(0, 4),
            0xFFFF,
            "bottom row is excluded"
        );
        assert_eq!(render.framebuffer.get(5, 0), 0xFFFF);
    }

    #[test]
    fn draw_textured_face_maps_uv_and_uses_per_scanline_palettes() {
        let mut render = RenderState::new(8, 8);
        let mut pixels = vec![0u8; 16];
        pixels[0] = 0; // transparent
        pixels[1] = 1;
        pixels[4] = 2;
        render.surfaces.push(Surface::from_indexed(4, 4, pixels));
        render.palette.set_bank_entry(0, 1, 255, 0, 0);
        render.palette.set_bank_entry(1, 2, 0, 0, 255);
        render.palette.set_active_palette(1, 1, 2);

        draw_textured_face(&mut render, &textured_quad(0, 0, 3, 2), 0);
        assert_eq!(render.framebuffer.get(0, 0), 0, "index 0 is transparent");
        assert_eq!(render.framebuffer.get(1, 0), 0xF800, "index 1, bank 0");
        assert_eq!(render.framebuffer.get(0, 1), 0x001F, "index 2, bank 1");
        assert_eq!(
            render.framebuffer.get(3, 0),
            0,
            "unpainted surface index stays clear"
        );
    }

    #[test]
    fn draw_textured_face_blended_loops_one_fewer_pixel() {
        let mut render = RenderState::new(8, 8);
        render.framebuffer.clear(0x07E0); // green
        let mut pixels = vec![0u8; 4];
        pixels[0] = 1;
        pixels[1] = 2;
        render.surfaces.push(Surface::from_indexed(2, 2, pixels));
        render.palette.set_bank_entry(0, 1, 255, 0, 0);

        draw_textured_face_blended(&mut render, &textured_quad(0, 0, 1, 1), 0);
        // `posDifference` loop: only x = 0 is written, averaged towards red:
        // (0xF800 & 0xF7BC) >> 1 = 0x7800, (0x07E0 & 0xF7BC) >> 1 = 0x03D0.
        assert_eq!(render.framebuffer.get(0, 0), 0x7BD0);
        assert_eq!(
            render.framebuffer.get(1, 0),
            0x07E0,
            "blended loop skips the last pixel"
        );
    }

    #[test]
    fn textured_culling_early_out_leaves_the_framebuffer_untouched() {
        let mut render = RenderState::new(8, 8);
        render.framebuffer.clear(0x1234);
        render
            .surfaces
            .push(Surface::from_indexed(2, 2, vec![1; 4]));
        draw_textured_face(&mut render, &textured_quad(0, 0, 2, 0), 0);
        draw_textured_face(&mut render, &textured_quad(20, 0, 24, 4), 0);
        assert!(
            render
                .framebuffer
                .pixels()
                .iter()
                .all(|pixel| *pixel == 0x1234)
        );
    }
}
