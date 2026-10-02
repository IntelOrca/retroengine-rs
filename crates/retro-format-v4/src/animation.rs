//! Parser for RSDKv4 animation files (`Data/Animations/*.ani`).
//!
//! On-disk order (`LoadAnimationFile` in `RSDKv4/Animation.cpp`; identical in
//! `RSDKv5/RSDK/Graphics/Legacy/AnimationLegacy.cpp`):
//!
//! ```text
//! u8       sheet count (at most SHEET_LIMIT)
//! repeat:  string sheet path relative to Data/Sprites (`Players/Sonic1.gif`)
//! u8       animation count
//! repeat animation count times:
//!     string name
//!     u8     frame count
//!     u8     speed
//!     u8     loop index
//!     u8     rotation style (0 none, 1 full, 2 45-degree, 3 static frames)
//!     repeat frame count times:
//!         u8  sheet index (into the sheet list)
//!         u8  hitbox id
//!         u8  sprite x, u8 sprite y, u8 width, u8 height
//!         i8  pivot x, i8 pivot y
//! u8       hitbox count
//! repeat:  4 sides x 8 directions of i8 extents (left, top, right, bottom per direction)
//! ```
//!
//! Upstream skips a zero-length sheet name and leaves the corresponding slot untouched; this
//! parser stores an empty string instead. For [`ROTSTYLE_STATICFRAMES`] animations the engine
//! halves `frameCount` after reading all frames (the second half of the frame list holds the
//! extra 90-degree rotations), so [`Animation::playback_frame_count`] reports the halved value
//! while [`Animation::frames`] keeps every serialised frame.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// Directory containing `.ani` files.
pub const ANIMATION_DIR: &str = "Data/Animations";
/// Maximum sheet count upstream supports (`sheetIDs[0x18]`).
pub const SHEET_LIMIT: usize = 0x18;
/// Number of hitbox directions (`HITBOX_DIR_COUNT`).
pub const HITBOX_DIRECTIONS: usize = 0x8;

/// Rotation style: no rotation.
pub const ROTSTYLE_NONE: u8 = 0;
/// Rotation style: full 360-degree frames.
pub const ROTSTYLE_FULL: u8 = 1;
/// Rotation style: 45-degree frames.
pub const ROTSTYLE_45DEG: u8 = 2;
/// Rotation style: 90-degree frames appended to the frame list.
pub const ROTSTYLE_STATICFRAMES: u8 = 3;

/// One animation frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnimationFrame {
    /// On-disk sheet index (position in [`AnimationFile::sheets`]).
    pub sheet_index: u8,
    /// Resolved sheet path.
    pub sheet: String,
    /// Hitbox id into [`AnimationFile::hitboxes`].
    pub hitbox_id: u8,
    /// Source x in the sheet, in pixels.
    pub x: u8,
    /// Source y in the sheet, in pixels.
    pub y: u8,
    /// Frame width in pixels.
    pub width: u8,
    /// Frame height in pixels.
    pub height: u8,
    /// Signed pivot x.
    pub pivot_x: i8,
    /// Signed pivot y.
    pub pivot_y: i8,
}

/// One animation entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Animation {
    /// Animation name (`Running`).
    pub name: String,
    /// Frame count as stored on disk.
    pub frame_count: u8,
    /// Frame count used for playback (halved for [`ROTSTYLE_STATICFRAMES`]).
    pub playback_frame_count: u8,
    /// Animation speed.
    pub speed: u8,
    /// Loop index.
    pub loop_point: u8,
    /// Rotation style, one of the `ROTSTYLE_*` constants.
    pub rotation_style: u8,
    /// All serialised frames, including extra rotation frames.
    pub frames: Vec<AnimationFrame>,
}

/// One hitbox: signed extents for each of [`HITBOX_DIRECTIONS`] facing directions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hitbox {
    /// Left extents by direction.
    pub left: [i8; HITBOX_DIRECTIONS],
    /// Top extents by direction.
    pub top: [i8; HITBOX_DIRECTIONS],
    /// Right extents by direction.
    pub right: [i8; HITBOX_DIRECTIONS],
    /// Bottom extents by direction.
    pub bottom: [i8; HITBOX_DIRECTIONS],
}

/// Parsed `.ani` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnimationFile {
    /// Sheet paths in file order.
    pub sheets: Vec<String>,
    /// Animations in file order.
    pub animations: Vec<Animation>,
    /// Hitboxes in file order.
    pub hitboxes: Vec<Hitbox>,
}

