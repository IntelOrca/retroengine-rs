//! Tile-layer state and the two per-line tile renderers.
//!
//! Ports `DrawHLineScrollLayer`/`DrawVLineScrollLayer` from `Drawing.cpp` plus the `TileLayer`
//! / `LineScroll` structures from `Scene.hpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195).
//! Rendering is integer-only; the only per-frame mutable layer field is `scroll_pos`, which the
//! background paths advance by `scroll_speed` once per draw exactly like upstream.

use crate::draw::{FLIP_X, FLIP_XY, FLIP_Y};
use crate::state::RenderState;

/// `LAYER_NOSCROLL`.
pub const LAYER_NOSCROLL: i32 = 0;
/// `LAYER_HSCROLL`.
pub const LAYER_HSCROLL: i32 = 1;
/// `LAYER_VSCROLL`.
pub const LAYER_VSCROLL: i32 = 2;
/// `LAYER_3DFLOOR`.
pub const LAYER_3DFLOOR: i32 = 3;
/// `LAYER_3DSKY`.
pub const LAYER_3DSKY: i32 = 4;

/// Number of 16x16 tiles the engine stores (`TILE_COUNT`).
pub const TILE_COUNT: usize = 0x400;
/// Pixels per 16x16 tile (`TILE_DATASIZE`).
pub const TILE_PIXEL_COUNT: usize = 16 * 16;
/// Length of the normalized 16x16 tileset buffer (`TILESET_SIZE`).
pub const TILE_SET_16_SIZE: usize = TILE_COUNT * TILE_PIXEL_COUNT;
/// Chunk stride of the engine's tile-layer buffer (`TILELAYER_CHUNK_W`).
pub const TILE_LAYER_STRIDE: usize = 0x100;
/// Height of the engine's tile-layer buffer (`TILELAYER_CHUNK_H`).
pub const TILE_LAYER_HEIGHT: usize = 0x100;
/// Length of a tile layer's line-scroll buffer (`TILELAYER_LINESCROLL_COUNT`).
pub const LINE_SCROLL_COUNT: usize = TILE_LAYER_HEIGHT * 0x80;
/// Number of parallax entries (`PARALLAX_COUNT`).
pub const PARALLAX_COUNT: usize = 0x100;

/// One 128x128 chunk descriptor decoded from `128x128Tiles.bin`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChunkEntry {
    /// `tiles128x128.gfxDataPos[chunk]`: byte offset into the normalized 16x16 tileset.
    pub gfx_data_pos: i32,
    /// `tiles128x128.direction[chunk]`.
    pub direction: u8,
    /// `tiles128x128.visualPlane[chunk]`.
    pub visual_plane: u8,
}

/// One of the engine's nine tile layers (`TileLayer`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerState {
    /// Layout width in 128x128 chunks (`xsize`).
    pub xsize: i32,
    /// Layout height in 128x128 chunks (`ysize`).
    pub ysize: i32,
    /// Scroll type, one of the `LAYER_*` constants.
    pub layer_type: i32,
    /// Layer angle (3D layers only).
    pub angle: i32,
    /// Layer x position (3D layers only).
    pub xpos: i32,
    /// Layer y position (3D layers only).
    pub ypos: i32,
    /// Layer z position (3D layers only).
    pub zpos: i32,
    /// Parallax factor (`parallaxFactor`).
    pub parallax_factor: i32,
    /// Auto-scroll speed (`scrollSpeed`).
    pub scroll_speed: i32,
    /// Auto-scroll position, advanced by background draws.
    pub scroll_pos: i32,
    /// `deformationOffset`.
    pub deformation_offset: i32,
    /// `deformationOffsetW`.
    pub deformation_offset_w: i32,
    /// Chunk indices in the engine's `0x100`-wide layout.
    pub tiles: Vec<u16>,
    /// Expanded per-line scroll data, padded to [`LINE_SCROLL_COUNT`].
    pub line_scroll: Vec<u8>,
}

