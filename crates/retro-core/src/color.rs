//! RGB888 to RGB565 and RGB5551 pixel packing used by the software renderer and SDL3 presenter.

/// Packs an RGB888 colour into RGB565 with the red channel in bits 15..11.
#[inline]
#[must_use]
pub const fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) & 0xF8) << 8) | (((g as u16) & 0xFC) << 3) | ((b as u16) >> 3)
}

/// Packs an RGB888 colour into RSDKv4's RGB5551 layout: red bits 11..15, green bits 6..10,
/// blue bits 1..5 and the opacity flag in bit 0, matching `RGB888_TO_RGB5551` in `RSDKv4/Palette.hpp`.
#[inline]
#[must_use]
pub const fn rgb888_to_rgb5551(r: u8, g: u8, b: u8, opaque: bool) -> u16 {
    let alpha = opaque as u16;
    (2 * ((b as u16) >> 3)) | (((g as u16) >> 3) << 6) | (((r as u16) >> 3) << 11) | alpha
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
    fn rgb5551_matches_upstream_known_answers() {
        assert_eq!(rgb888_to_rgb5551(255, 0, 0, true), 0xF801);
        assert_eq!(rgb888_to_rgb5551(255, 0, 0, false), 0xF800);
        assert_eq!(rgb888_to_rgb5551(0, 255, 0, true), 0x07C1);
        assert_eq!(rgb888_to_rgb5551(0, 255, 0, false), 0x07C0);
        assert_eq!(rgb888_to_rgb5551(0, 0, 255, true), 0x003F);
        assert_eq!(rgb888_to_rgb5551(0, 0, 255, false), 0x003E);
        assert_eq!(rgb888_to_rgb5551(255, 255, 255, true), 0xFFFF);
        assert_eq!(rgb888_to_rgb5551(255, 255, 255, false), 0xFFFE);
        assert_eq!(rgb888_to_rgb5551(0, 0, 0, true), 0x0001);
        assert_eq!(rgb888_to_rgb5551(0, 0, 0, false), 0x0000);
    }

    #[test]
    fn rgb5551_matches_upstream_macro_arithmetic() {
        for r in (0..=255u16).step_by(17) {
            for g in (0..=255u16).step_by(17) {
                for b in (0..=255u16).step_by(17) {
                    let upstream = (2 * (b >> 3)) | ((g >> 3) << 6) | ((r >> 3) << 11);
                    let expected = upstream | 1;
                    assert_eq!(rgb888_to_rgb5551(r as u8, g as u8, b as u8, true), expected);
                    assert_eq!(
                        rgb888_to_rgb5551(r as u8, g as u8, b as u8, false),
                        upstream
                    );
                }
            }
        }
    }

    #[test]
    fn rgb5551_low_bits_are_dropped() {
        assert_eq!(rgb888_to_rgb5551(7, 7, 7, true), 0x0001);
        assert_eq!(rgb888_to_rgb5551(8, 8, 8, true), 0x0843);
    }

    #[test]
    fn packers_are_const() {
        const RED565: u16 = rgb888_to_rgb565(255, 0, 0);
        const RED5551: u16 = rgb888_to_rgb5551(255, 0, 0, true);
        assert_eq!(RED565, 0xF800);
        assert_eq!(RED5551, 0xF801);
    }
}