impl AnimationFile {
    /// Parses an `.ani` file from memory, rejecting trailing bytes.
    ///
    /// [`AnimationFile::from_bytes_allow_trailing`] ignores extra bytes the way the engine does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, true)
    }

    /// Parses an `.ani` file from memory, ignoring bytes after the last hitbox.
    pub fn from_bytes_allow_trailing(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::parse(bytes, false)
    }

    fn parse(bytes: &[u8], strict: bool) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);

        let sheet_count = reader.read_u8()? as usize;
        if sheet_count > SHEET_LIMIT {
            return Err(FormatError::unsupported(format!(
                "sheet count {sheet_count} exceeds the engine limit of {SHEET_LIMIT}"
            )));
        }
        let mut sheets = Vec::with_capacity(sheet_count);
        for _ in 0..sheet_count {
            sheets.push(reader.read_string()?);
        }

        let animation_count = reader.read_u8()? as usize;
        let mut animations = Vec::with_capacity(animation_count);
        for _ in 0..animation_count {
            let name = reader.read_string()?;
            let frame_count = reader.read_u8()?;
            let speed = reader.read_u8()?;
            let loop_point = reader.read_u8()?;
            let rotation_style = reader.read_u8()?;

            let mut frames = Vec::with_capacity(usize::from(frame_count));
            for _ in 0..frame_count {
                let sheet_index = reader.read_u8()?;
                let sheet = sheets.get(usize::from(sheet_index)).ok_or_else(|| {
                    FormatError::invalid(format!(
                        "animation {name:?} references sheet {sheet_index}, but only {} sheets \
                         are defined",
                        sheets.len()
                    ))
                })?;
                frames.push(AnimationFrame {
                    sheet_index,
                    sheet: sheet.clone(),
                    hitbox_id: reader.read_u8()?,
                    x: reader.read_u8()?,
                    y: reader.read_u8()?,
                    width: reader.read_u8()?,
                    height: reader.read_u8()?,
                    pivot_x: reader.read_i8()?,
                    pivot_y: reader.read_i8()?,
                });
            }

            let playback_frame_count = if rotation_style == ROTSTYLE_STATICFRAMES {
                frame_count >> 1
            } else {
                frame_count
            };
            animations.push(Animation {
                name,
                frame_count,
                playback_frame_count,
                speed,
                loop_point,
                rotation_style,
                frames,
            });
        }

        let hitbox_count = reader.read_u8()? as usize;
        let mut hitboxes = Vec::with_capacity(hitbox_count);
        for _ in 0..hitbox_count {
            let mut left = [0i8; HITBOX_DIRECTIONS];
            let mut top = [0i8; HITBOX_DIRECTIONS];
            let mut right = [0i8; HITBOX_DIRECTIONS];
            let mut bottom = [0i8; HITBOX_DIRECTIONS];
            for direction in 0..HITBOX_DIRECTIONS {
                left[direction] = reader.read_i8()?;
                top[direction] = reader.read_i8()?;
                right[direction] = reader.read_i8()?;
                bottom[direction] = reader.read_i8()?;
            }
            hitboxes.push(Hitbox {
                left,
                top,
                right,
                bottom,
            });
        }

        if strict && !reader.is_empty() {
            return Err(FormatError::invalid(format!(
                "{} trailing bytes after animation hitboxes",
                reader.remaining()
            )));
        }

        Ok(Self {
            sheets,
            animations,
            hitboxes,
        })
    }

    /// Reads an `.ani` file at `path` (relative to the source root, e.g.
    /// `Data/Animations/Sonic.ani`) through `src` and parses it.
    pub fn load(path: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(path)?)
    }

    /// Reads `Data/Animations/<name>.ani` through `src`.
    pub fn load_named(name: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::load(&format!("{ANIMATION_DIR}/{name}.ani"), src)
    }

    /// Total number of serialised frames across all animations.
    pub fn total_frames(&self) -> usize {
        self.animations
            .iter()
            .map(|animation| animation.frames.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    type FrameTuple = (u8, u8, u8, u8, u8, u8, i8, i8);
    type AnimationTuple = (String, u8, u8, u8, u8, Vec<FrameTuple>);

    struct Fixture {
        sheets: Vec<String>,
        animations: Vec<AnimationTuple>,
        hitboxes: Vec<([i8; HITBOX_DIRECTIONS], [i8; HITBOX_DIRECTIONS])>,
    }

    impl Fixture {
        fn minimal() -> Self {
            Self {
                sheets: Vec::new(),
                animations: Vec::new(),
                hitboxes: Vec::new(),
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.push(self.sheets.len() as u8);
            for sheet in &self.sheets {
                bytes.push(sheet.len() as u8);
                bytes.extend_from_slice(sheet.as_bytes());
            }
            bytes.push(self.animations.len() as u8);
            for (name, frame_count, speed, loop_point, rotation_style, frames) in &self.animations {
                bytes.push(name.len() as u8);
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(*frame_count);
                bytes.push(*speed);
                bytes.push(*loop_point);
                bytes.push(*rotation_style);
                assert_eq!(usize::from(*frame_count), frames.len());
                for (sheet, hitbox, x, y, width, height, pivot_x, pivot_y) in frames {
                    bytes.push(*sheet);
                    bytes.push(*hitbox);
                    bytes.push(*x);
                    bytes.push(*y);
                    bytes.push(*width);
                    bytes.push(*height);
                    bytes.push(*pivot_x as u8);
                    bytes.push(*pivot_y as u8);
                }
            }
            bytes.push(self.hitboxes.len() as u8);
            for (left, top) in &self.hitboxes {
                let right = [0i8; HITBOX_DIRECTIONS];
                let bottom = [0i8; HITBOX_DIRECTIONS];
                for direction in 0..HITBOX_DIRECTIONS {
                    bytes.push(left[direction] as u8);
                    bytes.push(top[direction] as u8);
                    bytes.push(right[direction] as u8);
                    bytes.push(bottom[direction] as u8);
                }
            }
            bytes
        }
    }

    #[test]
    fn parses_minimal_file() {
        let file = AnimationFile::from_bytes(&Fixture::minimal().build()).unwrap();
        assert!(file.sheets.is_empty());
        assert!(file.animations.is_empty());
        assert!(file.hitboxes.is_empty());
        assert_eq!(file.total_frames(), 0);
    }

    #[test]
    fn parses_full_file() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = vec![
            "Players/Sonic1.gif".to_owned(),
            "Players/Sonic2.gif".to_owned(),
        ];
        fixture.animations = vec![
            (
                "Running".to_owned(),
                2,
                80,
                0,
                ROTSTYLE_NONE,
                vec![(0, 1, 4, 5, 24, 32, -2, -3), (1, 0, 28, 5, 24, 32, -4, -5)],
            ),
            (
                "Walk".to_owned(),
                4,
                10,
                2,
                ROTSTYLE_STATICFRAMES,
                vec![
                    (1, 0, 0, 0, 16, 16, 0, 0),
                    (1, 0, 16, 0, 16, 16, 0, 0),
                    (1, 0, 32, 0, 16, 16, 0, 0),
                    (1, 0, 48, 0, 16, 16, 0, 0),
                ],
            ),
        ];
        fixture.hitboxes = vec![([-1, 0, 1, 2, 3, 4, 5, 6], [7, 7, 7, 7, 7, 7, 7, 7])];

        let file = AnimationFile::from_bytes(&fixture.build()).unwrap();
        assert_eq!(file.sheets.len(), 2);
        assert_eq!(file.animations[0].name, "Running");
        assert_eq!(file.animations[0].frame_count, 2);
        assert_eq!(file.animations[0].playback_frame_count, 2);
        assert_eq!(file.animations[0].speed, 80);
        assert_eq!(file.animations[0].loop_point, 0);
        assert_eq!(file.animations[0].rotation_style, ROTSTYLE_NONE);
        assert_eq!(file.animations[0].frames[1].sheet, "Players/Sonic2.gif");
        assert_eq!(file.animations[0].frames[1].pivot_x, -4);
        assert_eq!(file.animations[0].frames[0].hitbox_id, 1);

        let walk = &file.animations[1];
        assert_eq!(walk.frame_count, 4);
        assert_eq!(walk.playback_frame_count, 2);
        assert_eq!(walk.frames.len(), 4);
        assert_eq!(walk.frames[3].x, 48);

        assert_eq!(file.hitboxes.len(), 1);
        assert_eq!(file.hitboxes[0].left, [-1, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(file.hitboxes[0].top, [7; HITBOX_DIRECTIONS]);
        assert_eq!(file.total_frames(), 6);
    }

    #[test]
    fn parses_empty_sheet_names() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = vec![String::new(), "A.gif".to_owned()];
        fixture.animations = vec![(
            "Idle".to_owned(),
            1,
            1,
            0,
            ROTSTYLE_NONE,
            vec![(0, 0, 0, 0, 1, 1, 0, 0)],
        )];
        let file = AnimationFile::from_bytes(&fixture.build()).unwrap();
        assert_eq!(file.sheets, ["", "A.gif"]);
        assert_eq!(file.animations[0].frames[0].sheet, "");
    }

    #[test]
    fn rejects_sheet_index_out_of_range() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = vec!["A.gif".to_owned()];
        fixture.animations = vec![(
            "Bad".to_owned(),
            1,
            1,
            0,
            ROTSTYLE_NONE,
            vec![(5, 0, 0, 0, 1, 1, 0, 0)],
        )];
        assert!(matches!(
            AnimationFile::from_bytes(&fixture.build()),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_too_many_sheets() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = (0..=SHEET_LIMIT)
            .map(|index| format!("S{index}.gif"))
            .collect();
        assert!(matches!(
            AnimationFile::from_bytes(&fixture.build()),
            Err(FormatError::Unsupported(_))
        ));
    }

    #[test]
    fn parses_maximum_counts() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = (0..SHEET_LIMIT)
            .map(|index| format!("S{index}.gif"))
            .collect();
        let frames: Vec<FrameTuple> = (0..255)
            .map(|index| ((index % SHEET_LIMIT) as u8, 0, 0, 0, 1, 1, 0, 0))
            .collect();
        fixture.animations = (0..255)
            .map(|index| {
                (
                    format!("Anim{index}"),
                    255,
                    1,
                    0,
                    ROTSTYLE_FULL,
                    frames.clone(),
                )
            })
            .collect();
        fixture.hitboxes = vec![([0; HITBOX_DIRECTIONS], [0; HITBOX_DIRECTIONS]); 255];
        let file = AnimationFile::from_bytes(&fixture.build()).unwrap();
        assert_eq!(file.sheets.len(), SHEET_LIMIT);
        assert_eq!(file.animations.len(), 255);
        assert_eq!(file.hitboxes.len(), 255);
        assert_eq!(file.total_frames(), 255 * 255);
    }

    #[test]
    fn rejects_trailing_bytes_unless_allowed() {
        let mut bytes = Fixture::minimal().build();
        bytes.push(0xAA);
        assert!(matches!(
            AnimationFile::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
        assert!(AnimationFile::from_bytes_allow_trailing(&bytes).is_ok());
    }

    #[test]
    fn truncated_inputs_error_cleanly() {
        let mut fixture = Fixture::minimal();
        fixture.sheets = vec!["A.gif".to_owned(), "B.gif".to_owned()];
        fixture.animations = vec![(
            "Run".to_owned(),
            2,
            3,
            1,
            ROTSTYLE_STATICFRAMES,
            vec![(0, 0, 1, 2, 3, 4, -1, -2), (1, 1, 5, 6, 7, 8, -3, -4)],
        )];
        fixture.hitboxes = vec![([-1; HITBOX_DIRECTIONS], [1; HITBOX_DIRECTIONS])];
        let bytes = fixture.build();
        for cut in 0..bytes.len() {
            assert!(
                AnimationFile::from_bytes(&bytes[..cut]).is_err(),
                "prefix of {cut} bytes unexpectedly parsed"
            );
        }
        assert!(AnimationFile::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn rejects_invalid_utf8_names() {
        let mut bytes = Vec::new();
        bytes.push(1);
        bytes.extend_from_slice(&[2, 0xC3, 0x28]);
        bytes.push(0);
        bytes.push(0);
        assert!(matches!(
            AnimationFile::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn loads_from_source() {
        let mut source = MemorySource::new();
        source.insert("Data/Animations/Sonic.ani", Fixture::minimal().build());
        let file = AnimationFile::load("Data/Animations/Sonic.ani", &source).unwrap();
        assert!(file.animations.is_empty());
        let file = AnimationFile::load_named("Sonic", &source).unwrap();
        assert!(file.animations.is_empty());
        assert!(matches!(
            AnimationFile::load_named("Missing", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0xFACE_FEEDu32;
        for length in 0..512usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = AnimationFile::from_bytes(&bytes);
            let _ = AnimationFile::from_bytes_allow_trailing(&bytes);
        }
    }
}
