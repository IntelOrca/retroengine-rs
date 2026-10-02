//! Scalar software-rasterizer drawing primitives.
//!
//! These are line-by-line ports of `Drawing.cpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195):
//! sprite blits, flips, scaled/rotated variants, ink effects, rectangles and tint masks. All
//! clipping, per-scanline palette selection and integer truncation match upstream; out-of-range
//! surface reads yield index `0` (transparent) instead of reading adjacent memory, which is the
//! only deviation and only observable with malformed assets.

use retro_core::math::{cos_512, sin_512};

use crate::framebuffer::{Framebuffer, rgb888_to_rgb565};
use crate::lookup::LookupTables;
use crate::palette::PaletteState;
use crate::surface::Surface;

/// `FLIP_NONE`.
pub const FLIP_NONE: u8 = 0;
/// `FLIP_X`.
pub const FLIP_X: u8 = 1;
/// `FLIP_Y`.
pub const FLIP_Y: u8 = 2;
/// `FLIP_XY`.
pub const FLIP_XY: u8 = 3;

/// Bundles the framebuffer with the palette and lookup tables a draw call reads.
pub struct Canvas<'a> {
    /// Destination framebuffer.
    pub framebuffer: &'a mut Framebuffer,
    /// Palette banks and the per-scanline bank selection.
    pub palette: &'a PaletteState,
    /// Blend/subtract/tint lookup tables.
    pub lookup: &'a LookupTables,
}

