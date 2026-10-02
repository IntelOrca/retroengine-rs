//! Parsers for the RSDKv4 tile sheets: `128x128Tiles.bin` and `16x16Tiles.gif`.
//!
//! `128x128Tiles.bin` holds [`TILE_SHEET_128_ENTRY_COUNT`] three-byte entries. Upstream
//! (`LoadStageChunks` in `RSDKv4/Scene.cpp`; identical in
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`) decodes each entry as:
//!
//! ```text
//! byte0 -= (byte0 >> 6) << 6;              // drop the two high bits
//! visual_plane = byte0 >> 4;   byte0 -= 16 * (byte0 >> 4);
//! direction    = byte0 >> 2;   byte0 -= 4 * (byte0 >> 2);
//! tile_index   = byte1 + (byte0 << 8);     // 0..=0x3FF
//! collision_flags[0] = byte2 >> 4;
//! collision_flags[1] = byte2 & 0xF;
//! ```
//!
//! `16x16Tiles.gif` is decoded by [`retro_image::decode_gif`]. Upstream accepts any width of
//! [`TILE_SIZE`] and a height of at most [`TILE_SHEET_16_MAX_TILES`] tiles
//! (`LoadStageGIFFile`), so this parser additionally requires the height to be a whole number
//! of tiles and not exceed the 0x400-tile engine limit.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// Side length of a 16x16 tile in pixels.
pub const TILE_SIZE: usize = 16;
/// Number of bytes in one decoded 16x16 tile.
pub const TILE_PIXEL_COUNT: usize = TILE_SIZE * TILE_SIZE;
/// File name of the 128x128 chunk tile sheet inside a stage folder.
pub const TILE_SHEET_128_FILE: &str = "128x128Tiles.bin";
/// File name of the 16x16 pixel tile sheet inside a stage folder.
pub const TILE_SHEET_16_FILE: &str = "16x16Tiles.gif";
/// Number of 128x128 chunk entries upstream reads (`CHUNKTILE_COUNT = 0x200 * 8 * 8`).
pub const TILE_SHEET_128_ENTRY_COUNT: usize = 0x200 * 8 * 8;
/// Byte size of a complete `128x128Tiles.bin`.
pub const TILE_SHEET_128_BYTES: usize = TILE_SHEET_128_ENTRY_COUNT * 3;
/// Maximum number of 16x16 tiles in `16x16Tiles.gif` (`TILE_COUNT`).
pub const TILE_SHEET_16_MAX_TILES: usize = 0x400;
/// Maximum GIF height accepted by the engine, in pixels.
pub const TILE_SHEET_16_MAX_HEIGHT: u16 = (TILE_SHEET_16_MAX_TILES * TILE_SIZE) as u16;

/// One three-byte `128x128Tiles.bin` chunk entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tile128 {
    /// Chunk direction (0..=3).
    pub direction: u8,
    /// Visual plane (0..=3).
    pub visual_plane: u8,
    /// 16x16 tile index into `16x16Tiles.gif` (0..=0x3FF).
    pub tile_index: u16,
    /// Collision plane A flags (high nibble of the third byte).
    pub collision_flag_a: u8,
    /// Collision plane B flags (low nibble of the third byte).
    pub collision_flag_b: u8,
}

impl Tile128 {
    /// Decodes one raw three-byte entry exactly as `LoadStageChunks` does.
    pub fn from_entry(entry: [u8; 3]) -> Self {
        let mut packed = entry[0] - ((entry[0] >> 6) << 6);
        let visual_plane = packed >> 4;
        packed -= 16 * (packed >> 4);
        let direction = packed >> 2;
        packed -= 4 * (packed >> 2);
        Self {
            direction,
            visual_plane,
            tile_index: u16::from(entry[1]) + (u16::from(packed) << 8),
            collision_flag_a: entry[2] >> 4,
            collision_flag_b: entry[2] & 0xF,
        }
    }

    /// Software-renderer index into the 16x16 tileset (`tiles128x128.gfxDataPos`).
    pub fn gfx_data_pos(&self) -> i32 {
        i32::from(self.tile_index) << 8
    }
}

