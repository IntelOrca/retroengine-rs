//! RGB888 to RGB565 and RGB5551 pixel packing used by the software renderer and SDL3 presenter.

/// Packs an RGB888 colour into RGB565 with the red channel in bits 15..11.
#[inline]
#[must_use]
pub const fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) & 0xF8) << 8) | (((g as u16) & 0xFC) << 3) | ((b as u16) >> 3)
}

/// Packs an RGB888 colour into RGB5551, with the opacity flag in bit 15.
#[inline]
#[must_use]
pub const fn rgb888_to_rgb5551(r: u8, g: u8, b: u8, opaque: bool) -> u16 {
    let alpha = if opaque { 0x8000 } else { 0x0000 };
    alpha | (((r as u16) & 0xF8) << 7) | (((g as u16) & 0xF8) << 2) | ((b as u16) >> 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_primaries() {
        assert_eq!(rgb888_to_rgb565(0, 0, 0), 0x0000);
        assert_eq!(rgb888_to_rgb565(255, 0, 0), 0xF800);
        assert_eq!(rgb888_to_rgb565(0, 255, 0), 0x07E0);
        assert_eq!(rgb888_to_rgb565(0, 0, 255), 0x001F);
        assert_eq!(rgb888_to_rgb565(255, 255, 255), 0xFFFF);
    }

    #[test]
    fn rgb565_low_bits_are_dropped() {
        assert_eq!(rgb888_to_rgb565(7, 3, 7), 0x0000);
        assert_eq!(rgb888_to_rgb565(8, 4, 8), 0x0821);
        assert_eq!(rgb888_to_rgb565(0xF8, 0xFC, 0xFF), 0xFFFF);
        assert_eq!(rgb888_to_rgb565(0xFF, 0xFF, 0xFF), 0xFFFF);
    }

    #[test]
    fn rgb5551_primaries() {
        assert_eq!(rgb888_to_rgb5551(0, 0, 0, false), 0x0000);
        assert_eq!(rgb888_to_rgb5551(0, 0, 0, true), 0x8000);
        assert_eq!(rgb888_to_rgb5551(255, 0, 0, true), 0xFC00);
        assert_eq!(rgb888_to_rgb5551(0, 255, 0, true), 0x83E0);
        assert_eq!(rgb888_to_rgb5551(0, 0, 255, true), 0x801F);
        assert_eq!(rgb888_to_rgb5551(255, 255, 255, true), 0xFFFF);
        assert_eq!(rgb888_to_rgb5551(255, 255, 255, false), 0x7FFF);
    }

    #[test]
    fn packers_are_const() {
        const RED565: u16 = rgb888_to_rgb565(255, 0, 0);
        const RED5551: u16 = rgb888_to_rgb5551(255, 0, 0, true);
        assert_eq!(RED565, 0xF800);
        assert_eq!(RED5551, 0xFC00);
    }
}