impl Canvas<'_> {
    /// `ClearScreen(byte index)`: fills the full `pitch * height` buffer with the active bank's
    /// colour `index`.
    pub fn clear_screen(&mut self, index: u8) {
        let color = self.palette.active_colors()[usize::from(index)];
        self.framebuffer.clear(color);
    }

    /// `DrawRectangle`.
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
        let alpha = alpha.min(0xFF);
        let pitch = self.framebuffer.pitch() as i32;
        let screen_height = self.framebuffer.height() as i32;
        let (mut width, mut x_pos) = (width, x_pos);
        if width + x_pos > pitch {
            width = pitch - x_pos;
        }
        if x_pos < 0 {
            width += x_pos;
            x_pos = 0;
        }
        let (mut height, mut y_pos) = (height, y_pos);
        if height + y_pos > screen_height {
            height = screen_height - y_pos;
        }
        if y_pos < 0 {
            height += y_pos;
            y_pos = 0;
        }
        if width <= 0 || height <= 0 || alpha <= 0 {
            return;
        }
        let color = rgb888_to_rgb565(r as u8, g as u8, b as u8);
        if alpha == 0xFF {
            for y in y_pos..y_pos + height {
                for x in x_pos..x_pos + width {
                    self.framebuffer.set(x, y, color);
                }
            }
            return;
        }
        let fbuffer_blend = 0x20 * (0xFF - alpha) as usize;
        let pixel_blend = 0x20 * alpha as usize;
        for y in y_pos..y_pos + height {
            for x in x_pos..x_pos + width {
                let pixel = self.framebuffer.get(x, y);
                self.framebuffer.set(
                    x,
                    y,
                    blend_pixel(self.lookup, pixel, color, fbuffer_blend, pixel_blend),
                );
            }
        }
    }

    /// `DrawTintRectangle`.
    pub fn draw_tint_rect(&mut self, x_pos: i32, y_pos: i32, width: i32, height: i32) {
        let pitch = self.framebuffer.pitch() as i32;
        let screen_height = self.framebuffer.height() as i32;
        let (mut width, mut x_pos) = (width, x_pos);
        if width + x_pos > pitch {
            width = pitch - x_pos;
        }
        if x_pos < 0 {
            width += x_pos;
            x_pos = 0;
        }
        let (mut height, mut y_pos) = (height, y_pos);
        if height + y_pos > screen_height {
            height = screen_height - y_pos;
        }
        if y_pos < 0 {
            height += y_pos;
            y_pos = 0;
        }
        if width < 0 || height < 0 {
            return;
        }
        for y in y_pos..y_pos + height {
            for x in x_pos..x_pos + width {
                let pixel = self.framebuffer.get(x, y);
                self.framebuffer.set(x, y, self.lookup.tint(pixel));
            }
        }
    }

    /// `DrawSprite`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Some((width, height, x_pos, y_pos, spr_x, spr_y)) = clip_sprite(
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
            x_pos,
            y_pos,
            width,
            height,
            spr_x,
            spr_y,
        ) else {
            return;
        };
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = surface.pixel(spr_x + column, spr_y + row);
                if index > 0 {
                    self.framebuffer
                        .set(x_pos + column, y_pos + row, colors[usize::from(index)]);
                }
            }
        }
    }

    /// `DrawSpriteFlipped`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_flipped(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        direction: u8,
    ) {
        let pitch = self.framebuffer.pitch() as i32;
        let screen_height = self.framebuffer.height() as i32;
        let width_flip = width;
        let height_flip = height;
        let (mut width, mut x_pos, mut spr_x) = (width, x_pos, spr_x);
        if width + x_pos > pitch {
            width = pitch - x_pos;
        }
        let mut width_flip = width_flip;
        if x_pos < 0 {
            spr_x -= x_pos;
            width += x_pos;
            width_flip += x_pos + x_pos;
            x_pos = 0;
        }
        let (mut height, mut y_pos, mut spr_y) = (height, y_pos, spr_y);
        if height + y_pos > screen_height {
            height = screen_height - y_pos;
        }
        let mut height_flip = height_flip;
        if y_pos < 0 {
            spr_y -= y_pos;
            height += y_pos;
            height_flip += y_pos + y_pos;
            y_pos = 0;
        }
        if width <= 0 || height <= 0 {
            return;
        }
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = match direction {
                    FLIP_X => surface.pixel(spr_x + width_flip - 1 - column, spr_y + row),
                    FLIP_Y => surface.pixel(spr_x + column, spr_y + height_flip - 1 - row),
                    FLIP_XY => surface.pixel(
                        spr_x + width_flip - 1 - column,
                        spr_y + height_flip - 1 - row,
                    ),
                    _ => surface.pixel(spr_x + column, spr_y + row),
                };
                if index > 0 {
                    self.framebuffer
                        .set(x_pos + column, y_pos + row, colors[usize::from(index)]);
                }
            }
        }
    }

    /// `DrawSpriteScaled`: `direction == FLIP_X` mirrors the sampling order exactly like
    /// upstream; other directions use the non-mirrored path.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_scaled(
        &mut self,
        surface: &Surface,
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
        let Some(scaled) = self.setup_scaled(
            direction,
            x_pos,
            y_pos,
            pivot_x,
            pivot_y,
            scale_x,
            scale_y,
            width,
            height,
            spr_x,
            spr_y,
            surface.width,
        ) else {
            return;
        };
        let ScaledGeometry {
            direction,
            true_x_pos,
            true_y_pos,
            width,
            height,
            final_scale_x,
            final_scale_y,
            rounded_x_pos,
            rounded_y_pos,
            width_m1,
            cursor,
        } = scaled;
        let surface_width = surface.width;
        let mut cursor = cursor;
        let mut rounded_y_pos = rounded_y_pos;
        if direction == FLIP_X {
            // Upstream starts the mirrored cursor at `widthM1` into the source row
            // (`Drawing.cpp:3157`).
            cursor = cursor.wrapping_add(width_m1);
            let mut gfx_pitch = 0i32;
            for row in 0..height {
                let colors = self.palette.line_colors((true_y_pos + row) as usize);
                let mut round_x_pos = rounded_x_pos;
                for column in 0..width {
                    let index = surface.pixel_at_offset(cursor);
                    if index > 0 {
                        self.framebuffer.set(
                            true_x_pos + column,
                            true_y_pos + row,
                            colors[usize::from(index)],
                        );
                    }
                    let offset_x = final_scale_x.wrapping_add(round_x_pos);
                    cursor = cursor.wrapping_sub(offset_x >> 11);
                    gfx_pitch = gfx_pitch.wrapping_add(offset_x >> 11);
                    round_x_pos = offset_x & 0x7FF;
                }
                let offset_y = final_scale_y.wrapping_add(rounded_y_pos);
                cursor = cursor
                    .wrapping_add(gfx_pitch)
                    .wrapping_add((offset_y >> 11).wrapping_mul(surface_width));
                rounded_y_pos = offset_y & 0x7FF;
                gfx_pitch = 0;
            }
        } else {
            let mut gfx_pitch = 0i32;
            for row in 0..height {
                let colors = self.palette.line_colors((true_y_pos + row) as usize);
                let mut round_x_pos = rounded_x_pos;
                for column in 0..width {
                    let index = surface.pixel_at_offset(cursor);
                    if index > 0 {
                        self.framebuffer.set(
                            true_x_pos + column,
                            true_y_pos + row,
                            colors[usize::from(index)],
                        );
                    }
                    let offset_x = final_scale_x.wrapping_add(round_x_pos);
                    cursor = cursor.wrapping_add(offset_x >> 11);
                    gfx_pitch = gfx_pitch.wrapping_add(offset_x >> 11);
                    round_x_pos = offset_x & 0x7FF;
                }
                let offset_y = final_scale_y.wrapping_add(rounded_y_pos);
                cursor = cursor
                    .wrapping_add((offset_y >> 11).wrapping_mul(surface_width))
                    .wrapping_sub(gfx_pitch);
                rounded_y_pos = offset_y & 0x7FF;
                gfx_pitch = 0;
            }
        }
    }

    /// `DrawScaledTintMask`: scaled sprite that tints the framebuffer under opaque pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scaled_tint_mask(
        &mut self,
        surface: &Surface,
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
        let Some(scaled) = self.setup_scaled(
            direction,
            x_pos,
            y_pos,
            pivot_x,
            pivot_y,
            scale_x,
            scale_y,
            width,
            height,
            spr_x,
            spr_y,
            surface.width,
        ) else {
            return;
        };
        let ScaledGeometry {
            direction,
            true_x_pos,
            true_y_pos,
            width,
            height,
            final_scale_x,
            final_scale_y,
            rounded_x_pos,
            rounded_y_pos,
            width_m1,
            cursor,
        } = scaled;
        let surface_width = surface.width;
        let mut cursor = cursor;
        let mut rounded_y_pos = rounded_y_pos;
        if direction == FLIP_X {
            // Upstream starts the mirrored cursor at `widthM1` into the source row
            // (`Drawing.cpp:2848`).
            cursor = cursor.wrapping_add(width_m1);
            let mut gfx_pitch = 0i32;
            for row in 0..height {
                let mut round_x_pos = rounded_x_pos;
                for column in 0..width {
                    if surface.pixel_at_offset(cursor) > 0 {
                        let pixel = self.framebuffer.get(true_x_pos + column, true_y_pos + row);
                        self.framebuffer.set(
                            true_x_pos + column,
                            true_y_pos + row,
                            self.lookup.tint(pixel),
                        );
                    }
                    let offset_x = final_scale_x.wrapping_add(round_x_pos);
                    cursor = cursor.wrapping_sub(offset_x >> 11);
                    gfx_pitch = gfx_pitch.wrapping_add(offset_x >> 11);
                    round_x_pos = offset_x & 0x7FF;
                }
                let offset_y = final_scale_y.wrapping_add(rounded_y_pos);
                cursor = cursor
                    .wrapping_add(gfx_pitch)
                    .wrapping_add((offset_y >> 11).wrapping_mul(surface_width));
                rounded_y_pos = offset_y & 0x7FF;
                gfx_pitch = 0;
            }
        } else {
            let mut gfx_pitch = 0i32;
            for row in 0..height {
                let mut round_x_pos = rounded_x_pos;
                for column in 0..width {
                    if surface.pixel_at_offset(cursor) > 0 {
                        let pixel = self.framebuffer.get(true_x_pos + column, true_y_pos + row);
                        self.framebuffer.set(
                            true_x_pos + column,
                            true_y_pos + row,
                            self.lookup.tint(pixel),
                        );
                    }
                    let offset_x = final_scale_x.wrapping_add(round_x_pos);
                    cursor = cursor.wrapping_add(offset_x >> 11);
                    gfx_pitch = gfx_pitch.wrapping_add(offset_x >> 11);
                    round_x_pos = offset_x & 0x7FF;
                }
                let offset_y = final_scale_y.wrapping_add(rounded_y_pos);
                cursor = cursor
                    .wrapping_add((offset_y >> 11).wrapping_mul(surface_width))
                    .wrapping_sub(gfx_pitch);
                rounded_y_pos = offset_y & 0x7FF;
                gfx_pitch = 0;
            }
        }
    }

    /// Computes the shared clipping/scale setup of the two scaled draw paths.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn setup_scaled(
        &self,
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
        surface_width: i32,
    ) -> Option<ScaledGeometry> {
        let true_scale_x = 4i32.wrapping_mul(scale_x);
        let true_scale_y = 4i32.wrapping_mul(scale_y);
        let mut width_m1 = width.wrapping_sub(1);
        let mut true_x_pos = x_pos.wrapping_sub(true_scale_x.wrapping_mul(pivot_x) >> 11);
        let mut width = true_scale_x.wrapping_mul(width) >> 11;
        let mut true_y_pos = y_pos.wrapping_sub(true_scale_y.wrapping_mul(pivot_y) >> 11);
        let mut height = true_scale_y.wrapping_mul(height) >> 11;
        let final_scale_x = final_scale(true_scale_x);
        let final_scale_y = final_scale(true_scale_y);
        let mut rounded_x_pos = 0i32;
        let mut rounded_y_pos = 0i32;
        let mut spr_x = spr_x;
        let mut spr_y = spr_y;
        let pitch = self.framebuffer.pitch() as i32;
        let screen_height = self.framebuffer.height() as i32;
        if width + true_x_pos > pitch {
            width = pitch - true_x_pos;
        }
        if direction != 0 {
            if true_x_pos < 0 {
                width_m1 = width_m1.wrapping_sub(true_x_pos.wrapping_mul(-final_scale_x) >> 11);
                rounded_x_pos =
                    (true_x_pos as u16 as i32).wrapping_mul(-(final_scale_x as i16 as i32)) & 0x7FF;
                width += true_x_pos;
                true_x_pos = 0;
            }
        } else if true_x_pos < 0 {
            spr_x = spr_x.wrapping_add(true_x_pos.wrapping_mul(-final_scale_x) >> 11);
            rounded_x_pos =
                (true_x_pos as u16 as i32).wrapping_mul(-(final_scale_x as i16 as i32)) & 0x7FF;
            width += true_x_pos;
            true_x_pos = 0;
        }
        if height + true_y_pos > screen_height {
            height = screen_height - true_y_pos;
        }
        if true_y_pos < 0 {
            spr_y = spr_y.wrapping_add(true_y_pos.wrapping_mul(-final_scale_y) >> 11);
            rounded_y_pos =
                (true_y_pos as u16 as i32).wrapping_mul(-(final_scale_y as i16 as i32)) & 0x7FF;
            height += true_y_pos;
            true_y_pos = 0;
        }
        if width <= 0 || height <= 0 {
            return None;
        }
        Some(ScaledGeometry {
            direction,
            true_x_pos,
            true_y_pos,
            width,
            height,
            final_scale_x,
            final_scale_y,
            rounded_x_pos,
            rounded_y_pos,
            width_m1,
            cursor: spr_x.wrapping_add(spr_y.wrapping_mul(surface_width)),
        })
    }

    /// `DrawSpriteRotated`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_rotated(
        &mut self,
        surface: &Surface,
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
        let spr_x_pos = pivot_x.wrapping_add(spr_x) << 9;
        let mut spr_y_pos = pivot_y.wrapping_add(spr_y) << 9;
        let mut full_width = width.wrapping_add(spr_x);
        let mut full_height = height.wrapping_add(spr_y);
        let mut angle = rotation & 0x1FF;
        if angle < 0 {
            angle += 0x200;
        }
        if angle != 0 {
            angle = 0x200 - angle;
        }
        let sine = sin_512(angle);
        let cosine = cos_512(angle);
        let (left, right, top, bottom) = rotated_bounds(
            direction,
            x_pos,
            y_pos,
            pivot_x,
            pivot_y,
            width,
            height,
            sine,
            cosine,
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
        );
        let max_x = right - left;
        let max_y = bottom - top;
        if max_x <= 0 || max_y <= 0 {
            return;
        }
        let start_x = left - x_pos;
        let start_y = top - y_pos;
        let shift_pivot = (spr_x << 9) - 1;
        full_width <<= 9;
        let shift_height = (spr_y << 9) - 1;
        full_height <<= 9;
        if cosine < 0 || sine < 0 {
            spr_y_pos = spr_y_pos.wrapping_add(sine).wrapping_add(cosine);
        }
        if direction == FLIP_X {
            let mut draw_x = spr_x_pos
                .wrapping_sub(
                    cosine
                        .wrapping_mul(start_x)
                        .wrapping_sub(sine.wrapping_mul(start_y)),
                )
                .wrapping_sub(0x100);
            let mut draw_y = cosine
                .wrapping_mul(start_y)
                .wrapping_add(spr_y_pos)
                .wrapping_add(sine.wrapping_mul(start_x));
            for row in 0..max_y {
                let colors = self.palette.line_colors((top + row) as usize);
                let mut final_x = draw_x;
                let mut final_y = draw_y;
                for column in 0..max_x {
                    if final_x > shift_pivot
                        && final_x < full_width
                        && final_y > shift_height
                        && final_y < full_height
                    {
                        let index = rotated_pixel(surface, final_x, final_y);
                        if index > 0 {
                            self.framebuffer.set(
                                left + column,
                                top + row,
                                colors[usize::from(index)],
                            );
                        }
                    }
                    final_x = final_x.wrapping_sub(cosine);
                    final_y = final_y.wrapping_add(sine);
                }
                draw_x = draw_x.wrapping_add(sine);
                draw_y = draw_y.wrapping_add(cosine);
            }
        } else {
            let mut draw_x = spr_x_pos
                .wrapping_add(cosine.wrapping_mul(start_x))
                .wrapping_sub(sine.wrapping_mul(start_y));
            let mut draw_y = cosine
                .wrapping_mul(start_y)
                .wrapping_add(spr_y_pos)
                .wrapping_add(sine.wrapping_mul(start_x));
            for row in 0..max_y {
                let colors = self.palette.line_colors((top + row) as usize);
                let mut final_x = draw_x;
                let mut final_y = draw_y;
                for column in 0..max_x {
                    if final_x > shift_pivot
                        && final_x < full_width
                        && final_y > shift_height
                        && final_y < full_height
                    {
                        let index = rotated_pixel(surface, final_x, final_y);
                        if index > 0 {
                            self.framebuffer.set(
                                left + column,
                                top + row,
                                colors[usize::from(index)],
                            );
                        }
                    }
                    final_x = final_x.wrapping_add(cosine);
                    final_y = final_y.wrapping_add(sine);
                }
                draw_x = draw_x.wrapping_sub(sine);
                draw_y = draw_y.wrapping_add(cosine);
            }
        }
    }

    /// `DrawSpriteRotozoom`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sprite_rotozoom(
        &mut self,
        surface: &Surface,
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
        if scale == 0 {
            return;
        }
        let spr_x_pos = pivot_x.wrapping_add(spr_x) << 9;
        let mut spr_y_pos = pivot_y.wrapping_add(spr_y) << 9;
        let mut full_width = width.wrapping_add(spr_x);
        let mut full_height = height.wrapping_add(spr_y);
        let mut angle = rotation & 0x1FF;
        if angle < 0 {
            angle += 0x200;
        }
        if angle != 0 {
            angle = 0x200 - angle;
        }
        let mut sine = scale.wrapping_mul(sin_512(angle)) >> 9;
        let mut cosine = scale.wrapping_mul(cos_512(angle)) >> 9;
        let (left, right, top, bottom) = rotated_bounds(
            direction,
            x_pos,
            y_pos,
            pivot_x,
            pivot_y,
            width,
            height,
            sine,
            cosine,
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
        );
        let max_x = right - left;
        let max_y = bottom - top;
        let true_scale = final_scale_512(scale);
        sine = true_scale.wrapping_mul(sin_512(angle)) >> 9;
        cosine = true_scale.wrapping_mul(cos_512(angle)) >> 9;
        if max_x <= 0 || max_y <= 0 {
            return;
        }
        let start_x = left - x_pos;
        let start_y = top - y_pos;
        let shift_pivot = (spr_x << 9) - 1;
        full_width <<= 9;
        let shift_height = (spr_y << 9) - 1;
        full_height <<= 9;
        if cosine < 0 || sine < 0 {
            spr_y_pos = spr_y_pos.wrapping_add(sine).wrapping_add(cosine);
        }
        if direction == FLIP_X {
            let mut draw_x = spr_x_pos
                .wrapping_sub(
                    cosine
                        .wrapping_mul(start_x)
                        .wrapping_sub(sine.wrapping_mul(start_y)),
                )
                .wrapping_sub(true_scale >> 1);
            let mut draw_y = cosine
                .wrapping_mul(start_y)
                .wrapping_add(spr_y_pos)
                .wrapping_add(sine.wrapping_mul(start_x));
            for row in 0..max_y {
                let colors = self.palette.line_colors((top + row) as usize);
                let mut final_x = draw_x;
                let mut final_y = draw_y;
                for column in 0..max_x {
                    if final_x > shift_pivot
                        && final_x < full_width
                        && final_y > shift_height
                        && final_y < full_height
                    {
                        let index = rotated_pixel(surface, final_x, final_y);
                        if index > 0 {
                            self.framebuffer.set(
                                left + column,
                                top + row,
                                colors[usize::from(index)],
                            );
                        }
                    }
                    final_x = final_x.wrapping_sub(cosine);
                    final_y = final_y.wrapping_add(sine);
                }
                draw_x = draw_x.wrapping_add(sine);
                draw_y = draw_y.wrapping_add(cosine);
            }
        } else {
            let mut draw_x = spr_x_pos
                .wrapping_add(cosine.wrapping_mul(start_x))
                .wrapping_sub(sine.wrapping_mul(start_y));
            let mut draw_y = cosine
                .wrapping_mul(start_y)
                .wrapping_add(spr_y_pos)
                .wrapping_add(sine.wrapping_mul(start_x));
            for row in 0..max_y {
                let colors = self.palette.line_colors((top + row) as usize);
                let mut final_x = draw_x;
                let mut final_y = draw_y;
                for column in 0..max_x {
                    if final_x > shift_pivot
                        && final_x < full_width
                        && final_y > shift_height
                        && final_y < full_height
                    {
                        let index = rotated_pixel(surface, final_x, final_y);
                        if index > 0 {
                            self.framebuffer.set(
                                left + column,
                                top + row,
                                colors[usize::from(index)],
                            );
                        }
                    }
                    final_x = final_x.wrapping_add(cosine);
                    final_y = final_y.wrapping_add(sine);
                }
                draw_x = draw_x.wrapping_sub(sine);
                draw_y = draw_y.wrapping_add(cosine);
            }
        }
    }

    /// `DrawBlendedSprite`: 50% blend with the framebuffer.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_blended_sprite(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
    ) {
        let Some((width, height, x_pos, y_pos, spr_x, spr_y)) = clip_sprite(
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
            x_pos,
            y_pos,
            width,
            height,
            spr_x,
            spr_y,
        ) else {
            return;
        };
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = surface.pixel(spr_x + column, spr_y + row);
                if index > 0 {
                    let color = colors[usize::from(index)];
                    let pixel = self.framebuffer.get(x_pos + column, y_pos + row);
                    self.framebuffer.set(
                        x_pos + column,
                        y_pos + row,
                        ((color & 0xF7DE) >> 1) + ((pixel & 0xF7DE) >> 1),
                    );
                }
            }
        }
    }

    /// `DrawAlphaBlendedSprite`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_alpha_blended_sprite(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let alpha = alpha.min(0xFF);
        let Some((width, height, x_pos, y_pos, spr_x, spr_y)) = clip_sprite(
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
            x_pos,
            y_pos,
            width,
            height,
            spr_x,
            spr_y,
        ) else {
            return;
        };
        if alpha <= 0 {
            return;
        }
        if alpha == 0xFF {
            for row in 0..height {
                let colors = self.palette.line_colors((y_pos + row) as usize);
                for column in 0..width {
                    let index = surface.pixel(spr_x + column, spr_y + row);
                    if index > 0 {
                        self.framebuffer.set(
                            x_pos + column,
                            y_pos + row,
                            colors[usize::from(index)],
                        );
                    }
                }
            }
            return;
        }
        let fbuffer_blend = 0x20 * (0xFF - alpha) as usize;
        let pixel_blend = 0x20 * alpha as usize;
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = surface.pixel(spr_x + column, spr_y + row);
                if index > 0 {
                    let color = colors[usize::from(index)];
                    let pixel = self.framebuffer.get(x_pos + column, y_pos + row);
                    self.framebuffer.set(
                        x_pos + column,
                        y_pos + row,
                        blend_pixel(self.lookup, pixel, color, fbuffer_blend, pixel_blend),
                    );
                }
            }
        }
    }

    /// `DrawAdditiveBlendedSprite`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_additive_blended_sprite(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let alpha = alpha.min(0xFF);
        let Some((width, height, x_pos, y_pos, spr_x, spr_y)) = clip_sprite(
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
            x_pos,
            y_pos,
            width,
            height,
            spr_x,
            spr_y,
        ) else {
            return;
        };
        if alpha <= 0 {
            return;
        }
        let table = 0x20 * alpha as usize;
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = surface.pixel(spr_x + column, spr_y + row);
                if index > 0 {
                    let color = usize::from(colors[usize::from(index)]);
                    let pixel = usize::from(self.framebuffer.get(x_pos + column, y_pos + row));
                    let r = ((usize::from(
                        self.lookup
                            .blend
                            .get(((color & 0xF800) >> 11) + table)
                            .copied()
                            .unwrap_or(0),
                    ) << 11)
                        + (pixel & 0xF800))
                        .min(0xF800);
                    let g = ((usize::from(
                        self.lookup
                            .blend
                            .get(((color & 0x7E0) >> 6) + table)
                            .copied()
                            .unwrap_or(0),
                    ) << 6)
                        + (pixel & 0x7E0))
                        .min(0x7E0);
                    let b = (usize::from(
                        self.lookup
                            .blend
                            .get((color & 0x1F) + table)
                            .copied()
                            .unwrap_or(0),
                    ) + (pixel & 0x1F))
                        .min(0x1F);
                    self.framebuffer
                        .set(x_pos + column, y_pos + row, (r | g | b) as u16);
                }
            }
        }
    }

    /// `DrawSubtractiveBlendedSprite`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_subtractive_blended_sprite(
        &mut self,
        surface: &Surface,
        x_pos: i32,
        y_pos: i32,
        width: i32,
        height: i32,
        spr_x: i32,
        spr_y: i32,
        alpha: i32,
    ) {
        let alpha = alpha.min(0xFF);
        let Some((width, height, x_pos, y_pos, spr_x, spr_y)) = clip_sprite(
            self.framebuffer.pitch() as i32,
            self.framebuffer.height() as i32,
            x_pos,
            y_pos,
            width,
            height,
            spr_x,
            spr_y,
        ) else {
            return;
        };
        if alpha <= 0 {
            return;
        }
        let table = 0x20 * alpha as usize;
        for row in 0..height {
            let colors = self.palette.line_colors((y_pos + row) as usize);
            for column in 0..width {
                let index = surface.pixel(spr_x + column, spr_y + row);
                if index > 0 {
                    let color = usize::from(colors[usize::from(index)]);
                    let pixel = usize::from(self.framebuffer.get(x_pos + column, y_pos + row));
                    let r = (pixel & 0xF800).saturating_sub(
                        usize::from(
                            self.lookup
                                .subtract
                                .get(((color & 0xF800) >> 11) + table)
                                .copied()
                                .unwrap_or(0),
                        ) << 11,
                    );
                    let g = (pixel & 0x7E0).saturating_sub(
                        usize::from(
                            self.lookup
                                .subtract
                                .get(((color & 0x7E0) >> 6) + table)
                                .copied()
                                .unwrap_or(0),
                        ) << 6,
                    );
                    let b = (pixel & 0x1F).saturating_sub(usize::from(
                        self.lookup
                            .subtract
                            .get((color & 0x1F) + table)
                            .copied()
                            .unwrap_or(0),
                    ));
                    self.framebuffer
                        .set(x_pos + column, y_pos + row, (r | g | b) as u16);
                }
            }
        }
    }
}