impl Default for LayerState {
    fn default() -> Self {
        Self {
            xsize: 0,
            ysize: 0,
            layer_type: 0,
            angle: 0,
            xpos: 0,
            ypos: 0,
            zpos: 0,
            parallax_factor: 0,
            scroll_speed: 0,
            scroll_pos: 0,
            deformation_offset: 0,
            deformation_offset_w: 0,
            tiles: vec![0; TILE_LAYER_STRIDE * TILE_LAYER_HEIGHT],
            line_scroll: vec![0; LINE_SCROLL_COUNT],
        }
    }
}

impl LayerState {
    /// Reads a chunk entry with the upstream `x + 0x100 * y` addressing.
    #[must_use]
    pub fn entry(&self, x: i32, y: i32) -> u16 {
        let index = i64::from(x) + 0x100 * i64::from(y);
        usize::try_from(index)
            .ok()
            .and_then(|index| self.tiles.get(index).copied())
            .unwrap_or(0)
    }

    /// Writes a chunk entry with the upstream `x + 0x100 * y` addressing.
    pub fn set_entry(&mut self, x: i32, y: i32, value: u16) {
        let index = i64::from(x) + 0x100 * i64::from(y);
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tiles.get_mut(index))
        {
            *slot = value;
        }
    }

    /// Reads line-scroll byte `index` (a byte offset into the buffer), or `0` when out of range.
    #[must_use]
    fn scroll(&self, index: i32) -> u8 {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.line_scroll.get(index).copied())
            .unwrap_or(0)
    }
}

/// Horizontal or vertical parallax table (`LineScroll`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallaxState {
    /// `parallaxFactor`.
    pub parallax_factor: Vec<i32>,
    /// `scrollSpeed`.
    pub scroll_speed: Vec<i32>,
    /// `scrollPos`.
    pub scroll_pos: Vec<i32>,
    /// `linePos`.
    pub line_pos: Vec<i32>,
    /// `deform`.
    pub deform: Vec<i32>,
    /// `entryCount`.
    pub entry_count: usize,
}

impl Default for ParallaxState {
    fn default() -> Self {
        Self {
            parallax_factor: vec![0; PARALLAX_COUNT],
            scroll_speed: vec![0; PARALLAX_COUNT],
            scroll_pos: vec![0; PARALLAX_COUNT],
            line_pos: vec![0; PARALLAX_COUNT],
            deform: vec![0; PARALLAX_COUNT],
            entry_count: 0,
        }
    }
}

/// Per-call options of a tile-layer draw.
#[derive(Debug, Clone, Copy)]
pub struct LayerView {
    /// Whether the layer is one of the background layers (nonzero `activeTileLayers` entry).
    pub is_background: bool,
    /// Whether the layer is drawn above the tile midpoint (`layerID >= tLayerMidPoint`).
    pub above_mid_point: bool,
    /// `xScrollOffset`.
    pub x_scroll_offset: i32,
    /// `yScrollOffset`.
    pub y_scroll_offset: i32,
}

/// Fetches a chunk entry, returning `None` for malformed indices.
fn chunk_entry(render: &RenderState, chunk: i32) -> Option<ChunkEntry> {
    usize::try_from(chunk)
        .ok()
        .and_then(|index| render.tiles.chunks.get(index))
        .copied()
}

/// A vertical run of tile pixels: draws `count` pixels down a column.
#[allow(clippy::too_many_arguments)]
fn draw_tile_column(
    render: &mut RenderState,
    entry: ChunkEntry,
    tile_x16: i32,
    out_col: i32,
    out_row: i32,
    count: i32,
) {
    let (base, step) = match entry.direction {
        FLIP_X => (0xF - tile_x16, 16),
        FLIP_Y => (tile_x16 + render.framebuffer.height() as i32, -16),
        FLIP_XY => (0xFF - tile_x16, -16),
        _ => (tile_x16, 16),
    };
    let mut cursor = entry.gfx_data_pos.wrapping_add(base);
    let colors: [u16; 256] = *render.palette.line_colors(0);
    for offset in 0..count {
        let index = render
            .tiles
            .pixels
            .get(usize::try_from(cursor).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0);
        if index > 0 {
            render
                .framebuffer
                .set(out_col, out_row + offset, colors[usize::from(index)]);
        }
        cursor = cursor.wrapping_add(step);
    }
}