/// Parsed `128x128Tiles.bin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TileSheet128 {
    /// One entry per chunk, in file order.
    pub entries: Vec<Tile128>,
}

impl TileSheet128 {
    /// Parses a `128x128Tiles.bin` from memory.
    ///
    /// The parser requires the full [`TILE_SHEET_128_BYTES`] payload; bytes after the last entry
    /// are ignored, matching the streaming loader.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);
        let mut entries = Vec::with_capacity(TILE_SHEET_128_ENTRY_COUNT);
        for _ in 0..TILE_SHEET_128_ENTRY_COUNT {
            entries.push(Tile128::from_entry(reader.read_array::<3>()?));
        }
        Ok(Self { entries })
    }

    /// Reads `<stage_dir>/128x128Tiles.bin` through `src` and parses it.
    pub fn load(stage_dir: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(&stage_file(stage_dir, TILE_SHEET_128_FILE))?)
    }
}

/// Decoded `16x16Tiles.gif` in indexed colour.
///
/// `pixels` keeps the raw palette indices produced by [`retro_image::decode_gif`]. Upstream's
/// software renderer later replaces every index equal to the first pixel with index 0 when it
/// copies the sheet into its tileset buffer; that normalisation is engine behaviour and is not
/// applied here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TileSheet16 {
    /// GIF width in pixels.
    pub width: u16,
    /// GIF height in pixels.
    pub height: u16,
    /// `width * height` palette indices in row-major order.
    pub pixels: Vec<u8>,
    /// GIF global colour table in RGB888 order.
    pub palette: Vec<[u8; 3]>,
}

impl TileSheet16 {
    /// Decodes a `16x16Tiles.gif` image from memory.
    pub fn from_gif(bytes: &[u8]) -> Result<Self, FormatError> {
        let image = retro_image::decode_gif(bytes).map_err(map_image_error)?;
        if image.width != TILE_SIZE as u16 {
            return Err(FormatError::invalid(format!(
                "{TILE_SHEET_16_FILE} width {} is not {TILE_SIZE}",
                image.width
            )));
        }
        if image.height % TILE_SIZE as u16 != 0 {
            return Err(FormatError::invalid(format!(
                "{TILE_SHEET_16_FILE} height {} is not a multiple of {TILE_SIZE}",
                image.height
            )));
        }
        if image.height > TILE_SHEET_16_MAX_HEIGHT {
            return Err(FormatError::invalid(format!(
                "{TILE_SHEET_16_FILE} height {} exceeds {TILE_SHEET_16_MAX_HEIGHT}",
                image.height
            )));
        }
        let expected = usize::from(image.width) * usize::from(image.height);
        if image.pixels.len() != expected {
            return Err(FormatError::invalid(format!(
                "{TILE_SHEET_16_FILE} decoded {} pixels, expected {expected}",
                image.pixels.len()
            )));
        }
        Ok(Self {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
            palette: image.palette,
        })
    }

    /// Reads `<stage_dir>/16x16Tiles.gif` through `src` and decodes it.
    pub fn load(stage_dir: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_gif(&src.read(&stage_file(stage_dir, TILE_SHEET_16_FILE))?)
    }

    /// Number of whole 16x16 tiles in the sheet.
    pub fn tile_count(&self) -> usize {
        usize::from(self.height) / TILE_SIZE
    }

    /// Raw palette indices of tile `index` (row-major order, [`TILE_PIXEL_COUNT`] bytes), or
    /// `None` when the index is out of range.
    pub fn tile(&self, index: usize) -> Option<&[u8]> {
        let start = index.checked_mul(TILE_PIXEL_COUNT)?;
        let end = start.checked_add(TILE_PIXEL_COUNT)?;
        self.pixels.get(start..end)
    }
}

fn stage_file(stage_dir: &str, file: &str) -> String {
    let directory = stage_dir.trim_end_matches('/');
    if directory.is_empty() {
        file.to_owned()
    } else {
        format!("{directory}/{file}")
    }
}