/// Shared clipping for the ink-effect sprite paths.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn clip_sprite(
    pitch: i32,
    screen_height: i32,
    x_pos: i32,
    y_pos: i32,
    width: i32,
    height: i32,
    spr_x: i32,
    spr_y: i32,
) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let (mut width, mut x_pos, mut spr_x) = (width, x_pos, spr_x);
    if width + x_pos > pitch {
        width = pitch - x_pos;
    }
    if x_pos < 0 {
        spr_x -= x_pos;
        width += x_pos;
        x_pos = 0;
    }
    let (mut height, mut y_pos, mut spr_y) = (height, y_pos, spr_y);
    if height + y_pos > screen_height {
        height = screen_height - y_pos;
    }
    if y_pos < 0 {
        spr_y -= y_pos;
        height += y_pos;
        y_pos = 0;
    }
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((width, height, x_pos, y_pos, spr_x, spr_y))
}

/// The `finalscale` float expression from the scaled draw paths.
fn final_scale(true_scale: i32) -> i32 {
    ((2048.0f32 / true_scale as f32) * 2048.0) as i32
}

/// The `truescale` float expression from the rotozoom draw path.
fn final_scale_512(scale: i32) -> i32 {
    ((512.0f32 / scale as f32) * 512.0) as i32
}

