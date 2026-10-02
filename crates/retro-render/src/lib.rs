//! Software renderer producing the u16 RGB565 framebuffer used as the parity artifact.
//!
//! This crate is the scalar reference implementation of the RSDKv4 `Drawing.cpp` pixel semantics:
//! the framebuffer layout, palette banks and lookup tables, sprite/ink primitives and the
//! per-line tile-layer renderers. The engine keeps all mutable game state; this crate only knows
//! how to turn it into pixels.
#![forbid(unsafe_code)]

pub mod draw;
pub mod framebuffer;
pub mod layers;
pub mod lookup;
pub mod palette;
pub mod state;
pub mod surface;

pub use draw::{Canvas, FLIP_NONE, FLIP_X, FLIP_XY, FLIP_Y};
pub use framebuffer::{Framebuffer, rgb565_to_rgb888, rgb888_to_rgb565};
pub use layers::{
    ChunkEntry, LAYER_3DFLOOR, LAYER_3DSKY, LAYER_HSCROLL, LAYER_NOSCROLL, LAYER_VSCROLL,
    LINE_SCROLL_COUNT, LayerState, LayerView, PARALLAX_COUNT, ParallaxState, TILE_SET_16_SIZE,
};
pub use lookup::LookupTables;
pub use palette::{ACTIVE_PALETTE, PALETTE_BANKS, PALETTE_COLORS, PaletteState};
pub use state::{DEFORM_COUNT, RenderState, SURFACE_COUNT, TileSet, new_layers, new_parallax};
pub use surface::Surface;

