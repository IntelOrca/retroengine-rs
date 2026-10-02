//! Parser for RSDKv4 palette files (`Data/Palettes/*.act`) and RGB565 conversion helpers.
//!
//! A `.act` file is a raw, unlabelled RGB888 colour table, conventionally 256 colours
//! ([`PALETTE_COLORS`] / [`PALETTE_BYTES`]) but not required to be: upstream (`LoadPalette` in
//! `RSDKv4/Palette.cpp`) seeks to `3 * startIndex` and reads `endIndex - startIndex` colours
//! without ever validating the file size, and RSDK ships partial files as well. This parser
//! therefore accepts any whole number of colours up to 256 and the [`Palette`] value is simply
//! the file's colours in order; the file itself has no global/stage split.
//!
//! The 96/32 split is a property of the *engine's* palette slots, not of the `.act` data:
//! `GameConfig.bin` fills engine indices `0x00..0x60` (96 colours,
//! [`GLOBAL_PALETTE_COLORS`]) and `StageConfig.bin` fills `0x60..0x80` (32 colours,
//! [`STAGE_PALETTE_COLORS`]). [`Palette::global`] and [`Palette::stage`] are only convenience
//! views of those two engine ranges when a palette is applied to those slots (e.g. a mod or
//! tool loading a full `.act` over the combined palette); they do not claim the file is split
//! that way. [`Palette::to_rgb565`] packs the whole table with the software renderer's RGB565
//! layout from [`retro_core::color::rgb888_to_rgb565`].

use serde::Serialize;

use crate::error::FormatError;
use retro_core::color::rgb888_to_rgb565;
use retro_io::DataSource;

/// Number of colours in a full `.act` palette.
pub const PALETTE_COLORS: usize = 0x100;
/// Byte size of a full `.act` palette.
pub const PALETTE_BYTES: usize = PALETTE_COLORS * 3;
/// Number of colours in the engine's global palette slot (`0x00..0x60`), filled from
/// `GameConfig.bin`; not a property of `.act` files.
pub const GLOBAL_PALETTE_COLORS: usize = 0x60;
/// Number of colours in the engine's stage palette slot (`0x60..0x80`), filled from
/// `StageConfig.bin`; not a property of `.act` files.
pub const STAGE_PALETTE_COLORS: usize = 0x20;
/// Engine palette index of the first stage colour.
pub const STAGE_PALETTE_INDEX: usize = GLOBAL_PALETTE_COLORS;

/// Raw RGB888 palette loaded from an `.act` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    /// Colours in file order.
    pub colors: Vec<[u8; 3]>,
}