/// `DrawVLineScrollLayer`.
pub fn draw_v_line_scroll_layer(
    render: &mut RenderState,
    layer: &mut LayerState,
    parallax: &mut ParallaxState,
    view: LayerView,
) {
    if layer.xsize == 0 || layer.ysize == 0 {
        return;
    }
    let screen_width = render.framebuffer.width() as i32;
    let screen_height = render.framebuffer.height() as i32;
    let mut layer_width = layer.xsize;
    let mut layer_height = layer.ysize;
    let above = view.above_mid_point as u8;
    let deform_index = if view.is_background { 2 } else { 0 };

    let mut deformation: i32;
    let xscroll_offset: i32;
    if view.is_background {
        let x_scroll = view.x_scroll_offset.wrapping_mul(layer.parallax_factor) >> 8;
        let full_layer_width = layer_width << 7;
        layer.scroll_pos = layer.scroll_pos.wrapping_add(layer.scroll_speed);
        if layer.scroll_pos > full_layer_width << 16 {
            layer.scroll_pos -= full_layer_width << 16;
        }
        xscroll_offset = x_scroll.wrapping_add(layer.scroll_pos >> 16) % full_layer_width;
        layer_width = full_layer_width >> 7;
        deformation = i32::from((xscroll_offset.wrapping_add(layer.deformation_offset)) as u8);
    } else {
        render.last_y_size = layer.ysize;
        xscroll_offset = view.x_scroll_offset;
        parallax.line_pos[0] = view.y_scroll_offset;
        parallax.deform[0] = 1;
        deformation = i32::from(view.x_scroll_offset.wrapping_add(layer.deformation_offset) as u8);
    }

    if layer.layer_type == LAYER_VSCROLL {
        if render.last_y_size != layer.ysize {
            let full_layer_height = layer_height << 7;
            for index in 0..parallax.entry_count.min(PARALLAX_COUNT) {
                parallax.line_pos[index] = view
                    .y_scroll_offset
                    .wrapping_mul(parallax.parallax_factor[index])
                    >> 8;
                parallax.scroll_pos[index] =
                    parallax.scroll_pos[index].wrapping_add(parallax.scroll_pos[index] << 16);
                if parallax.scroll_pos[index] > full_layer_height << 16 {
                    parallax.scroll_pos[index] -= full_layer_height << 16;
                }
                parallax.line_pos[index] = parallax.line_pos[index]
                    .wrapping_add(parallax.scroll_pos[index] >> 16)
                    % full_layer_height;
            }
            layer_height = full_layer_height >> 7;
        }
        render.last_y_size = layer_height;
    }

    let mut tile_x_pos = xscroll_offset % (layer_height << 7);
    if tile_x_pos < 0 {
        tile_x_pos += layer_height << 7;
    }
    let mut scroll_index = tile_x_pos;
    let mut chunk_x = tile_x_pos >> 7;
    let mut tile_x16 = tile_x_pos & 0xF;
    let mut tile_x = (tile_x_pos & 0x7F) >> 4;

    let mut out_col = 0;
    while out_col < screen_width {
        let scroll_value = usize::from(layer.scroll(scroll_index));
        let mut chunk_y = parallax.line_pos.get(scroll_value).copied().unwrap_or(0);
        if parallax.deform.get(scroll_value).copied().unwrap_or(0) != 0 {
            chunk_y = chunk_y.wrapping_add(
                render.deform_data[deform_index]
                    .get(usize::try_from(deformation).unwrap_or(usize::MAX))
                    .copied()
                    .unwrap_or(0),
            );
        }
        deformation += 1;
        scroll_index += 1;
        let full_layer_height = layer_height << 7;
        if chunk_y < 0 {
            chunk_y += full_layer_height;
        }
        if chunk_y >= full_layer_height {
            chunk_y -= full_layer_height;
        }
        let mut chunk_y_pos = chunk_y >> 7;
        let tile_y = chunk_y & 0xF;
        let first_count = 16 - tile_y;
        let mut chunk = (i32::from(layer.entry(chunk_x, chunk_y >> 7)) << 6)
            + tile_x
            + 8 * ((chunk_y & 0x7F) >> 4);
        let mut line_remain = screen_height;
        let mut out_row = 0;

        // First (partial) tile of the column.
        let entry = chunk_entry(render, chunk);
        if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
            draw_tile_column(render, entry, tile_x16, out_col, out_row, first_count);
        }
        out_row += first_count;
        line_remain -= first_count;

        // Bulk of the column.
        let mut chunk_tile_y = ((chunk_y & 0x7F) >> 4) + 1;
        let mut tiles_per_line = (screen_height >> 4) - 1;
        while tiles_per_line > 0 {
            tiles_per_line -= 1;
            if chunk_tile_y < 8 {
                chunk += 8;
            } else {
                chunk_y_pos += 1;
                if chunk_y_pos == layer_height {
                    chunk_y_pos = 0;
                }
                chunk_tile_y = 0;
                chunk = (i32::from(layer.entry(chunk_x, chunk_y_pos)) << 6) + tile_x;
            }
            line_remain -= 16;
            let entry = chunk_entry(render, chunk);
            if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
                draw_tile_column(render, entry, tile_x16, out_col, out_row, 16);
            }
            out_row += 16;
            chunk_tile_y += 1;
        }

        // Remaining pixels.
        while line_remain > 0 {
            let old = chunk_tile_y;
            chunk_tile_y += 1;
            if old < 8 {
                chunk += 8;
            } else {
                chunk_y_pos += 1;
                if chunk_y_pos == layer_height {
                    chunk_y_pos = 0;
                }
                chunk_tile_y = 0;
                chunk = (i32::from(layer.entry(chunk_x, chunk_y_pos)) << 6) + tile_x;
            }
            let count = line_remain.min(16);
            line_remain -= count;
            let entry = chunk_entry(render, chunk);
            if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
                draw_tile_column(render, entry, tile_x16, out_col, out_row, count);
            }
            out_row += count;
        }

        tile_x16 += 1;
        if tile_x16 >= 16 {
            tile_x16 = 0;
            tile_x += 1;
        }
        if tile_x >= 8 {
            chunk_x += 1;
            if chunk_x == layer_width {
                chunk_x = 0;
                scroll_index -= 0x80 * layer_width;
            }
            tile_x = 0;
        }
        out_col += 1;
    }
}