/// The mutable state of a scaled draw, output of [`Canvas::setup_scaled`].
struct ScaledGeometry {
    direction: u8,
    true_x_pos: i32,
    true_y_pos: i32,
    width: i32,
    height: i32,
    final_scale_x: i32,
    final_scale_y: i32,
    rounded_x_pos: i32,
    rounded_y_pos: i32,
    width_m1: i32,
    cursor: i32,
}

/// Computes the clipped rotated sprite bounds exactly like `DrawSpriteRotated`.
#[allow(clippy::too_many_arguments)]
fn rotated_bounds(
    direction: u8,
    x_pos: i32,
    y_pos: i32,
    pivot_x: i32,
    pivot_y: i32,
    width: i32,
    height: i32,
    sine: i32,
    cosine: i32,
    pitch: i32,
    screen_height: i32,
) -> (i32, i32, i32, i32) {
    let mut x_positions = [0i32; 4];
    let mut y_positions = [0i32; 4];
    if direction == FLIP_X {
        x_positions[0] = x_pos.wrapping_add(
            sine.wrapping_mul(-pivot_y - 2)
                .wrapping_add(cosine.wrapping_mul(pivot_x + 2))
                >> 9,
        );
        y_positions[0] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(-pivot_y - 2)
                .wrapping_sub(sine.wrapping_mul(pivot_x + 2))
                >> 9,
        );
        x_positions[1] = x_pos.wrapping_add(
            sine.wrapping_mul(-pivot_y - 2)
                .wrapping_add(cosine.wrapping_mul(pivot_x - width - 2))
                >> 9,
        );
        y_positions[1] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(-pivot_y - 2)
                .wrapping_sub(sine.wrapping_mul(pivot_x - width - 2))
                >> 9,
        );
        x_positions[2] = x_pos.wrapping_add(
            sine.wrapping_mul(height - pivot_y + 2)
                .wrapping_add(cosine.wrapping_mul(pivot_x + 2))
                >> 9,
        );
        y_positions[2] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(height - pivot_y + 2)
                .wrapping_sub(sine.wrapping_mul(pivot_x + 2))
                >> 9,
        );
        let a = pivot_x - width - 2;
        let b = height - pivot_y + 2;
        x_positions[3] =
            x_pos.wrapping_add(sine.wrapping_mul(b).wrapping_add(cosine.wrapping_mul(a)) >> 9);
        y_positions[3] =
            y_pos.wrapping_add(cosine.wrapping_mul(b).wrapping_sub(sine.wrapping_mul(a)) >> 9);
    } else {
        x_positions[0] = x_pos.wrapping_add(
            sine.wrapping_mul(-pivot_y - 2)
                .wrapping_add(cosine.wrapping_mul(-pivot_x - 2))
                >> 9,
        );
        y_positions[0] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(-pivot_y - 2)
                .wrapping_sub(sine.wrapping_mul(-pivot_x - 2))
                >> 9,
        );
        x_positions[1] = x_pos.wrapping_add(
            sine.wrapping_mul(-pivot_y - 2)
                .wrapping_add(cosine.wrapping_mul(width - pivot_x + 2))
                >> 9,
        );
        y_positions[1] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(-pivot_y - 2)
                .wrapping_sub(sine.wrapping_mul(width - pivot_x + 2))
                >> 9,
        );
        x_positions[2] = x_pos.wrapping_add(
            sine.wrapping_mul(height - pivot_y + 2)
                .wrapping_add(cosine.wrapping_mul(-pivot_x - 2))
                >> 9,
        );
        y_positions[2] = y_pos.wrapping_add(
            cosine
                .wrapping_mul(height - pivot_y + 2)
                .wrapping_sub(sine.wrapping_mul(-pivot_x - 2))
                >> 9,
        );
        let a = width - pivot_x + 2;
        let b = height - pivot_y + 2;
        x_positions[3] =
            x_pos.wrapping_add(sine.wrapping_mul(b).wrapping_add(cosine.wrapping_mul(a)) >> 9);
        y_positions[3] =
            y_pos.wrapping_add(cosine.wrapping_mul(b).wrapping_sub(sine.wrapping_mul(a)) >> 9);
    }
    let left = x_positions.iter().copied().min().unwrap_or(0).max(0);
    let right = x_positions.iter().copied().max().unwrap_or(0).min(pitch);
    let top = y_positions.iter().copied().min().unwrap_or(0).max(0);
    let bottom = y_positions
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .min(screen_height);
    (left, right, top, bottom)
}

