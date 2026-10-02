//! Parser for the RSDKv4 `Data/Stages/<folder>/CollisionMasks.bin` file.
//!
//! Upstream (`LoadStageCollisions` in `RSDKv4/Scene.cpp`; identical in
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`) reads exactly
//! [`COLLISION_TILE_BYTES`] bytes for every one of the [`COLLISION_TILE_COUNT`] tiles, plane by
//! plane (`CPATH_COUNT` planes per tile, tile-major):
//!
//! ```text
//! u8   ceiling flag (high nibble) and collision flags (low nibble)
//! u32  angle, little-endian
//! u8[8]  sixteen 4-bit height samples: roof samples for ceiling tiles, floor samples otherwise
//! u8   collision bitmap for samples 8..15
//! u8   collision bitmap for samples 0..7
//! ```
//!
//! The remaining slope and wall masks are derived from those samples exactly as upstream does:
//! non-solid samples get `0x40` floor / `-0x40` roof sentinels, and the left/right wall heights
//! are the first/last sample a horizontal ray at that column crosses. All 15 bytes are always
//! consumed regardless of the tile type, so the file has a fixed size of
//! [`COLLISION_FILE_BYTES`].

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use crate::tiles::TILE_SIZE;
use retro_io::DataSource;

/// File name of the collision mask file inside a stage folder.
pub const COLLISION_FILE: &str = "CollisionMasks.bin";
/// Number of 16x16 collision tiles (`TILE_COUNT`).
pub const COLLISION_TILE_COUNT: usize = 0x400;
/// Number of collision planes per tile (`CPATH_COUNT`).
pub const COLLISION_PLANE_COUNT: usize = 2;
/// Number of bytes in one tile/plane record.
pub const COLLISION_TILE_BYTES: usize = 1 + 4 + TILE_SIZE / 2 + 2;
/// Byte size of a complete `CollisionMasks.bin`.
pub const COLLISION_FILE_BYTES: usize =
    COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT * COLLISION_TILE_BYTES;

/// One tile of one collision plane.
///
/// The four masks hold per-column heights (left/right wall) or per-row heights (floor/roof) in
/// the same orientation the engine uses: floor/roof are indexed by row, wall masks by column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollisionTile {
    /// High nibble of the flags byte; ceiling tiles derive their masks from the roof samples.
    pub ceiling: bool,
    /// Low nibble of the flags byte (upstream `collisionMasks[p].flags[t]`).
    pub flags: u8,
    /// Surface angle (upstream `collisionMasks[p].angles[t]`).
    pub angle: u32,
    /// Floor heights for each column.
    pub floor: [i8; TILE_SIZE],
    /// Roof heights for each column.
    pub roof: [i8; TILE_SIZE],
    /// Height of the left wall for each row.
    pub left_wall: [i8; TILE_SIZE],
    /// Height of the right wall for each row.
    pub right_wall: [i8; TILE_SIZE],
}

impl CollisionTile {
    /// Floor height at `column` (0..16), or `None` when out of range.
    pub fn floor_mask(&self, column: usize) -> Option<i8> {
        self.floor.get(column).copied()
    }

    /// Roof height at `column` (0..16), or `None` when out of range.
    pub fn roof_mask(&self, column: usize) -> Option<i8> {
        self.roof.get(column).copied()
    }

    /// Left wall height at `row` (0..16), or `None` when out of range.
    pub fn left_wall_mask(&self, row: usize) -> Option<i8> {
        self.left_wall.get(row).copied()
    }

    /// Right wall height at `row` (0..16), or `None` when out of range.
    pub fn right_wall_mask(&self, row: usize) -> Option<i8> {
        self.right_wall.get(row).copied()
    }
}

/// All collision tiles of one collision plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollisionPlane {
    /// One entry per 16x16 tile, in file order.
    pub tiles: Vec<CollisionTile>,
}

/// Parsed `CollisionMasks.bin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollisionMasks {
    /// The two collision planes (`CPATH_COUNT`).
    pub planes: [CollisionPlane; COLLISION_PLANE_COUNT],
}

impl CollisionMasks {
    /// Parses a `CollisionMasks.bin` from memory.
    ///
    /// The full [`COLLISION_FILE_BYTES`] payload is required; trailing bytes are ignored,
    /// matching the streaming loader.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);
        let mut first = Vec::with_capacity(COLLISION_TILE_COUNT);
        let mut second = Vec::with_capacity(COLLISION_TILE_COUNT);
        for _ in 0..COLLISION_TILE_COUNT {
            first.push(read_tile(&mut reader)?);
            second.push(read_tile(&mut reader)?);
        }
        Ok(Self {
            planes: [
                CollisionPlane { tiles: first },
                CollisionPlane { tiles: second },
            ],
        })
    }

    /// Reads `<stage_dir>/CollisionMasks.bin` through `src` and parses it.
    pub fn load(stage_dir: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        let directory = stage_dir.trim_end_matches('/');
        let path = if directory.is_empty() {
            COLLISION_FILE.to_owned()
        } else {
            format!("{directory}/{COLLISION_FILE}")
        };
        Self::from_bytes(&src.read(&path)?)
    }

    /// Returns collision plane `index`, or `None` when out of range.
    pub fn plane(&self, index: usize) -> Option<&CollisionPlane> {
        self.planes.get(index)
    }

    /// Returns tile `tile` of plane `plane`, or `None` when either index is out of range.
    pub fn tile(&self, plane: usize, tile: usize) -> Option<&CollisionTile> {
        self.planes.get(plane)?.tiles.get(tile)
    }
}