/// Horizontal tile-run draw: blits `count` pixels of one 16x16 tile row.
#[allow(clippy::too_many_arguments)]
fn draw_tile_row(
    render: &mut RenderState,
    entry: ChunkEntry,
    tile_y16: i32,
    tile_px_x_offset: i32,
    out_row: i32,
    out_col: i32,
    count: i32,
    colors: &[u16; 256],
) {
    let (base, step) = match entry.direction {
        FLIP_X => (16 * tile_y16 + 0xF, -1),
        FLIP_Y => (16 * (0xF - tile_y16), 1),
        FLIP_XY => (16 * (0xF - tile_y16) + 0xF, -1),
        _ => (16 * tile_y16, 1),
    };
    let start = if step < 0 {
        entry
            .gfx_data_pos
            .wrapping_add(base)
            .wrapping_sub(tile_px_x_offset)
    } else {
        entry
            .gfx_data_pos
            .wrapping_add(base)
            .wrapping_add(tile_px_x_offset)
    };
    let mut cursor = start;
    for offset in 0..count {
        let index = render
            .tiles
            .pixels
            .get(usize::try_from(cursor).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(0);
        if index > 0 {
            render
                .framebuffer
                .set(out_col + offset, out_row, colors[usize::from(index)]);
        }
        cursor = cursor.wrapping_add(step);
    }
}

/// `DrawHLineScrollLayer`.
pub fn draw_h_line_scroll_layer(
    render: &mut RenderState,
    layer: &mut LayerState,
    parallax: &mut ParallaxState,
    view: LayerView,
) {
    if layer.xsize == 0 || layer.ysize == 0 {
        return;
    }
    let pitch = render.framebuffer.pitch() as i32;
    let screen_height = render.framebuffer.height() as i32;
    let screenwidth16 = (pitch >> 4) - 1;
    let layer_width = layer.xsize;
    let mut layer_height = layer.ysize;
    let above = view.above_mid_point as u8;
    let deform_index = if view.is_background { 2 } else { 0 };

    let mut deformation: i32;
    let mut deformation_w: i32;
    let yscroll_offset: i32;
    if view.is_background {
        let y_scroll = view.y_scroll_offset.wrapping_mul(layer.parallax_factor) >> 8;
        let full_height = layer_height << 7;
        layer.scroll_pos = layer.scroll_pos.wrapping_add(layer.scroll_speed);
        if layer.scroll_pos > full_height << 16 {
            layer.scroll_pos -= full_height << 16;
        }
        yscroll_offset = y_scroll.wrapping_add(layer.scroll_pos >> 16) % full_height;
        layer_height = full_height >> 7;
        deformation = i32::from((yscroll_offset.wrapping_add(layer.deformation_offset)) as u8);
        deformation_w = i32::from(
            yscroll_offset
                .wrapping_add(render.water_draw_pos)
                .wrapping_add(layer.deformation_offset_w) as u8,
        );
    } else {
        render.last_x_size = layer.xsize;
        yscroll_offset = view.y_scroll_offset;
        for line_pos in parallax.line_pos.iter_mut().take(PARALLAX_COUNT) {
            *line_pos = view.x_scroll_offset;
        }
        deformation = i32::from(view.y_scroll_offset.wrapping_add(layer.deformation_offset) as u8);
        deformation_w = i32::from(
            view.y_scroll_offset
                .wrapping_add(render.water_draw_pos)
                .wrapping_add(layer.deformation_offset_w) as u8,
        );
    }

    if layer.layer_type == LAYER_HSCROLL {
        if render.last_x_size != layer_width {
            let full_layer_width = layer_width << 7;
            for index in 0..parallax.entry_count.min(PARALLAX_COUNT) {
                let mut line_pos = view
                    .x_scroll_offset
                    .wrapping_mul(parallax.parallax_factor[index])
                    >> 8;
                if parallax.scroll_pos[index] > full_layer_width << 16 {
                    parallax.scroll_pos[index] -= full_layer_width << 16;
                }
                if parallax.scroll_pos[index] < 0 {
                    parallax.scroll_pos[index] += full_layer_width << 16;
                }
                line_pos =
                    line_pos.wrapping_add(parallax.scroll_pos[index] >> 16) % full_layer_width;
                parallax.line_pos[index] = line_pos;
            }
        }
        render.last_x_size = if view.is_background { layer_width } else { -1 };
    }

    let mut tile_y_pos = yscroll_offset % (layer_height << 7);
    if tile_y_pos < 0 {
        tile_y_pos += layer_height << 7;
    }
    let mut scroll_index = tile_y_pos;
    let mut tile_y16 = tile_y_pos & 0xF;
    let mut chunk_y = tile_y_pos >> 7;
    let mut tile_y = (tile_y_pos & 0x7F) >> 4;

    let drawable_lines = [render.water_draw_pos, screen_height - render.water_draw_pos];
    let mut out_row = 0;
    for (region, drawable) in drawable_lines.into_iter().enumerate() {
        let mut remaining = drawable;
        while remaining > 0 {
            remaining -= 1;
            let colors: [u16; 256] = *render.palette.line_colors(out_row as usize);
            let scroll_value = usize::from(layer.scroll(scroll_index));
            let mut chunk_x = parallax.line_pos.get(scroll_value).copied().unwrap_or(0);
            if parallax.deform.get(scroll_value).copied().unwrap_or(0) != 0 {
                chunk_x = chunk_x.wrapping_add(
                    render.deform_data[deform_index + region]
                        .get(
                            usize::try_from(if region == 0 {
                                deformation
                            } else {
                                deformation_w
                            })
                            .unwrap_or(usize::MAX),
                        )
                        .copied()
                        .unwrap_or(0),
                );
            }
            if region == 0 {
                deformation += 1;
            } else {
                deformation_w += 1;
            }
            scroll_index += 1;

            let full_layer_width = layer_width << 7;
            if chunk_x < 0 {
                chunk_x += full_layer_width;
            }
            if chunk_x >= full_layer_width {
                chunk_x -= full_layer_width;
            }
            let mut chunk_x_pos = chunk_x >> 7;
            let tile_px_x_pos = chunk_x & 0xF;
            let first_count = 16 - tile_px_x_pos;
            let mut chunk = (i32::from(layer.entry(chunk_x_pos, chunk_y)) << 6)
                + ((chunk_x & 0x7F) >> 4)
                + 8 * tile_y;
            let mut line_remain = pitch;
            let mut out_col = 0;

            // First (partial) tile of the row.
            let entry = chunk_entry(render, chunk);
            if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
                draw_tile_row(
                    render,
                    entry,
                    tile_y16,
                    tile_px_x_pos,
                    out_row,
                    out_col,
                    first_count,
                    &colors,
                );
            }
            out_col += first_count;
            line_remain -= first_count;

            // Bulk of the row.
            let mut chunk_tile_x = ((chunk_x & 0x7F) >> 4) + 1;
            let mut tiles_per_line = screenwidth16;
            while tiles_per_line > 0 {
                tiles_per_line -= 1;
                if chunk_tile_x < 8 {
                    chunk += 1;
                } else {
                    chunk_x_pos += 1;
                    if chunk_x_pos == layer_width {
                        chunk_x_pos = 0;
                    }
                    chunk_tile_x = 0;
                    chunk = (i32::from(layer.entry(chunk_x_pos, chunk_y)) << 6) + 8 * tile_y;
                }
                line_remain -= 16;
                let entry = chunk_entry(render, chunk);
                if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
                    draw_tile_row(render, entry, tile_y16, 0, out_row, out_col, 16, &colors);
                }
                out_col += 16;
                chunk_tile_x += 1;
            }

            // Remaining pixels of the row.
            while line_remain > 0 {
                let old = chunk_tile_x;
                chunk_tile_x += 1;
                if old < 8 {
                    chunk += 1;
                } else {
                    chunk_tile_x = 0;
                    chunk_x_pos += 1;
                    if chunk_x_pos == layer_width {
                        chunk_x_pos = 0;
                    }
                    chunk = (i32::from(layer.entry(chunk_x_pos, chunk_y)) << 6) + 8 * tile_y;
                }
                let count = line_remain.min(16);
                line_remain -= count;
                let entry = chunk_entry(render, chunk);
                if let Some(entry) = entry.filter(|entry| entry.visual_plane == above) {
                    draw_tile_row(render, entry, tile_y16, 0, out_row, out_col, count, &colors);
                }
                out_col += count;
            }

            tile_y16 += 1;
            if tile_y16 >= 16 {
                tile_y16 = 0;
                tile_y += 1;
            }
            if tile_y >= 8 {
                chunk_y += 1;
                if chunk_y == layer_height {
                    chunk_y = 0;
                    scroll_index -= 0x80 * layer_height;
                }
                tile_y = 0;
            }
            out_row += 1;
        }
    }
}
