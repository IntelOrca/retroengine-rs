//! Indexed sprite/tile surfaces (`GFXSurface` + `graphicData`).
//!
//! Upstream keeps every decoded GIF in one `graphicData` byte buffer with per-surface
//! `dataPosition`/`width`/`widthShift` metadata. This port keeps each sheet in its own
//! [`Surface`] with bounds-checked lookups; pixel index `0` is transparent for every draw path.

/// One indexed-colour sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// Sheet width in pixels (`GFXSurface::width`).
    pub width: i32,
    /// Sheet height in pixels (`GFXSurface::height`).
    pub height: i32,
    /// `log2(width)` fallback used by the rotated draw paths (`GFXSurface::widthShift`).
    pub width_shift: i32,
    /// `width * height` palette indices in row-major order.
    pub pixels: Vec<u8>,
}

impl Surface {
    /// Builds a surface from indexed pixels.
    #[must_use]
    pub fn from_indexed(width: u16, height: u16, pixels: Vec<u8>) -> Self {
        let expected = usize::from(width) * usize::from(height);
        let mut pixels = pixels;
        pixels.resize(expected, 0);
        let mut width_shift = 0;
        let mut w = width;
        while w > 1 {
            w >>= 1;
            width_shift += 1;
        }
        Self {
            width: i32::from(width),
            height: i32::from(height),
            width_shift,
            pixels,
        }
    }

    /// An empty surface, used for removed sheets.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            width_shift: 0,
            pixels: Vec::new(),
        }
    }

    /// Whether the surface has no pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty() || self.width <= 0 || self.height <= 0
    }

    /// Reads a pixel by flat offset (the `gfxData` cursor of the scaled paths), returning `0`
    /// when out of range.
    #[must_use]
    pub fn pixel_at_offset(&self, offset: i32) -> u8 {
        if offset < 0 {
            return 0;
        }
        self.pixels.get(offset as usize).copied().unwrap_or(0)
    }

    /// Reads a pixel with bounds checks, returning `0` (transparent) out of range.
    #[must_use]
    pub fn pixel(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 0;
        }
        let index = y as usize * self.width as usize + x as usize;
        self.pixels.get(index).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_shift_matches_surface_width() {
        assert_eq!(Surface::from_indexed(64, 64, vec![0; 4096]).width_shift, 6);
        assert_eq!(Surface::from_indexed(1, 1, vec![0]).width_shift, 0);
        assert_eq!(
            Surface::from_indexed(320, 32, vec![0; 10240]).width_shift,
            8
        );
    }

    #[test]
    fn pixel_lookups_are_bounds_checked() {
        let surface = Surface::from_indexed(2, 2, vec![1, 2, 3, 4]);
        assert_eq!(surface.pixel(0, 0), 1);
        assert_eq!(surface.pixel(1, 1), 4);
        assert_eq!(surface.pixel(2, 0), 0);
        assert_eq!(surface.pixel(-1, 0), 0);
        assert_eq!(surface.pixel(0, 2), 0);
    }

    #[test]
    fn short_pixel_buffers_are_zero_padded() {
        let surface = Surface::from_indexed(2, 2, vec![9]);
        assert_eq!(surface.pixel(0, 0), 9);
        assert_eq!(surface.pixel(1, 1), 0);
    }
}