impl Palette {
    /// Parses an `.act` palette from memory.
    ///
    /// The payload must contain a whole number of RGB triples and no more than
    /// [`PALETTE_BYTES`] bytes; an empty file yields an empty palette.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if !bytes.len().is_multiple_of(3) {
            return Err(FormatError::invalid(format!(
                "palette size {} is not a multiple of 3",
                bytes.len()
            )));
        }
        if bytes.len() > PALETTE_BYTES {
            return Err(FormatError::invalid(format!(
                "palette size {} exceeds {PALETTE_BYTES}",
                bytes.len()
            )));
        }
        let (chunks, _remainder) = bytes.as_chunks::<3>();
        let colors = chunks.to_vec();
        Ok(Self { colors })
    }

    /// Reads an `.act` file at `path` (relative to the source root, e.g.
    /// `Data/Palettes/ElectricFlash.act`) through `src` and parses it.
    pub fn load(path: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(path)?)
    }

    /// Number of colours in the palette.
    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// Whether the palette contains no colours.
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// Returns colour `index`, or `None` when out of range.
    pub fn entry(&self, index: usize) -> Option<[u8; 3]> {
        self.colors.get(index).copied()
    }

    /// Returns the RGB888 slice `start..end`, or `None` when `end` is out of range.
    pub fn slice(&self, start: usize, end: usize) -> Option<&[[u8; 3]]> {
        if start > end {
            return None;
        }
        self.colors.get(start..end)
    }

    /// View of the colours that would occupy the engine's 96-entry global palette range
    /// (`0x00..0x60`) if this table were applied there, truncated to the file length.
    ///
    /// This is a slicing convenience for the combined engine palette layout; a `.act` file is a
    /// raw table with no inherent global/stage division.
    pub fn global(&self) -> &[[u8; 3]] {
        &self.colors[..self.colors.len().min(GLOBAL_PALETTE_COLORS)]
    }

    /// View of the colours that would occupy the engine's 32-entry stage palette range
    /// (`0x60..0x80`) if this table were applied to the combined engine palette, truncated to
    /// the file length.
    ///
    /// This is a slicing convenience for the combined engine palette layout; a `.act` file is a
    /// raw table with no inherent global/stage division.
    pub fn stage(&self) -> &[[u8; 3]] {
        let start = self.colors.len().min(STAGE_PALETTE_INDEX);
        let end = self
            .colors
            .len()
            .min(STAGE_PALETTE_INDEX + STAGE_PALETTE_COLORS);
        &self.colors[start..end]
    }

    /// Packs every colour into the software renderer's RGB565 format.
    pub fn to_rgb565(&self) -> Vec<u16> {
        self.colors
            .iter()
            .map(|color| rgb888_to_rgb565(color[0], color[1], color[2]))
            .collect()
    }

    /// Packs colour `index` into RGB565, or `None` when out of range.
    pub fn rgb565(&self, index: usize) -> Option<u16> {
        self.entry(index)
            .map(|color| rgb888_to_rgb565(color[0], color[1], color[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    #[test]
    fn parses_full_palette() {
        let mut bytes = vec![0u8; PALETTE_BYTES];
        bytes[0..3].copy_from_slice(&[255, 0, 255]);
        bytes[3..6].copy_from_slice(&[0, 255, 0]);
        let palette = Palette::from_bytes(&bytes).unwrap();
        assert_eq!(palette.len(), PALETTE_COLORS);
        assert_eq!(palette.entry(0), Some([255, 0, 255]));
        assert_eq!(palette.entry(1), Some([0, 255, 0]));
        assert_eq!(palette.entry(PALETTE_COLORS), None);
    }

    #[test]
    fn converts_rgb565() {
        let mut bytes = vec![0u8; PALETTE_BYTES];
        bytes[0..3].copy_from_slice(&[255, 0, 255]);
        bytes[3..6].copy_from_slice(&[0, 255, 0]);
        bytes[6..9].copy_from_slice(&[0, 0, 255]);
        let palette = Palette::from_bytes(&bytes).unwrap();
        assert_eq!(palette.rgb565(0), Some(0xF81F));
        assert_eq!(palette.rgb565(1), Some(0x07E0));
        assert_eq!(palette.rgb565(2), Some(0x001F));
        assert_eq!(palette.rgb565(3), Some(0x0000));
        assert_eq!(palette.rgb565(PALETTE_COLORS), None);
        assert_eq!(palette.to_rgb565()[..3], [0xF81F, 0x07E0, 0x001F]);
    }

    #[test]
    fn accepts_partial_and_empty_palettes() {
        let palette = Palette::from_bytes(&vec![1u8; 384]).unwrap();
        assert_eq!(palette.len(), 128);
        assert_eq!(palette.stage().len(), 128 - STAGE_PALETTE_INDEX);
        assert_eq!(palette.global().len(), GLOBAL_PALETTE_COLORS);

        let palette = Palette::from_bytes(&[]).unwrap();
        assert!(palette.is_empty());
        assert!(palette.global().is_empty());
        assert!(palette.stage().is_empty());
    }

    #[test]
    fn slices_global_and_stage_palettes() {
        let mut bytes = vec![0u8; PALETTE_BYTES];
        let (chunks, _remainder) = bytes.as_chunks_mut::<3>();
        for (index, chunk) in chunks.iter_mut().enumerate() {
            chunk[0] = index as u8;
        }
        let palette = Palette::from_bytes(&bytes).unwrap();
        assert_eq!(palette.global().len(), GLOBAL_PALETTE_COLORS);
        assert_eq!(palette.global()[0x5F][0], 0x5F);
        assert_eq!(palette.stage().len(), STAGE_PALETTE_COLORS);
        assert_eq!(palette.stage()[0][0], 0x60);
        assert_eq!(palette.stage()[0x1F][0], 0x7F);
        assert_eq!(
            palette.slice(0x60, 0x80).unwrap().len(),
            STAGE_PALETTE_COLORS
        );
        assert_eq!(palette.slice(0x80, 0x60), None);
        assert_eq!(palette.slice(0, PALETTE_COLORS + 1), None);
    }

    #[test]
    fn rejects_invalid_sizes() {
        assert!(matches!(
            Palette::from_bytes(&[0, 0]),
            Err(FormatError::Invalid(_))
        ));
        assert!(matches!(
            Palette::from_bytes(&vec![0u8; PALETTE_BYTES + 3]),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn loads_from_source() {
        let mut source = MemorySource::new();
        source.insert("Data/Palettes/ElectricFlash.act", vec![0u8; PALETTE_BYTES]);
        let palette = Palette::load("Data/Palettes/ElectricFlash.act", &source).unwrap();
        assert_eq!(palette.len(), PALETTE_COLORS);
        assert!(matches!(
            Palette::load("Data/Palettes/Missing.act", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0xFEED_BEEFu32;
        for length in 0..512usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = Palette::from_bytes(&bytes);
        }
    }
}
