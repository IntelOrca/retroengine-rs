//! Aggregate software-renderer state owned by the engine.

use crate::SCREEN_HEIGHT;
use crate::draw::Canvas;
use crate::framebuffer::Framebuffer;
use crate::layers::{ChunkEntry, LayerState, ParallaxState, TILE_SET_16_SIZE};
use crate::lookup::LookupTables;
use crate::palette::PaletteState;
use crate::surface::Surface;

/// Maximum number of resident sprite sheets (`SURFACE_COUNT`).
pub const SURFACE_COUNT: usize = 24;
/// Size of one deformation table (`DEFORM_COUNT`).
pub const DEFORM_COUNT: usize = 0x100 + 320;

/// The 128x128 chunk metadata (`tiles128x128`) plus the normalized 16x16 indexed tileset.
#[derive(Clone, Default)]
pub struct TileSet {
    /// One entry per chunk, indexed by the engine's `chunk` value.
    pub chunks: Vec<ChunkEntry>,
    /// `tilesetGFXData`: all 16x16 tiles in indexed colour, transparent normalized to `0`.
    pub pixels: Vec<u8>,
}

/// Everything the software renderer needs besides swappable 3D/game state.
pub struct RenderState {
    /// Destination framebuffer (`Engine.frameBuffer`).
    pub framebuffer: Framebuffer,
    /// Palette banks and per-scanline selection.
    pub palette: PaletteState,
    /// Blend/subtract/tint lookup tables.
    pub lookup: LookupTables,
    /// Resident sprite sheets (`gfxSurface`/`graphicData`).
    pub surfaces: Vec<Surface>,
    /// Tile chunks and indexed tileset.
    pub tiles: TileSet,
    /// `lastXSize`, shared by the horizontal tile layer path.
    pub last_x_size: i32,
    /// `lastYSize`, shared by the vertical tile layer path.
    pub last_y_size: i32,
    /// `waterDrawPos` computed by `DrawStageGFX`.
    pub water_draw_pos: i32,
    /// `bgDeformationData0..3` (`DEFORM_FG`, `DEFORM_FG_WATER`, `DEFORM_BG`, `DEFORM_BG_WATER`).
    pub deform_data: [Vec<i32>; 4],
    /// `fadeMode`.
    pub fade_mode: i32,
    /// `fadeR`.
    pub fade_r: u8,
    /// `fadeG`.
    pub fade_g: u8,
    /// `fadeB`.
    pub fade_b: u8,
    /// `fadeA` (already clamped to `0x100` by `SetFade`).
    pub fade_a: u16,
    /// Display-only dim timer (`Engine.dimTimer`).
    pub dim_timer: i32,
    /// Display-only dim limit (`Engine.dimLimit`).
    pub dim_limit: i32,
    /// Display-only dim max (`Engine.dimMax`).
    pub dim_max: f32,
    /// Display-only dim percent (`Engine.dimPercent`), applied when presenting.
    pub dim_percent: f32,
}

