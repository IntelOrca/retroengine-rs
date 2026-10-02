//! The u16 RGB565 software framebuffer.
//!
//! The framebuffer mirrors upstream `Engine.frameBuffer` (`Drawing.cpp`): a row-major buffer of
//! RGB565 pixels whose row stride (`GFX_LINESIZE`) is the logical width rounded up to a multiple
//! of 8 (`SetScreenSize` stores `(displayWidth + 9) & -0x8`). Only the first `width` pixels of
//! each row are presented or dumped; the padding is render scratch, exactly like upstream.

/// Converts an RGB565 pixel into RGB888 by bit replication (`r5 << 3 | r5 >> 2`).
#[must_use]
pub const fn rgb565_to_rgb888(pixel: u16) -> [u8; 3] {
    let r = ((pixel >> 11) & 0x1F) as u8;
    let g = ((pixel >> 5) & 0x3F) as u8;
    let b = (pixel & 0x1F) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// Packs RGB888 into RGB565 exactly like upstream `PACK_RGB888`.
#[must_use]
pub const fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    retro_core::color::rgb888_to_rgb565(r, g, b)
}

/// A row-major RGB565 framebuffer with a padded pitch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pitch: usize,
    pixels: Vec<u16>,
}

impl Framebuffer {
    /// Creates a framebuffer with the upstream pitch alignment `(width + 9) & -0x8`.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let pitch = (width + 9) & !0x7;
        Self::with_pitch(width, height, pitch)
    }

    /// Creates a framebuffer with an explicit pitch; the pitch must be at least the width.
    #[must_use]
    pub fn with_pitch(width: usize, height: usize, pitch: usize) -> Self {
        let pitch = pitch.max(width);
        Self {
            width,
            height,
            pitch,
            pixels: vec![0; pitch * height],
        }
    }

    /// Visible width in pixels (`SCREEN_XSIZE`).
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels (`SCREEN_YSIZE`).
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Row stride in pixels (`GFX_LINESIZE`).
    #[must_use]
    pub const fn pitch(&self) -> usize {
        self.pitch
    }

    /// Total number of backing pixels (`pitch * height`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.pixels.len()
    }

    /// Whether the framebuffer has no pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// All backing pixels as a slice (`pitch * height`).
    #[must_use]
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }

    /// All backing pixels as a mutable slice.
    pub fn pixels_mut(&mut self) -> &mut [u16] {
        &mut self.pixels
    }

    /// Fills the whole backing buffer with `color`.
    pub fn clear(&mut self, color: u16) {
        self.pixels.fill(color);
    }

    /// Reads one backing pixel, returning `0` when out of range.
    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> u16 {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return 0;
        };
        if x >= self.pitch || y >= self.height {
            return 0;
        }
        self.pixels.get(y * self.pitch + x).copied().unwrap_or(0)
    }

    /// Writes one backing pixel, ignoring out-of-range coordinates.
    pub fn set(&mut self, x: i32, y: i32, color: u16) {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        if x >= self.pitch || y >= self.height {
            return;
        }
        if let Some(pixel) = self.pixels.get_mut(y * self.pitch + x) {
            *pixel = color;
        }
    }

    /// Returns backing row `y`, or an empty slice when out of range.
    #[must_use]
    pub fn row(&self, y: usize) -> &[u16] {
        let start = y * self.pitch;
        self.pixels.get(start..start + self.pitch).unwrap_or(&[])
    }

    /// Returns visible pixels row `y` (without pitch padding).
    #[must_use]
    pub fn visible_row(&self, y: usize) -> &[u16] {
        self.row(y).get(..self.width).unwrap_or(&[])
    }

    /// Feeds the full backing buffer into a BLAKE3 hasher as little-endian u16 bytes.
    pub fn hash_into(&self, hasher: &mut blake3::Hasher) {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 2);
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_le_bytes());
        }
        hasher.update(&bytes);
    }

    /// BLAKE3 hash of the full backing buffer (`pitch * height` little-endian u16 pixels).
    #[must_use]
    pub fn hash(&self) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        self.hash_into(&mut hasher);
        hasher.finalize()
    }

    /// Hex BLAKE3 hash of the full backing buffer.
    #[must_use]
    pub fn hash_hex(&self) -> String {
        self.hash().to_hex().to_string()
    }

    /// Copies the visible `width * height` region (without pitch padding) into `out`, reusing
    /// its allocation. Used to feed the platform present API.
    pub fn copy_visible_into(&self, out: &mut Vec<u16>) {
        out.clear();
        out.reserve(self.width * self.height);
        for y in 0..self.height {
            out.extend_from_slice(self.visible_row(y));
        }
    }

    /// Converts the visible `width * height` region to RGB888 (3 bytes per pixel).
    #[must_use]
    pub fn to_rgb888(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 3);
        for y in 0..self.height {
            for pixel in self.visible_row(y) {
                out.extend_from_slice(&rgb565_to_rgb888(*pixel));
            }
        }
        out
    }

    /// Encodes the visible region as a PNG.
    pub fn to_png_bytes(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&self.to_rgb888())?;
        }
        Ok(out)
    }

    /// Encodes the visible region as a binary PPM (`P6`) for tooling without the PNG crate.
    #[must_use]
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        out.extend_from_slice(&self.to_rgb888());
        out
    }

    /// Ratio of non-black visible pixels, used by the dump tooling sanity checks.
    #[must_use]
    pub fn non_black_ratio(&self) -> f64 {
        let total = self.width * self.height;
        if total == 0 {
            return 0.0;
        }
        let mut non_black = 0usize;
        for y in 0..self.height {
            non_black += self
                .visible_row(y)
                .iter()
                .filter(|pixel| **pixel != 0)
                .count();
        }
        non_black as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_matches_upstream_alignment() {
        assert_eq!(Framebuffer::new(320, 224).pitch(), 328);
        assert_eq!(Framebuffer::new(424, 240).pitch(), 432);
        assert_eq!(Framebuffer::new(32, 8).pitch(), 40);
    }

    #[test]
    fn clear_and_accessors() {
        let mut framebuffer = Framebuffer::new(4, 2);
        framebuffer.clear(0x1234);
        assert_eq!(framebuffer.get(3, 1), 0x1234);
        assert_eq!(
            framebuffer.get(4, 1),
            0x1234,
            "pitch padding keeps the clear colour"
        );
        assert_eq!(framebuffer.get(8, 1), 0);
        assert_eq!(framebuffer.get(0, 2), 0);
        framebuffer.set(-1, 0, 0xFFFF);
        framebuffer.set(0, 0, 0xABCD);
        assert_eq!(framebuffer.get(0, 0), 0xABCD);
    }

    #[test]
    fn rgb565_round_trip_matches_bit_replication() {
        assert_eq!(rgb565_to_rgb888(0xF800), [255, 0, 0]);
        assert_eq!(rgb565_to_rgb888(0x07E0), [0, 255, 0]);
        assert_eq!(rgb565_to_rgb888(0x001F), [0, 0, 255]);
        assert_eq!(rgb565_to_rgb888(0x0000), [0, 0, 0]);
        assert_eq!(rgb565_to_rgb888(0xFFFF), [255, 255, 255]);
        // 0x8C51 is r=17, g=34, b=17; bit replication expands to 140/138/140.
        assert_eq!(rgb565_to_rgb888(0x8C51), [140, 138, 140]);
    }

    #[test]
    fn hash_covers_the_padding() {
        let first = Framebuffer::new(4, 2);
        let mut second = Framebuffer::new(4, 2);
        assert_eq!(first.hash(), second.hash());
        second.set(7, 1, 1); // padding column
        assert_ne!(first.hash(), second.hash());
    }

    #[test]
    fn ppm_header_is_well_formed() {
        let framebuffer = Framebuffer::new(2, 1);
        let ppm = framebuffer.to_ppm();
        assert!(ppm.starts_with(b"P6\n2 1\n255\n"));
        assert_eq!(ppm.len(), "P6\n2 1\n255\n".len() + 6);
    }

    #[test]
    fn png_round_trips_visible_pixels() {
        let mut framebuffer = Framebuffer::new(3, 2);
        framebuffer.set(0, 0, 0xF800);
        framebuffer.set(1, 0, 0x07E0);
        framebuffer.set(2, 0, 0x001F);
        framebuffer.set(0, 1, 0xFFFF);
        let png = framebuffer.to_png_bytes().unwrap();
        let decoder = png::Decoder::new(png.as_slice());
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(
            &buffer[..info.buffer_size()],
            &[
                255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            ]
        );
    }

    #[test]
    fn non_black_ratio_counts_visible_pixels_only() {
        let mut framebuffer = Framebuffer::new(2, 2);
        assert_eq!(framebuffer.non_black_ratio(), 0.0);
        framebuffer.set(0, 0, 1);
        framebuffer.set(3, 0, 1); // padding is ignored
        assert_eq!(framebuffer.non_black_ratio(), 0.25);
    }
}
