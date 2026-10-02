//! Parser for the RSDKv4 `Data/Stages/<folder>/Backgrounds.bin` file.
//!
//! On-disk order (`LoadStageBackground` in `RSDKv4/Scene.cpp`; identical in
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`):
//!
//! ```text
//! u8       background layer count
//! u8       horizontal parallax entry count
//! repeat:  u16 parallax factor (little-endian), u8 scroll speed (stored << 10), u8 deform
//! u8       vertical parallax entry count
//! repeat:  u16 parallax factor (little-endian), u8 scroll speed (stored << 10), u8 deform
//! repeat layer count times (background layers 1..=count):
//!     u8   layout width in 128x128 chunks
//!     u8   unused
//!     u8   layout height in 128x128 chunks
//!     u8   unused
//!     u8   layer type
//!     u16  parallax factor (little-endian)
//!     u8   scroll speed (stored << 10)
//!     RLE  line scroll data, terminated by 0xFF 0xFF
//!     u16[] layout chunk indices, row-major (width * height entries)
//! ```
//!
//! The RLE stream is a byte stream with one escape: `0xFF index count` emits `index` exactly
//! `count - 1` times (a `count` of 0 emits nothing), and `0xFF 0xFF` ends the stream. Upstream
//! writes into a fixed `0x8000`-byte buffer without bounds checks; this parser rejects streams
//! that would exceed that capacity instead of overflowing.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// File name of the background file inside a stage folder.
pub const BACKGROUNDS_FILE: &str = "Backgrounds.bin";
/// Size of the engine's line scroll buffer (`TILELAYER_CHUNK_H * CHUNK_SIZE`).
pub const LINE_SCROLL_CAPACITY: usize = 0x100 * 0x80;

/// One entry of the horizontal or vertical parallax table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ParallaxEntry {
    /// Parallax factor as stored (`u16` little-endian in the file).
    pub parallax_factor: u16,
    /// Scroll speed, already shifted left by 10 like the engine does.
    pub scroll_speed: i32,
    /// Deformation id used by the layer deformation tables.
    pub deform: u8,
}

/// One background layer (stored at engine layer indices 1..=count).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackgroundLayer {
    /// Layout width in 128x128 chunks.
    pub width: u8,
    /// Layout height in 128x128 chunks.
    pub height: u8,
    /// Layer scroll type (`LAYER_NOSCROLL`, `LAYER_HSCROLL`, ...).
    pub layer_type: u8,
    /// Parallax factor as stored (`u16` little-endian in the file).
    pub parallax_factor: u16,
    /// Scroll speed, already shifted left by 10 like the engine does.
    pub scroll_speed: i32,
    /// Decompressed per-scanline scroll data.
    pub line_scroll: Vec<u8>,
    /// Layout chunk indices, row-major with stride [`BackgroundLayer::width`].
    pub layout: Vec<u16>,
}

/// Parsed `Backgrounds.bin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backgrounds {
    /// Number of background layers that follow the parallax tables.
    pub layer_count: u8,
    /// Horizontal parallax entries.
    pub horizontal: Vec<ParallaxEntry>,
    /// Vertical parallax entries.
    pub vertical: Vec<ParallaxEntry>,
    /// Background layers in file order.
    pub layers: Vec<BackgroundLayer>,
}

impl Backgrounds {
    /// Parses a `Backgrounds.bin` from memory, rejecting trailing bytes.
    ///
    /// The engine stops reading after the last layer; [`Backgrounds::from_bytes_allow_trailing`]
    /// is available for files with extra data.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, true)
    }

    /// Parses a `Backgrounds.bin` from memory, ignoring bytes after the last layer.
    pub fn from_bytes_allow_trailing(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, false)
    }

    fn parse(bytes: &[u8], strict: bool) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);

        let layer_count = reader.read_u8()?;
        let horizontal = read_parallax(&mut reader)?;
        let vertical = read_parallax(&mut reader)?;

        let mut layers = Vec::with_capacity(usize::from(layer_count));
        for _ in 0..layer_count {
            let width = reader.read_u8()?;
            let _unused_width = reader.read_u8()?;
            let height = reader.read_u8()?;
            let _unused_height = reader.read_u8()?;
            let layer_type = reader.read_u8()?;
            let parallax_factor = reader.read_u16_le()?;
            let scroll_speed = i32::from(reader.read_u8()?) << 10;
            let line_scroll = read_line_scroll(&mut reader)?;

            let tile_count = usize::from(width) * usize::from(height);
            let mut layout = Vec::with_capacity(tile_count);
            for _ in 0..tile_count {
                layout.push(reader.read_u16_le()?);
            }

            layers.push(BackgroundLayer {
                width,
                height,
                layer_type,
                parallax_factor,
                scroll_speed,
                line_scroll,
                layout,
            });
        }

        if strict && !reader.is_empty() {
            return Err(FormatError::invalid(format!(
                "{} trailing bytes after background layers",
                reader.remaining()
            )));
        }

        Ok(Self {
            layer_count,
            horizontal,
            vertical,
            layers,
        })
    }

    /// Reads `<stage_dir>/Backgrounds.bin` through `src` and parses it.
    pub fn load(stage_dir: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        let directory = stage_dir.trim_end_matches('/');
        let path = if directory.is_empty() {
            BACKGROUNDS_FILE.to_owned()
        } else {
            format!("{directory}/{BACKGROUNDS_FILE}")
        };
        Self::from_bytes(&src.read(&path)?)
    }
}