fn map_image_error(error: retro_image::ImageError) -> FormatError {
    match error {
        retro_image::ImageError::Truncated => FormatError::Truncated,
        other @ (retro_image::ImageError::Invalid(_) | retro_image::ImageError::Unsupported(_)) => {
            FormatError::invalid(other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    #[test]
    fn decodes_tile128_entries() {
        // byte0 = 0xE6 -> 0x26 after masking; visual_plane = 2; direction = 1; index high = 2
        let tile = Tile128::from_entry([0xE6, 0x34, 0xA5]);
        assert_eq!(tile.visual_plane, 2);
        assert_eq!(tile.direction, 1);
        assert_eq!(tile.tile_index, 0x234);
        assert_eq!(tile.collision_flag_a, 0xA);
        assert_eq!(tile.collision_flag_b, 0x5);
        assert_eq!(tile.gfx_data_pos(), 0x23400);
    }

    #[test]
    fn decodes_extreme_tile128_entries() {
        let tile = Tile128::from_entry([0xFF, 0xFF, 0xFF]);
        assert_eq!(tile.visual_plane, 3);
        assert_eq!(tile.direction, 3);
        assert_eq!(tile.tile_index, 0x3FF);
        let tile = Tile128::from_entry([0x00, 0x00, 0x00]);
        assert_eq!(tile.visual_plane, 0);
        assert_eq!(tile.direction, 0);
        assert_eq!(tile.tile_index, 0);
    }

    #[test]
    fn parses_full_tile_sheet_128() {
        let mut bytes = vec![0u8; TILE_SHEET_128_BYTES];
        bytes[0] = 0xE6;
        bytes[1] = 0x34;
        bytes[2] = 0xA5;
        let sheet = TileSheet128::from_bytes(&bytes).unwrap();
        assert_eq!(sheet.entries.len(), TILE_SHEET_128_ENTRY_COUNT);
        assert_eq!(sheet.entries[0].tile_index, 0x234);
        assert_eq!(sheet.entries[1], Tile128::from_entry([0, 0, 0]));
    }

    #[test]
    fn truncated_tile_sheet_128_errors() {
        let bytes = vec![0u8; TILE_SHEET_128_BYTES - 1];
        assert!(matches!(
            TileSheet128::from_bytes(&bytes),
            Err(FormatError::Truncated)
        ));
        assert!(matches!(
            TileSheet128::from_bytes(&[]),
            Err(FormatError::Truncated)
        ));
    }

    #[test]
    fn tile_sheet_128_ignores_trailing_bytes() {
        let mut bytes = vec![0u8; TILE_SHEET_128_BYTES];
        bytes.push(0xFF);
        assert!(TileSheet128::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn loads_tile_sheet_128_from_source() {
        let mut source = MemorySource::new();
        source.insert(
            "Data/Stages/Zone01/128x128Tiles.bin",
            vec![0u8; TILE_SHEET_128_BYTES],
        );
        let sheet = TileSheet128::load("Data/Stages/Zone01", &source).unwrap();
        assert_eq!(sheet.entries.len(), TILE_SHEET_128_ENTRY_COUNT);
        assert!(matches!(
            TileSheet128::load("Data/Stages/Missing", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn tile_16_accessors_are_bounds_checked() {
        let sheet = TileSheet16 {
            width: 16,
            height: 32,
            pixels: vec![0; 16 * 32],
            palette: Vec::new(),
        };
        assert_eq!(sheet.tile_count(), 2);
        assert_eq!(sheet.tile(0).map(|tile| tile.len()), Some(TILE_PIXEL_COUNT));
        assert!(sheet.tile(2).is_none());
        assert!(sheet.tile(usize::MAX).is_none());
    }

    #[test]
    fn rejects_wrong_gif_input() {
        assert!(matches!(
            TileSheet16::from_gif(b"not a gif"),
            Err(FormatError::Invalid(_))
        ));
        assert!(matches!(
            TileSheet16::from_gif(&[]),
            Err(FormatError::Truncated) | Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0xABCD_EF01u32;
        for length in 0..256usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = TileSheet128::from_bytes(&bytes);
            let _ = TileSheet16::from_gif(&bytes);
        }
    }
}