/// `SCREEN_YSIZE`.
pub const SCREEN_HEIGHT: usize = 240;
/// `LAYER_COUNT`.
pub const LAYER_COUNT: usize = 9;

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas<'a>(
        framebuffer: &'a mut Framebuffer,
        palette: &'a PaletteState,
        lookup: &'a LookupTables,
    ) -> Canvas<'a> {
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
    }

    fn two_color_palette() -> PaletteState {
        let mut palette = PaletteState::new();
        palette.set_entry(0, 1, 255, 0, 0);
        palette.set_entry(0, 2, 0, 255, 0);
        palette.set_entry(0, 3, 0, 0, 255);
        palette
    }

    #[test]
    fn draw_sprite_none_and_flips() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(2, 1, vec![1, 2]);

        let mut framebuffer = Framebuffer::new(2, 1);
        canvas(&mut framebuffer, &palette, &lookup).draw_sprite(&surface, 0, 0, 2, 1, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0xF800);
        assert_eq!(framebuffer.get(1, 0), 0x07E0);

        let mut framebuffer = Framebuffer::new(2, 1);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_sprite_flipped(&surface, 0, 0, 2, 1, 0, 0, FLIP_X);
        assert_eq!(framebuffer.get(0, 0), 0x07E0);
        assert_eq!(framebuffer.get(1, 0), 0xF800);

        let surface = Surface::from_indexed(1, 2, vec![1, 2]);
        let mut framebuffer = Framebuffer::new(1, 2);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_sprite_flipped(&surface, 0, 0, 1, 2, 0, 0, FLIP_Y);
        assert_eq!(framebuffer.get(0, 0), 0x07E0);
        assert_eq!(framebuffer.get(0, 1), 0xF800);

        let surface = Surface::from_indexed(2, 2, vec![1, 2, 3, 1]);
        let mut framebuffer = Framebuffer::new(2, 2);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_sprite_flipped(&surface, 0, 0, 2, 2, 0, 0, FLIP_XY);
        assert_eq!(framebuffer.get(0, 0), 0xF800);
        assert_eq!(framebuffer.get(1, 0), 0x001F);
        assert_eq!(framebuffer.get(0, 1), 0x07E0);
        assert_eq!(framebuffer.get(1, 1), 0xF800);
    }

    #[test]
    fn sprite_transparency_and_clipping() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(2, 1, vec![0, 2]);

        // Transparent index 0 leaves the framebuffer untouched.
        let mut framebuffer = Framebuffer::new(2, 1);
        framebuffer.clear(0xFFFF);
        canvas(&mut framebuffer, &palette, &lookup).draw_sprite(&surface, 0, 0, 2, 1, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0xFFFF);
        assert_eq!(framebuffer.get(1, 0), 0x07E0);

        // Clipping on the left drops the transparent first pixel and lands the green one at x=0.
        let mut framebuffer = Framebuffer::new(1, 1);
        canvas(&mut framebuffer, &palette, &lookup).draw_sprite(&surface, -1, 0, 2, 1, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0x07E0);

        // Clipping on the right truncates the run.
        let mut framebuffer = Framebuffer::new(1, 1);
        canvas(&mut framebuffer, &palette, &lookup).draw_sprite(&surface, 0, 0, 2, 1, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0);
    }

    #[test]
    fn per_scanline_palette_banks_are_used() {
        let mut palette = two_color_palette();
        palette.set_entry(1, 1, 0, 0, 255);
        palette.set_active_palette(1, 1, 2);
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(1, 2, vec![1, 1]);
        let mut framebuffer = Framebuffer::new(1, 2);
        canvas(&mut framebuffer, &palette, &lookup).draw_sprite(&surface, 0, 0, 1, 2, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0xF800, "row 0 uses bank 0");
        assert_eq!(framebuffer.get(0, 1), 0x001F, "row 1 uses bank 1");
    }

    #[test]
    fn blend_ink_matches_upstream_component_math() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(1, 1, vec![3]); // blue
        let mut framebuffer = Framebuffer::new(1, 1);
        framebuffer.clear(0xF800); // red
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_alpha_blended_sprite(&surface, 0, 0, 1, 1, 0, 0, 0x80);
        assert_eq!(framebuffer.get(0, 0), 0x780F);
    }

    #[test]
    fn additive_ink_matches_upstream_component_math() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(1, 1, vec![3]); // blue
        let mut framebuffer = Framebuffer::new(1, 1);
        framebuffer.clear(0xF800); // red
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_additive_blended_sprite(&surface, 0, 0, 1, 1, 0, 0, 0x80);
        // B = 128*31>>8 = 15, R/G keep the framebuffer bits.
        assert_eq!(framebuffer.get(0, 0), 0xF80F);
    }

    #[test]
    fn subtractive_ink_matches_upstream_component_math() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(1, 1, vec![3]); // blue
        let mut framebuffer = Framebuffer::new(1, 1);
        framebuffer.clear(0xFFFF);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_subtractive_blended_sprite(&surface, 0, 0, 1, 1, 0, 0, 0x80);
        // R loses 0x7800, G loses 0x3C0, B loses nothing.
        assert_eq!(framebuffer.get(0, 0), 0x843F);
    }

    #[test]
    fn blend_ink_is_average() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(1, 1, vec![3]); // blue
        let mut framebuffer = Framebuffer::new(1, 1);
        framebuffer.clear(0xF800); // red
        canvas(&mut framebuffer, &palette, &lookup).draw_blended_sprite(&surface, 0, 0, 1, 1, 0, 0);
        // (0xF800 & 0xF7DE) >> 1 = 0x7800; (0x001F & 0xF7DE) >> 1 = 0x000F.
        assert_eq!(framebuffer.get(0, 0), 0x780F);
    }

    #[test]
    fn tint_rect_and_rect_draws() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let mut framebuffer = Framebuffer::new(2, 1);
        canvas(&mut framebuffer, &palette, &lookup).draw_rect(0, 0, 2, 1, 255, 0, 0, 0xFF);
        assert_eq!(framebuffer.get(0, 0), 0xF800);
        canvas(&mut framebuffer, &palette, &lookup).draw_tint_rect(0, 0, 1, 1);
        assert_eq!(framebuffer.get(0, 0), 0x841 * 16, "pure red tints by 16");
        assert_eq!(framebuffer.get(1, 0), 0xF800, "outside the tint rect");

        canvas(&mut framebuffer, &palette, &lookup).draw_rect(0, 0, 2, 1, 0, 255, 0, 0x80);
        // The tinted red pixel 0x8410 is r=16, g=16, b=16, so the green overlay blends to
        // R = 127*16>>8 = 7 -> 0x3800, G = 7 + 128*31>>8 = 22 -> 0x0580, B = 7.
        assert_eq!(framebuffer.get(0, 0), 0x3D87);
    }

    #[test]
    fn scaled_tint_mask_tints_under_opaque_pixels_only() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(2, 2, vec![1, 1, 1, 1]);
        let mut framebuffer = Framebuffer::new(2, 2);
        framebuffer.clear(0xF800);
        framebuffer.set(0, 1, 0x07E0);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_scaled_tint_mask(&surface, FLIP_NONE, 0, 0, 0, 0, 512, 512, 2, 2, 0, 0);
        // 1:1 scale; the mask tints each covered framebuffer pixel through `tintLookupTable`.
        assert_eq!(framebuffer.get(0, 0), lookup.tint(0xF800));
        assert_eq!(framebuffer.get(1, 0), lookup.tint(0xF800));
        assert_eq!(framebuffer.get(0, 1), lookup.tint(0x07E0));
        assert_eq!(framebuffer.get(1, 1), lookup.tint(0xF800));

        // With no opaque source pixels the framebuffer must be untouched.
        let surface = Surface::from_indexed(2, 2, vec![0, 0, 0, 0]);
        let mut framebuffer = Framebuffer::new(2, 2);
        framebuffer.clear(0xF800);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_scaled_tint_mask(&surface, FLIP_NONE, 0, 0, 0, 0, 512, 512, 2, 2, 0, 0);
        assert_eq!(framebuffer.get(0, 0), 0xF800);
    }

    #[test]
    fn clear_screen_uses_active_palette_index() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let mut framebuffer = Framebuffer::new(2, 2);
        canvas(&mut framebuffer, &palette, &lookup).clear_screen(2);
        assert!(framebuffer.pixels().iter().all(|pixel| *pixel == 0x07E0));
    }

    #[test]
    fn scaled_sprite_smoke_test() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(2, 2, vec![1, 2, 2, 1]);
        let mut framebuffer = Framebuffer::new(4, 4);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_sprite_scaled(&surface, FLIP_NONE, 0, 0, 0, 0, 1024, 1024, 2, 2, 0, 0);
        assert!(framebuffer.pixels().iter().any(|pixel| *pixel != 0));
        let lit = framebuffer
            .pixels()
            .iter()
            .filter(|pixel| **pixel != 0)
            .count();
        assert!(lit >= 4, "scaled sprite should cover its area, got {lit}");
    }

    #[test]
    fn rotated_sprite_smoke_test() {
        let palette = two_color_palette();
        let lookup = LookupTables::new();
        let surface = Surface::from_indexed(4, 4, vec![1; 16]);
        let mut framebuffer = Framebuffer::new(16, 16);
        canvas(&mut framebuffer, &palette, &lookup)
            .draw_sprite_rotated(&surface, FLIP_NONE, 8, 8, 0, 0, 0, 0, 4, 4, 0);
        assert!(framebuffer.pixels().iter().any(|pixel| *pixel != 0));
    }

    #[test]
    fn horizontal_layer_blits_tiles_and_transparency() {
        let palette = two_color_palette();
        let mut render = RenderState::new(32, 2);
        render.palette = palette;
        render.water_draw_pos = 2;
        let mut pixels = vec![0u8; TILE_SET_16_SIZE];
        pixels[..16].copy_from_slice(&[1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        render.tiles.pixels = pixels;
        render.tiles.chunks = vec![ChunkEntry::default()];
        let mut layer = LayerState {
            xsize: 1,
            ysize: 1,
            layer_type: LAYER_HSCROLL,
            ..LayerState::default()
        };
        layer.set_entry(0, 0, 0);
        let mut parallax = ParallaxState::default();
        layers::draw_h_line_scroll_layer(
            &mut render,
            &mut layer,
            &mut parallax,
            LayerView {
                is_background: false,
                above_mid_point: false,
                x_scroll_offset: 0,
                y_scroll_offset: 0,
            },
        );
        // pitch is 40; the tile pattern repeats across the row.
        for x in 0..4 {
            assert_eq!(render.framebuffer.get(x, 0), 0xF800, "x={x}");
        }
        for x in 4..16 {
            assert_eq!(render.framebuffer.get(x, 0), 0, "transparent x={x}");
        }
        assert_eq!(render.framebuffer.get(0, 1), 0, "tile row 1 is transparent");
    }

    #[test]
    fn horizontal_layer_uses_second_chunk_in_bulk_path() {
        let palette = two_color_palette();
        let mut render = RenderState::new(160, 1);
        render.palette = palette;
        render.water_draw_pos = 1;
        let mut pixels = vec![0u8; TILE_SET_16_SIZE];
        pixels[..16].copy_from_slice(&[1; 16]);
        pixels[256..272].copy_from_slice(&[2; 16]);
        render.tiles.pixels = pixels;
        // Chunk 0 occupies tile indices 0..64; chunk 1 tiles start at index 64.
        render.tiles.chunks = vec![ChunkEntry::default(); 128];
        render.tiles.chunks[0] = ChunkEntry {
            gfx_data_pos: 0,
            direction: 0,
            visual_plane: 0,
        };
        for entry in render.tiles.chunks.iter_mut().skip(64) {
            *entry = ChunkEntry {
                gfx_data_pos: 256,
                direction: 0,
                visual_plane: 0,
            };
        }
        // Second chunk column (xsize 2) holds chunk index 1.
        let mut layer = LayerState {
            xsize: 2,
            ysize: 1,
            layer_type: LAYER_HSCROLL,
            ..LayerState::default()
        };
        layer.set_entry(0, 0, 0);
        layer.set_entry(1, 0, 1);
        let mut parallax = ParallaxState::default();
        layers::draw_h_line_scroll_layer(
            &mut render,
            &mut layer,
            &mut parallax,
            LayerView {
                is_background: false,
                above_mid_point: false,
                x_scroll_offset: 0,
                y_scroll_offset: 0,
            },
        );
        assert_eq!(render.framebuffer.get(0, 0), 0xF800);
        // The second 16x16 tile of chunk 0 is empty; chunk 1 starts at pixel 128.
        assert_eq!(render.framebuffer.get(128, 0), 0x07E0);
        assert_eq!(render.framebuffer.get(143, 0), 0x07E0);
    }

    #[test]
    fn vertical_layer_blits_a_column() {
        let palette = two_color_palette();
        let mut render = RenderState::new(2, 32);
        render.palette = palette;
        let mut pixels = vec![0u8; TILE_SET_16_SIZE];
        for row in 0..16 {
            pixels[row * 16] = 1;
        }
        render.tiles.pixels = pixels;
        render.tiles.chunks = vec![ChunkEntry::default()];
        let mut layer = LayerState {
            xsize: 1,
            ysize: 1,
            layer_type: LAYER_VSCROLL,
            ..LayerState::default()
        };
        layer.set_entry(0, 0, 0);
        let mut parallax = ParallaxState::default();
        layers::draw_v_line_scroll_layer(
            &mut render,
            &mut layer,
            &mut parallax,
            LayerView {
                is_background: false,
                above_mid_point: false,
                x_scroll_offset: 0,
                y_scroll_offset: 0,
            },
        );
        for y in 0..16 {
            assert_eq!(render.framebuffer.get(0, y), 0xF800, "y={y}");
            assert_eq!(render.framebuffer.get(1, y), 0, "second column stays blank");
        }
    }
}