fn read_tile(reader: &mut Reader<'_>) -> Result<CollisionTile, FormatError> {
    let raw_flags = reader.read_u8()?;
    let ceiling = raw_flags >> 4 != 0;
    let flags = raw_flags & 0xF;
    let angle = reader.read_u32_le()?;

    let mut floor = [0i8; TILE_SIZE];
    let mut roof = [0i8; TILE_SIZE];
    let mut left_wall = [0i8; TILE_SIZE];
    let mut right_wall = [0i8; TILE_SIZE];

    if ceiling {
        let (pairs, _remainder) = roof.as_chunks_mut::<2>();
        for pair in pairs {
            let byte = reader.read_u8()?;
            pair[0] = (byte >> 4) as i8;
            pair[1] = (byte & 0xF) as i8;
        }

        let solid = reader.read_u8()?;
        for bit in 0..TILE_SIZE / 2 {
            if solid & (1 << bit) != 0 {
                floor[bit + TILE_SIZE / 2] = 0;
            } else {
                floor[bit + TILE_SIZE / 2] = 0x40;
                roof[bit + TILE_SIZE / 2] = -0x40;
            }
        }

        let solid = reader.read_u8()?;
        for bit in 0..TILE_SIZE / 2 {
            if solid & (1 << bit) != 0 {
                floor[bit] = 0;
            } else {
                floor[bit] = 0x40;
                roof[bit] = -0x40;
            }
        }

        for (column, mask) in left_wall.iter_mut().enumerate() {
            let mut height = 0i32;
            while height > -1 {
                if height == TILE_SIZE as i32 {
                    *mask = 0x40;
                    break;
                } else if (column as i32) > i32::from(roof[height as usize]) {
                    height += 1;
                } else {
                    *mask = height as i8;
                    break;
                }
            }
        }

        for (column, mask) in right_wall.iter_mut().enumerate() {
            let mut height = TILE_SIZE as i32 - 1;
            while height < TILE_SIZE as i32 {
                if height == -1 {
                    *mask = -0x40;
                    break;
                } else if (column as i32) > i32::from(roof[height as usize]) {
                    height -= 1;
                } else {
                    *mask = height as i8;
                    break;
                }
            }
        }
    } else {
        let (pairs, _remainder) = floor.as_chunks_mut::<2>();
        for pair in pairs {
            let byte = reader.read_u8()?;
            pair[0] = (byte >> 4) as i8;
            pair[1] = (byte & 0xF) as i8;
        }

        let solid = reader.read_u8()?;
        for bit in 0..TILE_SIZE / 2 {
            if solid & (1 << bit) != 0 {
                roof[bit + TILE_SIZE / 2] = 0xF;
            } else {
                floor[bit + TILE_SIZE / 2] = 0x40;
                roof[bit + TILE_SIZE / 2] = -0x40;
            }
        }

        let solid = reader.read_u8()?;
        for bit in 0..TILE_SIZE / 2 {
            if solid & (1 << bit) != 0 {
                roof[bit] = 0xF;
            } else {
                floor[bit] = 0x40;
                roof[bit] = -0x40;
            }
        }

        for (column, mask) in left_wall.iter_mut().enumerate() {
            let mut height = 0i32;
            while height > -1 {
                if height == TILE_SIZE as i32 {
                    *mask = 0x40;
                    break;
                } else if (column as i32) < i32::from(floor[height as usize]) {
                    height += 1;
                } else {
                    *mask = height as i8;
                    break;
                }
            }
        }

        for (column, mask) in right_wall.iter_mut().enumerate() {
            let mut height = TILE_SIZE as i32 - 1;
            while height < TILE_SIZE as i32 {
                if height == -1 {
                    *mask = -0x40;
                    break;
                } else if (column as i32) < i32::from(floor[height as usize]) {
                    height -= 1;
                } else {
                    *mask = height as i8;
                    break;
                }
            }
        }
    }

    Ok(CollisionTile {
        ceiling,
        flags,
        angle,
        floor,
        roof,
        left_wall,
        right_wall,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    /// Serialises one tile record in the exact on-disk order.
    fn tile_bytes(flags: u8, angle: u32, samples: [u8; 8], solid_a: u8, solid_b: u8) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(COLLISION_TILE_BYTES);
        bytes.push(flags);
        bytes.extend_from_slice(&angle.to_le_bytes());
        bytes.extend_from_slice(&samples);
        bytes.push(solid_a);
        bytes.push(solid_b);
        bytes
    }

    fn blank_file() -> Vec<u8> {
        let tile = tile_bytes(0, 0, [0; 8], 0, 0);
        let mut bytes = Vec::with_capacity(COLLISION_FILE_BYTES);
        for _ in 0..COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT {
            bytes.extend_from_slice(&tile);
        }
        bytes
    }

    #[test]
    fn parses_blank_masks() {
        let masks = CollisionMasks::from_bytes(&blank_file()).unwrap();
        assert_eq!(masks.planes[0].tiles.len(), COLLISION_TILE_COUNT);
        assert_eq!(masks.planes[1].tiles.len(), COLLISION_TILE_COUNT);
        let tile = masks.tile(1, 1023).unwrap();
        assert!(!tile.ceiling);
        assert_eq!(tile.flags, 0);
        assert_eq!(tile.angle, 0);
        assert_eq!(tile.floor, [0x40; TILE_SIZE]);
        assert_eq!(tile.roof, [-0x40; TILE_SIZE]);
        assert_eq!(tile.left_wall, [0x40; TILE_SIZE]);
        assert_eq!(tile.right_wall, [-0x40; TILE_SIZE]);
        assert_eq!(masks.plane(2), None);
        assert_eq!(masks.tile(0, COLLISION_TILE_COUNT), None);
    }

    #[test]
    fn parses_regular_tile_samples() {
        let mut bytes = tile_bytes(
            0x0F,
            0x1234_5678,
            [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0],
            0x01,
            0x80,
        );
        bytes.resize(COLLISION_FILE_BYTES, 0);
        let masks = CollisionMasks::from_bytes(&bytes).unwrap();
        let tile = &masks.planes[0].tiles[0];
        assert!(!tile.ceiling);
        assert_eq!(tile.flags, 0xF);
        assert_eq!(tile.angle, 0x1234_5678);
        assert_eq!(
            tile.floor,
            [
                0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 8, 9, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
                0x40
            ]
        );
        assert_eq!(
            tile.roof,
            [
                -0x40, -0x40, -0x40, -0x40, -0x40, -0x40, -0x40, 0xF, 0xF, -0x40, -0x40, -0x40,
                -0x40, -0x40, -0x40, -0x40
            ]
        );
        assert_eq!(tile.left_wall[0], 0x40);
        assert_eq!(tile.left_wall[8], 7);
        assert_eq!(tile.floor_mask(8), Some(9));
        assert_eq!(tile.roof_mask(0), Some(-0x40));
        assert_eq!(tile.floor_mask(16), None);
    }

    #[test]
    fn parses_ceiling_tile_samples() {
        let mut bytes = tile_bytes(
            0x1F,
            1,
            [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0],
            0xFF,
            0xFF,
        );
        bytes.resize(COLLISION_FILE_BYTES, 0);
        let masks = CollisionMasks::from_bytes(&bytes).unwrap();
        let tile = &masks.planes[0].tiles[0];
        assert!(tile.ceiling);
        assert_eq!(tile.flags, 0xF);
        assert_eq!(tile.angle, 1);
        assert_eq!(
            tile.roof,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0]
        );
        assert_eq!(tile.floor, [0; TILE_SIZE]);
        assert_eq!(tile.left_wall[0], 0);
        assert_eq!(tile.left_wall[2], 1);
        assert_eq!(tile.right_wall[0], 15);
        assert_eq!(tile.left_wall_mask(2), Some(1));
        assert_eq!(tile.right_wall_mask(0), Some(15));
    }

    #[test]
    fn truncated_inputs_error() {
        let bytes = blank_file();
        for cut in [0, 1, 4, 5, 14, 15, 16, 30, COLLISION_FILE_BYTES - 1] {
            assert!(
                matches!(
                    CollisionMasks::from_bytes(&bytes[..cut]),
                    Err(FormatError::Truncated)
                ),
                "prefix of {cut} bytes unexpectedly parsed"
            );
        }
        assert!(CollisionMasks::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn ignores_trailing_bytes() {
        let mut bytes = blank_file();
        bytes.push(0xFF);
        assert!(CollisionMasks::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn loads_from_source() {
        let mut source = MemorySource::new();
        source.insert("Data/Stages/Zone01/CollisionMasks.bin", blank_file());
        let masks = CollisionMasks::load("Data/Stages/Zone01", &source).unwrap();
        assert_eq!(masks.planes[0].tiles.len(), COLLISION_TILE_COUNT);
        assert!(matches!(
            CollisionMasks::load("Data/Stages/Missing", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0x0BAD_F00Du32;
        for length in 0..256usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = CollisionMasks::from_bytes(&bytes);
        }
    }
}