fn read_parallax(reader: &mut Reader<'_>) -> Result<Vec<ParallaxEntry>, FormatError> {
    let count = reader.read_u8()? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push(ParallaxEntry {
            parallax_factor: reader.read_u16_le()?,
            scroll_speed: i32::from(reader.read_u8()?) << 10,
            deform: reader.read_u8()?,
        });
    }
    Ok(entries)
}

fn read_line_scroll(reader: &mut Reader<'_>) -> Result<Vec<u8>, FormatError> {
    let mut line_scroll = Vec::new();
    loop {
        let byte = reader.read_u8()?;
        if byte == 0xFF {
            let next = reader.read_u8()?;
            if next == 0xFF {
                break;
            }
            let count = reader.read_u8()?;
            // Upstream computes `count - 1` as a signed int, so 0 emits nothing.
            let repeats = usize::from(count.saturating_sub(1));
            if line_scroll.len() + repeats > LINE_SCROLL_CAPACITY {
                return Err(FormatError::invalid(format!(
                    "line scroll data exceeds {LINE_SCROLL_CAPACITY} bytes"
                )));
            }
            line_scroll.extend(std::iter::repeat_n(next, repeats));
        } else {
            if line_scroll.len() >= LINE_SCROLL_CAPACITY {
                return Err(FormatError::invalid(format!(
                    "line scroll data exceeds {LINE_SCROLL_CAPACITY} bytes"
                )));
            }
            line_scroll.push(byte);
        }
    }
    Ok(line_scroll)
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    type LayerTuple = (u8, u8, u8, u16, u8, Vec<u8>, Vec<u16>);

    struct Fixture {
        layer_count: u8,
        horizontal: Vec<(u16, u8, u8)>,
        vertical: Vec<(u16, u8, u8)>,
        layers: Vec<LayerTuple>,
    }

    impl Fixture {
        fn minimal() -> Self {
            Self {
                layer_count: 0,
                horizontal: Vec::new(),
                vertical: Vec::new(),
                layers: Vec::new(),
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.push(self.layer_count);
            bytes.push(self.horizontal.len() as u8);
            for (factor, speed, deform) in &self.horizontal {
                bytes.extend_from_slice(&factor.to_le_bytes());
                bytes.push(*speed);
                bytes.push(*deform);
            }
            bytes.push(self.vertical.len() as u8);
            for (factor, speed, deform) in &self.vertical {
                bytes.extend_from_slice(&factor.to_le_bytes());
                bytes.push(*speed);
                bytes.push(*deform);
            }
            assert_eq!(self.layers.len(), usize::from(self.layer_count));
            for (width, height, layer_type, factor, speed, line_scroll, layout) in &self.layers {
                bytes.push(*width);
                bytes.push(0);
                bytes.push(*height);
                bytes.push(0);
                bytes.push(*layer_type);
                bytes.extend_from_slice(&factor.to_le_bytes());
                bytes.push(*speed);
                bytes.extend_from_slice(line_scroll);
                bytes.extend_from_slice(&[0xFF, 0xFF]);
                assert_eq!(layout.len(), usize::from(*width) * usize::from(*height));
                for tile in layout {
                    bytes.extend_from_slice(&tile.to_le_bytes());
                }
            }
            bytes
        }
    }

    #[test]
    fn parses_minimal_backgrounds() {
        let backgrounds = Backgrounds::from_bytes(&Fixture::minimal().build()).unwrap();
        assert_eq!(backgrounds.layer_count, 0);
        assert!(backgrounds.horizontal.is_empty());
        assert!(backgrounds.vertical.is_empty());
        assert!(backgrounds.layers.is_empty());
    }

    #[test]
    fn parses_parallax_and_layers() {
        let mut fixture = Fixture::minimal();
        fixture.layer_count = 2;
        fixture.horizontal = vec![(0x0102, 3, 7), (0xFFFF, 255, 1)];
        fixture.vertical = vec![(0x8000, 0, 2)];
        fixture.layers = vec![
            (
                2,
                2,
                1,
                0x0020,
                16,
                vec![1, 2],
                vec![0x11, 0x22, 0x33, 0x44],
            ),
            (1, 1, 2, 0xFFFF, 200, Vec::new(), vec![0x55]),
        ];
        let backgrounds = Backgrounds::from_bytes(&fixture.build()).unwrap();
        assert_eq!(backgrounds.layer_count, 2);
        assert_eq!(backgrounds.horizontal.len(), 2);
        assert_eq!(
            backgrounds.horizontal[0],
            ParallaxEntry {
                parallax_factor: 0x0102,
                scroll_speed: 3 << 10,
                deform: 7
            }
        );
        assert_eq!(backgrounds.vertical[0].parallax_factor, 0x8000);
        assert_eq!(backgrounds.layers[0].width, 2);
        assert_eq!(backgrounds.layers[0].height, 2);
        assert_eq!(backgrounds.layers[0].layer_type, 1);
        assert_eq!(backgrounds.layers[0].scroll_speed, 16 << 10);
        assert_eq!(backgrounds.layers[0].line_scroll, [1, 2]);
        assert_eq!(backgrounds.layers[0].layout, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(backgrounds.layers[1].scroll_speed, 200 << 10);
        assert!(backgrounds.layers[1].line_scroll.is_empty());
    }

    #[test]
    fn expands_rle_line_scroll() {
        let mut fixture = Fixture::minimal();
        fixture.layer_count = 1;
        fixture.layers = vec![(1, 1, 1, 0, 0, vec![0xFF, 9, 3, 4, 5], vec![7])];
        let backgrounds = Backgrounds::from_bytes(&fixture.build()).unwrap();
        assert_eq!(backgrounds.layers[0].line_scroll, [9, 9, 4, 5]);
    }

    #[test]
    fn zero_count_rle_run_emits_nothing() {
        let mut fixture = Fixture::minimal();
        fixture.layer_count = 1;
        fixture.layers = vec![(1, 1, 1, 0, 0, vec![0xFF, 9, 0, 1], vec![7])];
        let backgrounds = Backgrounds::from_bytes(&fixture.build()).unwrap();
        assert_eq!(backgrounds.layers[0].line_scroll, [1]);
    }

    #[test]
    fn oversized_line_scroll_errors() {
        let mut fixture = Fixture::minimal();
        fixture.layer_count = 1;
        fixture.layers = vec![(1, 1, 1, 0, 0, vec![0u8; LINE_SCROLL_CAPACITY + 1], vec![0])];
        assert!(matches!(
            Backgrounds::from_bytes(&fixture.build()),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_trailing_bytes_unless_allowed() {
        let mut bytes = Fixture::minimal().build();
        bytes.push(0xAA);
        assert!(matches!(
            Backgrounds::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
        assert!(Backgrounds::from_bytes_allow_trailing(&bytes).is_ok());
    }

    #[test]
    fn truncated_inputs_error_cleanly() {
        let mut fixture = Fixture::minimal();
        fixture.layer_count = 1;
        fixture.horizontal = vec![(0x1234, 5, 6)];
        fixture.layers = vec![(2, 1, 1, 0x10, 1, vec![0xFF, 3, 2], vec![1, 2])];
        let bytes = fixture.build();
        for cut in 0..bytes.len() {
            assert!(
                Backgrounds::from_bytes(&bytes[..cut]).is_err(),
                "prefix of {cut} bytes unexpectedly parsed"
            );
        }
        assert!(Backgrounds::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn loads_from_source() {
        let mut source = MemorySource::new();
        source.insert(
            "Data/Stages/Zone01/Backgrounds.bin",
            Fixture::minimal().build(),
        );
        let backgrounds = Backgrounds::load("Data/Stages/Zone01", &source).unwrap();
        assert!(backgrounds.layers.is_empty());
        assert!(matches!(
            Backgrounds::load("Data/Stages/Missing", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0x5EED_1234u32;
        for length in 0..512usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = Backgrounds::from_bytes(&bytes);
            let _ = Backgrounds::from_bytes_allow_trailing(&bytes);
        }
    }
}