/// Samples the rotated/rotozoom source cursor with upstream's row stride
/// `(y << surface.width_shift) + x` (`Drawing.cpp:3362`), i.e. the rounded-down power-of-two
/// width from `Surface::width_shift`, not the exact surface width.
fn rotated_pixel(surface: &Surface, final_x: i32, final_y: i32) -> u8 {
    let x = final_x >> 9;
    let y = final_y >> 9;
    let offset = y
        .wrapping_shl(surface.width_shift.max(0) as u32)
        .wrapping_add(x);
    surface.pixel_at_offset(offset)
}

/// Blends one framebuffer pixel with a source colour through the blend table.
fn blend_pixel(
    lookup: &LookupTables,
    framebuffer: u16,
    color: u16,
    fbuffer_blend: usize,
    pixel_blend: usize,
) -> u16 {
    let r = ((u32::from(
        lookup
            .blend
            .get(fbuffer_blend + (((framebuffer as usize) & 0xF800) >> 11))
            .copied()
            .unwrap_or(0),
    ) + u32::from(
        lookup
            .blend
            .get(pixel_blend + (((color as usize) & 0xF800) >> 11))
            .copied()
            .unwrap_or(0),
    )) << 11) as u16;
    let g = ((u32::from(
        lookup
            .blend
            .get(fbuffer_blend + (((framebuffer as usize) & 0x7E0) >> 6))
            .copied()
            .unwrap_or(0),
    ) + u32::from(
        lookup
            .blend
            .get(pixel_blend + (((color as usize) & 0x7E0) >> 6))
            .copied()
            .unwrap_or(0),
    )) << 6) as u16;
    let b = (u32::from(
        lookup
            .blend
            .get(fbuffer_blend + ((framebuffer as usize) & 0x1F))
            .copied()
            .unwrap_or(0),
    ) + u32::from(
        lookup
            .blend
            .get(pixel_blend + ((color as usize) & 0x1F))
            .copied()
            .unwrap_or(0),
    )) as u16;
    r | g | b
}