impl RenderState {
    /// Creates a zeroed render state for a `width x height` screen.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let mut tiles = TileSet::default();
        tiles.pixels.resize(TILE_SET_16_SIZE, 0);
        Self {
            framebuffer: Framebuffer::new(width, height),
            palette: PaletteState::new(),
            lookup: LookupTables::new(),
            surfaces: Vec::new(),
            tiles,
            last_x_size: -1,
            last_y_size: -1,
            water_draw_pos: height as i32,
            deform_data: [
                vec![0; DEFORM_COUNT],
                vec![0; DEFORM_COUNT],
                vec![0; DEFORM_COUNT],
                vec![0; DEFORM_COUNT],
            ],
            fade_mode: 0,
            fade_r: 0,
            fade_g: 0,
            fade_b: 0,
            fade_a: 0,
            dim_timer: 0,
            dim_limit: 0,
            dim_max: 1.0,
            dim_percent: 1.0,
        }
    }

    /// Borrows the framebuffer with the palette and lookup tables.
    pub fn canvas(&mut self) -> Canvas<'_> {
        Canvas {
            framebuffer: &mut self.framebuffer,
            palette: &self.palette,
            lookup: &self.lookup,
        }
    }

    /// `DrawSprite` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_sprite(surface, x_pos, y_pos, width, height, spr_x, spr_y);
    }

    /// `DrawSpriteFlipped` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_flipped(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        direction: u8,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_sprite_flipped(
            surface, x_pos, y_pos, width, height, spr_x, spr_y, direction,
        );
    }

    /// `DrawSpriteScaled` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_scaled(
        &mut self,
        sheet_id: i32,
        direction: u8,
        x_pos: i32,
        y_pos: i32,
        pivot_x: i32,
        pivot_y: i32,
        scale_x: i32,
        scale_y: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_sprite_scaled(
            surface, direction, x_pos, y_pos, pivot_x, pivot_y, scale_x, scale_y, width, height,
            spr_x, spr_y,
        );
    }

    /// `DrawScaledTintMask` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scaled_tint_mask(
        &mut self,
        sheet_id: i32,
        direction: u8,
        x_pos: i32,
        y_pos: i32,
        pivot_x: i32,
        pivot_y: i32,
        scale_x: i32,
        scale_y: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_scaled_tint_mask(
            surface, direction, x_pos, y_pos, pivot_x, pivot_y, scale_x, scale_y, width, height,
            spr_x, spr_y,
        );
    }

    /// `DrawSpriteRotated` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_rotated(
        &mut self,
        sheet_id: i32,
        direction: u8,
        x_pos: i32,
        y_pos: i32,
        pivot_x: i32,
        pivot_y: i32,
        spr_x: i32,
        spr_y: i32,
        width: i32,
        height: i32,
        rotation: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_sprite_rotated(
            surface, direction, x_pos, y_pos, pivot_x, pivot_y, spr_x, spr_y, width, height,
            rotation,
        );
    }

    /// `DrawSpriteRotozoom` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_rotozoom(
        &mut self,
        sheet_id: i32,
        direction: u8,
        x_pos: i32,
        y_pos: i32,
        pivot_x: i32,
        pivot_y: i32,
        spr_x: i32,
        spr_y: i32,
        width: i32,
        height: i32,
        rotation: i32,
        scale: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_sprite_rotozoom(
            surface, direction, x_pos, y_pos, pivot_x, pivot_y, spr_x, spr_y, width, height,
            rotation, scale,
        );
    }

    /// `DrawBlendedSprite` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_blended_sprite(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_blended_sprite(surface, x_pos, y_pos, width, height, spr_x, spr_y);
    }

    /// `DrawAlphaBlendedSprite` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_alpha_blended_sprite(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_alpha_blended_sprite(surface, x_pos, y_pos, width, height, spr_x, spr_y, alpha);
    }

    /// `DrawAdditiveBlendedSprite` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_additive_blended_sprite(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_additive_blended_sprite(surface, x_pos, y_pos, width, height, spr_x, spr_y, alpha);
    }

    /// `DrawSubtractiveBlendedSprite` against resident sheet `sheet_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_subtractive_blended_sprite(
        &mut self,
        sheet_id: i32,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let Self {
            framebuffer,
            palette,
            lookup,
            surfaces,
            ..
        } = self;
        let Some(surface) = usize::try_from(sheet_id)
            .ok()
            .and_then(|index| surfaces.get(index))
            .filter(|surface| !surface.is_empty())
        else {
            return;
        };
        Canvas {
            framebuffer,
            palette,
            lookup,
        }
        .draw_subtractive_blended_sprite(surface, x_pos, y_pos, width, height, spr_x, spr_y, alpha);
    }

    /// `DrawRectangle` on the framebuffer.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rect(
        &mut self,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        r: i32,
        g: i32,
        b: i32,
        alpha: i32,
    ) {
        self.canvas()
            .draw_rect(x_pos, y_pos, width, height, r, g, b, alpha);
    }

    /// `DrawTintRectangle` on the framebuffer.
    pub fn draw_tint_rect(&mut self, x_pos: i32, y_pos: i32, width: i32, height: i32) {
        self.canvas().draw_tint_rect(x_pos, y_pos, width, height);
    }

    /// `ClearScreen`.
    pub fn clear_screen(&mut self, index: u8) {
        self.canvas().clear_screen(index);
    }

    /// `ClearGraphicsData`: drops every resident sheet.
    pub fn clear_surfaces(&mut self) {
        self.surfaces.clear();
    }

    /// Returns the resident surface for `sheet_id`.
    #[must_use]
    pub fn surface(&self, sheet_id: i32) -> Option<&Surface> {
        usize::try_from(sheet_id)
            .ok()
            .and_then(|index| self.surfaces.get(index))
            .filter(|surface| !surface.is_empty())
    }

    /// Applies the script `SetScreenFade` state (`SetFade`).
    pub fn set_fade(&mut self, r: i32, g: i32, b: i32, a: u16) {
        self.fade_mode = 1;
        self.fade_r = r as u8;
        self.fade_g = g as u8;
        self.fade_b = b as u8;
        self.fade_a = a.min(0x100);
    }

    /// Draws the current fade rectangle if `fadeMode > 0`.
    pub fn draw_fade(&mut self) {
        if self.fade_mode <= 0 {
            return;
        }
        let width = self.framebuffer.width() as i32;
        let height = self.framebuffer.height() as i32;
        let (r, g, b, a) = (self.fade_r, self.fade_g, self.fade_b, self.fade_a);
        self.canvas().draw_rect(
            0,
            0,
            width,
            height,
            i32::from(r),
            i32::from(g),
            i32::from(b),
            i32::from(a),
        );
    }

    /// Processes display dimming exactly like the `FlipScreen` `dimPercent` update.
    pub fn process_dimming(&mut self) {
        if self.dim_timer < self.dim_limit {
            if self.dim_percent < 1.0 {
                self.dim_percent += 0.05;
                if self.dim_percent > 1.0 {
                    self.dim_percent = 1.0;
                }
            }
        } else if self.dim_percent > 0.25 && self.dim_limit >= 0 {
            self.dim_percent *= 0.9;
        }
    }

    /// The display dim amount (`dimMax * dimPercent`), applied when presenting.
    #[must_use]
    pub fn dim_amount(&self) -> f32 {
        self.dim_max * self.dim_percent
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new(424, SCREEN_HEIGHT)
    }
}

/// Creates fresh layer states with zeroed chunk and line-scroll buffers.
#[must_use]
pub fn new_layers() -> Vec<LayerState> {
    vec![LayerState::default(); crate::LAYER_COUNT]
}

/// Creates a fresh parallax table pair.
#[must_use]
pub fn new_parallax() -> ParallaxState {
    ParallaxState::default()
}
