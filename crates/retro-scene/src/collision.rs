//! Tile and object collision routines.
//!
//! Ported from `RSDKv4/Collision.cpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195). The tile
//! routines (`Find*Position`, `Object*Collision`, `Object*Grip`, `TouchCollision`,
//! `BoxCollision`) operate on the [`EntityStore`] slot selected by `objectEntityPos`, return
//! upstream's `scriptEng.checkResult`, and update the entity exactly like the reference.
//!
//! `ProcessObjectMovement` is a documented divergence: the full upstream implementation is
//! `ProcessAirCollision`/`ProcessPathGrip` plus the hitbox-driven `ProcessTileCollisions`
//! (~1000 lines). This port performs the same fixed-point integration and a deterministic
//! three-sensor floor probe using [`SceneCollision::find_floor_position`], but does not
//! implement path grip, slope/wall pushing, or ceiling movement. It never panics and is fully
//! deterministic; see the M3 report for the explicit gap list.

use serde::Serialize;

use retro_format_v4::{CollisionMasks, Hitbox, Tile128, TileSheet128};

use crate::entity::{Entity, EntityStore, TypeGroupList};

/// `CSIDE_FLOOR`.
pub const CSIDE_FLOOR: i32 = 0;
/// `CSIDE_LWALL`.
pub const CSIDE_LWALL: i32 = 1;
/// `CSIDE_RWALL`.
pub const CSIDE_RWALL: i32 = 2;
/// `CSIDE_ROOF`.
pub const CSIDE_ROOF: i32 = 3;
/// `CSIDE_LENTITY` (rev03).
pub const CSIDE_LENTITY: i32 = 4;
/// `CSIDE_RENTITY` (rev03).
pub const CSIDE_RENTITY: i32 = 5;

/// `CMODE_FLOOR`.
pub const CMODE_FLOOR: u8 = 0;
/// `CMODE_LWALL`.
pub const CMODE_LWALL: u8 = 1;
/// `CMODE_ROOF`.
pub const CMODE_ROOF: u8 = 2;
/// `CMODE_RWALL`.
pub const CMODE_RWALL: u8 = 3;

/// `SOLID_ALL`.
pub const SOLID_ALL: u8 = 0;
/// `SOLID_TOP`.
pub const SOLID_TOP: u8 = 1;
/// `SOLID_LRB`.
pub const SOLID_LRB: u8 = 2;
/// `SOLID_NONE`.
pub const SOLID_NONE: u8 = 3;

/// `FLIP_NONE`.
pub const FLIP_NONE: u8 = 0;
/// `FLIP_X`.
pub const FLIP_X: u8 = 1;
/// `FLIP_Y`.
pub const FLIP_Y: u8 = 2;
/// `FLIP_XY`.
pub const FLIP_XY: u8 = 3;

/// `C_TOUCH`.
pub const C_TOUCH: i32 = 0;
/// `C_SOLID`.
pub const C_SOLID: i32 = 1;
/// `C_SOLID2`.
pub const C_SOLID2: i32 = 2;
/// `C_PLATFORM`.
pub const C_PLATFORM: i32 = 3;
/// `C_BOX`.
pub const C_BOX: i32 = 0x10000;

/// One collision sensor (`struct CollisionSensor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct CollisionSensor {
    /// Sensor X position in 16.16 fixed point.
    pub xpos: i32,
    /// Sensor Y position in 16.16 fixed point.
    pub ypos: i32,
    /// Surface angle.
    pub angle: i32,
    /// Whether the sensor collided.
    pub collided: bool,
}

impl CollisionSensor {
    /// Creates a sensor at `(x, y)` (16.16 fixed point) with `angle`.
    #[must_use]
    pub fn new(x: i32, y: i32, angle: i32) -> Self {
        Self {
            xpos: x,
            ypos: y,
            angle,
            collided: false,
        }
    }
}

/// Tile collision state: the chunk grid, the 128x128 chunk tile sheet and the collision masks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneCollision {
    /// Active layout.
    pub layout: crate::stage::StageLayout,
    /// Chunk-to-16x16 tile mapping (`tiles128x128`).
    pub tiles: TileSheet128,
    /// Per-tile collision masks (`collisionMasks`).
    pub masks: CollisionMasks,
    /// `collisionTolerance`, set by `ProcessTileCollisions`/movement.
    pub collision_tolerance: i32,
}

impl SceneCollision {
    /// Creates a collision context from its parts.
    #[must_use]
    pub fn new(
        layout: crate::stage::StageLayout,
        tiles: TileSheet128,
        masks: CollisionMasks,
    ) -> Self {
        Self {
            layout,
            tiles,
            masks,
            collision_tolerance: 15,
        }
    }

    /// Returns the 16x16 tile of `layout_chunk` at `(tile_x, tile_y)`, or a zeroed entry when
    /// the layout or sheet is out of range. Upstream would read out of bounds here.
    fn chunk_tile(&self, layout_chunk: u16, tile_x: i32, tile_y: i32) -> Tile128 {
        let index = (usize::from(layout_chunk) << 6)
            .wrapping_add(usize::try_from(tile_x).unwrap_or(0))
            .wrapping_add(usize::try_from(tile_y).unwrap_or(0) << 3);
        self.tiles.entries.get(index).cloned().unwrap_or(Tile128 {
            direction: 0,
            visual_plane: 0,
            tile_index: 0,
            collision_flag_a: 0,
            collision_flag_b: 0,
        })
    }

    /// Returns the collision mask tile at `(plane, tile_index)`, or `None` when out of range.
    #[must_use]
    pub fn mask_tile(
        &self,
        plane: usize,
        tile_index: usize,
    ) -> Option<&retro_format_v4::CollisionTile> {
        self.masks.tile(plane, tile_index)
    }

    /// Returns the solidity nibble for `plane`, matching upstream
    /// `tiles128x128.collisionFlags[plane][chunk]` (plane 0 = high nibble, plane 1 = low).
    fn tile_flag(tile: &Tile128, plane: usize) -> u8 {
        if plane == 0 {
            tile.collision_flag_a
        } else {
            tile.collision_flag_b
        }
    }

    fn floor_height(&self, plane: usize, column: usize, tile_index: usize) -> i8 {
        self.mask_tile(plane, tile_index)
            .and_then(|tile| tile.floor.get(column).copied())
            .unwrap_or(0x40)
    }

    fn roof_height(&self, plane: usize, column: usize, tile_index: usize) -> i8 {
        self.mask_tile(plane, tile_index)
            .and_then(|tile| tile.roof.get(column).copied())
            .unwrap_or(-0x40)
    }

    fn left_wall_height(&self, plane: usize, row: usize, tile_index: usize) -> i8 {
        self.mask_tile(plane, tile_index)
            .and_then(|tile| tile.left_wall.get(row).copied())
            .unwrap_or(0x40)
    }

    fn right_wall_height(&self, plane: usize, row: usize, tile_index: usize) -> i8 {
        self.mask_tile(plane, tile_index)
            .and_then(|tile| tile.right_wall.get(row).copied())
            .unwrap_or(-0x40)
    }

    fn angle(&self, plane: usize, tile_index: usize) -> u32 {
        self.mask_tile(plane, tile_index)
            .map(|tile| tile.angle)
            .unwrap_or(0)
    }

    /// `FindFloorPosition`: probes up to three vertical steps below `start_y` for a floor surface
    /// in the same 16x16 tile path.
    pub fn find_floor_position(
        &self,
        plane: usize,
        mut sensor: CollisionSensor,
        start_y: i32,
    ) -> CollisionSensor {
        let angle = sensor.angle;
        let mut step = 0;
        while step < 16 * 3 {
            if !sensor.collided {
                let x_pos = sensor.xpos >> 16;
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let y_pos = (sensor.ypos >> 16).wrapping_sub(16).wrapping_add(step);
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                if x_pos > -1 && y_pos > -1 {
                    let layout_chunk = self.layout.chunk(chunk_x, chunk_y);
                    let tile = self.chunk_tile(layout_chunk, tile_x, tile_y);
                    let tile_index = usize::from(tile.tile_index);
                    let flags = Self::tile_flag(&tile, plane);
                    if flags != SOLID_LRB && flags != SOLID_NONE {
                        match tile.direction {
                            FLIP_NONE => {
                                let column = (x_pos & 15) as usize;
                                let mask = self.floor_height(plane, column, tile_index);
                                if mask < 0x40 {
                                    sensor.ypos = i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle = (self.angle(plane, tile_index) & 0xFF) as i32;
                                }
                            }
                            FLIP_X => {
                                let column = 15 - (x_pos & 15);
                                let mask = self.floor_height(plane, column as usize, tile_index);
                                if mask < 0x40 {
                                    sensor.ypos = i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        0x100 - ((self.angle(plane, tile_index) & 0xFF) as i32);
                                }
                            }
                            FLIP_Y => {
                                let column = (x_pos & 15) as usize;
                                let mask = self.roof_height(plane, column, tile_index);
                                if mask > -0x40 {
                                    sensor.ypos =
                                        15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        0x180 - ((self.angle(plane, tile_index) >> 24) as i32);
                                }
                            }
                            _ => {
                                let column = 15 - (x_pos & 15);
                                let mask = self.roof_height(plane, column as usize, tile_index);
                                if mask > -0x40 {
                                    sensor.ypos =
                                        15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - (0x180 - ((self.angle(plane, tile_index) >> 24) as i32));
                                }
                            }
                        }
                    }
                    if sensor.collided {
                        if sensor.angle < 0 {
                            sensor.angle += 0x100;
                        }
                        if sensor.angle >= 0x100 {
                            sensor.angle -= 0x100;
                        }
                        let difference = (sensor.angle - angle).abs();
                        if difference > 0x20
                            && (sensor.angle - 0x100 - angle).abs() > 0x20
                            && (sensor.angle + 0x100 - angle).abs() > 0x20
                        {
                            sensor.ypos = start_y.wrapping_shl(16);
                            sensor.collided = false;
                            sensor.angle = angle;
                            step = 16 * 3;
                        } else if (sensor.ypos - start_y).abs() > self.collision_tolerance {
                            sensor.ypos = start_y.wrapping_shl(16);
                            sensor.collided = false;
                        }
                    }
                }
            }
            step += 16;
        }
        sensor
    }

    /// `FindLWallPosition`.
    pub fn find_lwall_position(
        &self,
        plane: usize,
        mut sensor: CollisionSensor,
        start_x: i32,
    ) -> CollisionSensor {
        let angle = sensor.angle;
        let mut step = 0;
        while step < 16 * 3 {
            if !sensor.collided {
                let x_pos = (sensor.xpos >> 16).wrapping_sub(16).wrapping_add(step);
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let y_pos = sensor.ypos >> 16;
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                if x_pos > -1 && y_pos > -1 {
                    let layout_chunk = self.layout.chunk(chunk_x, chunk_y);
                    let tile = self.chunk_tile(layout_chunk, tile_x, tile_y);
                    let tile_index = usize::from(tile.tile_index);
                    if Self::tile_flag(&tile, plane) < SOLID_NONE {
                        let row = usize::try_from(y_pos & 15).unwrap_or(0);
                        match tile.direction {
                            FLIP_NONE => {
                                let mask = self.left_wall_height(plane, row, tile_index);
                                if mask < 0x40 {
                                    sensor.xpos = i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        ((self.angle(plane, tile_index) >> 8) & 0xFF) as i32;
                                }
                            }
                            FLIP_X => {
                                let mask = self.right_wall_height(plane, row, tile_index);
                                if mask > -0x40 {
                                    sensor.xpos =
                                        15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - ((self.angle(plane, tile_index) >> 16) & 0xFF) as i32;
                                }
                            }
                            FLIP_Y => {
                                let row = 15 - (y_pos & 15);
                                let mask = self.left_wall_height(plane, row as usize, tile_index);
                                if mask < 0x40 {
                                    sensor.xpos = i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x180
                                        - ((self.angle(plane, tile_index) >> 8) & 0xFF) as i32;
                                }
                            }
                            _ => {
                                let row = 15 - (y_pos & 15);
                                let mask = self.right_wall_height(plane, row as usize, tile_index);
                                if mask > -0x40 {
                                    sensor.xpos =
                                        15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - (0x180
                                            - ((self.angle(plane, tile_index) >> 16) & 0xFF)
                                                as i32);
                                }
                            }
                        }
                    }
                    if sensor.collided {
                        if sensor.angle < 0 {
                            sensor.angle += 0x100;
                        }
                        if sensor.angle >= 0x100 {
                            sensor.angle -= 0x100;
                        }
                        if (angle - sensor.angle).abs() > 0x20 {
                            sensor.xpos = start_x.wrapping_shl(16);
                            sensor.collided = false;
                            sensor.angle = angle;
                            step = 16 * 3;
                        } else if (sensor.xpos - start_x).abs() > self.collision_tolerance {
                            sensor.xpos = start_x.wrapping_shl(16);
                            sensor.collided = false;
                        }
                    }
                }
            }
            step += 16;
        }
        sensor
    }

    /// `FindRoofPosition`.
    pub fn find_roof_position(
        &self,
        plane: usize,
        mut sensor: CollisionSensor,
        start_y: i32,
    ) -> CollisionSensor {
        let angle = sensor.angle;
        let mut step = 0;
        while step < 16 * 3 {
            if !sensor.collided {
                let x_pos = sensor.xpos >> 16;
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let y_pos = (sensor.ypos >> 16).wrapping_add(16).wrapping_sub(step);
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                if x_pos > -1 && y_pos > -1 {
                    let layout_chunk = self.layout.chunk(chunk_x, chunk_y);
                    let tile = self.chunk_tile(layout_chunk, tile_x, tile_y);
                    let tile_index = usize::from(tile.tile_index);
                    if Self::tile_flag(&tile, plane) < SOLID_NONE {
                        match tile.direction {
                            FLIP_NONE => {
                                let column = (x_pos & 15) as usize;
                                let mask = self.roof_height(plane, column, tile_index);
                                if mask > -0x40 {
                                    sensor.ypos = i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        ((self.angle(plane, tile_index) >> 24) & 0xFF) as i32;
                                }
                            }
                            FLIP_X => {
                                let column = 15 - (x_pos & 15);
                                let mask = self.roof_height(plane, column as usize, tile_index);
                                if mask > -0x40 {
                                    sensor.ypos = i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - ((self.angle(plane, tile_index) >> 24) & 0xFF) as i32;
                                }
                            }
                            FLIP_Y => {
                                let column = (x_pos & 15) as usize;
                                let mask = self.floor_height(plane, column, tile_index);
                                if mask < 0x40 {
                                    sensor.ypos =
                                        15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        0x180 - ((self.angle(plane, tile_index) & 0xFF) as i32);
                                }
                            }
                            _ => {
                                let column = 15 - (x_pos & 15);
                                let mask = self.floor_height(plane, column as usize, tile_index);
                                if mask < 0x40 {
                                    sensor.ypos =
                                        15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - (0x180 - ((self.angle(plane, tile_index) & 0xFF) as i32));
                                }
                            }
                        }
                    }
                    if sensor.collided {
                        if sensor.angle < 0 {
                            sensor.angle += 0x100;
                        }
                        if sensor.angle >= 0x100 {
                            sensor.angle -= 0x100;
                        }
                        if (sensor.angle - angle).abs() <= 0x20 {
                            if (sensor.ypos - start_y).abs() > self.collision_tolerance {
                                sensor.ypos = start_y.wrapping_shl(16);
                                sensor.collided = false;
                            }
                        } else {
                            sensor.ypos = start_y.wrapping_shl(16);
                            sensor.collided = false;
                            sensor.angle = angle;
                            step = 16 * 3;
                        }
                    }
                }
            }
            step += 16;
        }
        sensor
    }

    /// `FindRWallPosition`.
    pub fn find_rwall_position(
        &self,
        plane: usize,
        mut sensor: CollisionSensor,
        start_x: i32,
    ) -> CollisionSensor {
        let angle = sensor.angle;
        let mut step = 0;
        while step < 16 * 3 {
            if !sensor.collided {
                let x_pos = (sensor.xpos >> 16).wrapping_add(16).wrapping_sub(step);
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let y_pos = sensor.ypos >> 16;
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                if x_pos > -1 && y_pos > -1 {
                    let layout_chunk = self.layout.chunk(chunk_x, chunk_y);
                    let tile = self.chunk_tile(layout_chunk, tile_x, tile_y);
                    let tile_index = usize::from(tile.tile_index);
                    if Self::tile_flag(&tile, plane) < SOLID_NONE {
                        let row = usize::try_from(y_pos & 15).unwrap_or(0);
                        match tile.direction {
                            FLIP_NONE => {
                                let mask = self.right_wall_height(plane, row, tile_index);
                                if mask > -0x40 {
                                    sensor.xpos = i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle =
                                        ((self.angle(plane, tile_index) >> 16) & 0xFF) as i32;
                                }
                            }
                            FLIP_X => {
                                let mask = self.left_wall_height(plane, row, tile_index);
                                if mask < 0x40 {
                                    sensor.xpos =
                                        15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - ((self.angle(plane, tile_index) >> 8) & 0xFF) as i32;
                                }
                            }
                            FLIP_Y => {
                                let row = 15 - (y_pos & 15);
                                let mask = self.right_wall_height(plane, row as usize, tile_index);
                                if mask > -0x40 {
                                    sensor.xpos = i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x180
                                        - ((self.angle(plane, tile_index) >> 16) & 0xFF) as i32;
                                }
                            }
                            _ => {
                                let row = 15 - (y_pos & 15);
                                let mask = self.left_wall_height(plane, row as usize, tile_index);
                                if mask < 0x40 {
                                    sensor.xpos =
                                        15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4);
                                    sensor.collided = true;
                                    sensor.angle = 0x100
                                        - (0x180
                                            - ((self.angle(plane, tile_index) >> 8) & 0xFF) as i32);
                                }
                            }
                        }
                    }
                    if sensor.collided {
                        if sensor.angle < 0 {
                            sensor.angle += 0x100;
                        }
                        if sensor.angle >= 0x100 {
                            sensor.angle -= 0x100;
                        }
                        if (sensor.angle - angle).abs() > 0x20 {
                            sensor.xpos = start_x.wrapping_shl(16);
                            sensor.collided = false;
                            sensor.angle = angle;
                            step = 16 * 3;
                        } else if (sensor.xpos - start_x).abs() > self.collision_tolerance {
                            sensor.xpos = start_x.wrapping_shl(16);
                            sensor.collided = false;
                        }
                    }
                }
            }
            step += 16;
        }
        sensor
    }

    fn in_bounds(&self, x_pos: i32, y_pos: i32) -> bool {
        x_pos > 0 && x_pos < self.layout.width_px() && y_pos > 0 && y_pos < self.layout.height_px()
    }

    fn object_chunk(&self, x_pos: i32, y_pos: i32) -> (u16, Tile128, usize) {
        let chunk_x = x_pos >> 7;
        let tile_x = (x_pos & 0x7F) >> 4;
        let chunk_y = y_pos >> 7;
        let tile_y = (y_pos & 0x7F) >> 4;
        let layout_chunk = self.layout.chunk(chunk_x, chunk_y);
        let tile = self.chunk_tile(layout_chunk, tile_x, tile_y);
        let tile_index = usize::from(tile.tile_index);
        (layout_chunk, tile, tile_index)
    }

    /// `ObjectFloorCollision(xOffset, yOffset, cPath)`.
    pub fn object_floor_collision(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16).wrapping_add(y_offset);
        if !self.in_bounds(x_pos, y_pos) {
            return false;
        }
        let chunk_y = y_pos >> 7;
        let tile_y = (y_pos & 0x7F) >> 4;
        let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
        if Self::tile_flag(&tile, c_path) == SOLID_LRB
            || Self::tile_flag(&tile, c_path) == SOLID_NONE
        {
            return false;
        }
        let column = (x_pos & 15) as usize;
        let new_y = match tile.direction {
            FLIP_NONE => {
                let mask = self.floor_height(c_path, column, tile_index);
                if (y_pos & 15) <= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            FLIP_X => {
                let mask = self.floor_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                if (y_pos & 15) <= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            FLIP_Y => {
                let mask = self.roof_height(c_path, column, tile_index);
                if (y_pos & 15) <= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            _ => {
                let mask = self.roof_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                if (y_pos & 15) <= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
        };
        if let Some(entity) = store.get_mut(slot) {
            entity.ypos = (new_y.wrapping_sub(y_offset)).wrapping_shl(16);
        }
        true
    }

    /// `ObjectLWallCollision(xOffset, yOffset, cPath)`.
    pub fn object_lwall_collision(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16).wrapping_add(y_offset);
        if !self.in_bounds(x_pos, y_pos) {
            return false;
        }
        let chunk_x = x_pos >> 7;
        let tile_x = (x_pos & 0x7F) >> 4;
        let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
        if Self::tile_flag(&tile, c_path) == SOLID_TOP
            || Self::tile_flag(&tile, c_path) >= SOLID_NONE
        {
            return false;
        }
        let row = (y_pos & 15) as usize;
        let new_x = match tile.direction {
            FLIP_NONE => {
                let mask = self.left_wall_height(c_path, row, tile_index);
                if (x_pos & 15) <= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            FLIP_X => {
                let mask = self.right_wall_height(c_path, row, tile_index);
                if (x_pos & 15) <= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            FLIP_Y => {
                let mask = self.left_wall_height(c_path, 15 - (y_pos & 15) as usize, tile_index);
                if (x_pos & 15) <= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            _ => {
                let mask = self.right_wall_height(c_path, 15 - (y_pos & 15) as usize, tile_index);
                if (x_pos & 15) <= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
        };
        if let Some(entity) = store.get_mut(slot) {
            entity.xpos = (new_x.wrapping_sub(x_offset)).wrapping_shl(16);
        }
        true
    }

    /// `ObjectRoofCollision(xOffset, yOffset, cPath)`.
    pub fn object_roof_collision(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16).wrapping_add(y_offset);
        if !self.in_bounds(x_pos, y_pos) {
            return false;
        }
        let chunk_y = y_pos >> 7;
        let tile_y = (y_pos & 0x7F) >> 4;
        let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
        if Self::tile_flag(&tile, c_path) == SOLID_TOP
            || Self::tile_flag(&tile, c_path) >= SOLID_NONE
        {
            return false;
        }
        let column = (x_pos & 15) as usize;
        let new_y = match tile.direction {
            FLIP_NONE => {
                let mask = self.roof_height(c_path, column, tile_index);
                if (y_pos & 15) >= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            FLIP_X => {
                let mask = self.roof_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                if (y_pos & 15) >= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            FLIP_Y => {
                let mask = self.floor_height(c_path, column, tile_index);
                if (y_pos & 15) >= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
            _ => {
                let mask = self.floor_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                if (y_pos & 15) >= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4)
            }
        };
        if let Some(entity) = store.get_mut(slot) {
            entity.ypos = (new_y.wrapping_sub(y_offset)).wrapping_shl(16);
        }
        true
    }

    /// `ObjectRWallCollision(xOffset, yOffset, cPath)`.
    pub fn object_rwall_collision(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16).wrapping_add(y_offset);
        if !self.in_bounds(x_pos, y_pos) {
            return false;
        }
        let chunk_x = x_pos >> 7;
        let tile_x = (x_pos & 0x7F) >> 4;
        let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
        if Self::tile_flag(&tile, c_path) == SOLID_TOP
            || Self::tile_flag(&tile, c_path) >= SOLID_NONE
        {
            return false;
        }
        let row = (y_pos & 15) as usize;
        let new_x = match tile.direction {
            FLIP_NONE => {
                let mask = self.right_wall_height(c_path, row, tile_index);
                if (x_pos & 15) >= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            FLIP_X => {
                let mask = self.left_wall_height(c_path, row, tile_index);
                if (x_pos & 15) >= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            FLIP_Y => {
                let mask = self.right_wall_height(c_path, 15 - (y_pos & 15) as usize, tile_index);
                if (x_pos & 15) >= i32::from(mask) {
                    return false;
                }
                i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
            _ => {
                let mask = self.left_wall_height(c_path, 15 - (y_pos & 15) as usize, tile_index);
                if (x_pos & 15) >= 15 - i32::from(mask) {
                    return false;
                }
                15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4)
            }
        };
        if let Some(entity) = store.get_mut(slot) {
            entity.xpos = (new_x.wrapping_sub(x_offset)).wrapping_shl(16);
        }
        true
    }

    /// `ObjectFloorGrip(xOffset, yOffset, cPath)`.
    pub fn object_floor_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16) + x_offset;
        let start_y = (entity.ypos >> 16).wrapping_add(y_offset);
        let mut y_pos = start_y - 16;
        let mut result = false;
        for _ in 0..3 {
            if self.in_bounds(x_pos, y_pos) && !result {
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
                if Self::tile_flag(&tile, c_path) != SOLID_LRB
                    && Self::tile_flag(&tile, c_path) != SOLID_NONE
                {
                    let column = (x_pos & 15) as usize;
                    let found = match tile.direction {
                        FLIP_NONE => {
                            let mask = self.floor_height(c_path, column, tile_index);
                            (mask < 64).then_some(i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        FLIP_X => {
                            let mask =
                                self.floor_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                            (mask < 64).then_some(i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        FLIP_Y => {
                            let mask = self.roof_height(c_path, column, tile_index);
                            (mask > -64)
                                .then_some(15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        _ => {
                            let mask =
                                self.roof_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                            (mask > -64)
                                .then_some(15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                    };
                    if let Some(new_y) = found {
                        if let Some(entity) = store.get_mut(slot) {
                            entity.ypos = new_y << 16;
                        }
                        result = true;
                    }
                }
            }
            y_pos += 16;
        }
        if result {
            let entity_y = store.get_or_blank(slot).ypos >> 16;
            if (entity_y.wrapping_sub(start_y)).wrapping_abs() < 16 {
                if let Some(entity) = store.get_mut(slot) {
                    entity.ypos = (entity_y.wrapping_sub(y_offset)).wrapping_shl(16);
                }
                return result;
            }
            if let Some(entity) = store.get_mut(slot) {
                entity.ypos = (start_y.wrapping_sub(y_offset)).wrapping_shl(16);
            }
            result = false;
        }
        result
    }

    /// `ObjectLWallGrip(xOffset, yOffset, cPath)`.
    pub fn object_lwall_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let start_x = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16) + y_offset;
        let mut x_pos = start_x - 16;
        let mut result = false;
        for _ in 0..3 {
            if self.in_bounds(x_pos, y_pos) && !result {
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
                if Self::tile_flag(&tile, c_path) < SOLID_NONE {
                    let row = (y_pos & 15) as usize;
                    let found = match tile.direction {
                        FLIP_NONE => {
                            let mask = self.left_wall_height(c_path, row, tile_index);
                            (mask < 64).then_some(i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        FLIP_X => {
                            let mask = self.right_wall_height(c_path, row, tile_index);
                            (mask > -64)
                                .then_some(15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        FLIP_Y => {
                            let mask = self.left_wall_height(
                                c_path,
                                15 - (y_pos & 15) as usize,
                                tile_index,
                            );
                            (mask < 64).then_some(i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        _ => {
                            let mask = self.right_wall_height(
                                c_path,
                                15 - (y_pos & 15) as usize,
                                tile_index,
                            );
                            (mask > -64)
                                .then_some(15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                    };
                    if let Some(new_x) = found {
                        if let Some(entity) = store.get_mut(slot) {
                            entity.xpos = new_x << 16;
                        }
                        result = true;
                    }
                }
            }
            x_pos += 16;
        }
        if result {
            let entity_x = store.get_or_blank(slot).xpos >> 16;
            if (entity_x.wrapping_sub(start_x)).wrapping_abs() < 16 {
                if let Some(entity) = store.get_mut(slot) {
                    entity.xpos = (entity_x.wrapping_sub(x_offset)).wrapping_shl(16);
                }
                return result;
            }
            if let Some(entity) = store.get_mut(slot) {
                entity.xpos = (start_x.wrapping_sub(x_offset)).wrapping_shl(16);
            }
            result = false;
        }
        result
    }

    /// `ObjectRoofGrip(xOffset, yOffset, cPath)`.
    pub fn object_roof_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let x_pos = (entity.xpos >> 16) + x_offset;
        let start_y = (entity.ypos >> 16).wrapping_add(y_offset);
        let mut y_pos = start_y + 16;
        let mut result = false;
        for _ in 0..3 {
            if self.in_bounds(x_pos, y_pos) && !result {
                let chunk_y = y_pos >> 7;
                let tile_y = (y_pos & 0x7F) >> 4;
                let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
                if Self::tile_flag(&tile, c_path) < SOLID_NONE {
                    let column = (x_pos & 15) as usize;
                    let found = match tile.direction {
                        FLIP_NONE => {
                            let mask = self.roof_height(c_path, column, tile_index);
                            (mask > -64).then_some(i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        FLIP_X => {
                            let mask =
                                self.roof_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                            (mask > -64).then_some(i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        FLIP_Y => {
                            let mask = self.floor_height(c_path, column, tile_index);
                            (mask < 64)
                                .then_some(15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                        _ => {
                            let mask =
                                self.floor_height(c_path, 15 - (x_pos & 15) as usize, tile_index);
                            (mask < 64)
                                .then_some(15 - i32::from(mask) + (chunk_y << 7) + (tile_y << 4))
                        }
                    };
                    if let Some(new_y) = found {
                        if let Some(entity) = store.get_mut(slot) {
                            entity.ypos = new_y << 16;
                        }
                        result = true;
                    }
                }
            }
            y_pos -= 16;
        }
        if result {
            let entity_y = store.get_or_blank(slot).ypos >> 16;
            if (entity_y.wrapping_sub(start_y)).wrapping_abs() < 16 {
                if let Some(entity) = store.get_mut(slot) {
                    entity.ypos = (entity_y.wrapping_sub(y_offset)).wrapping_shl(16);
                }
                return result;
            }
            if let Some(entity) = store.get_mut(slot) {
                entity.ypos = (start_y.wrapping_sub(y_offset)).wrapping_shl(16);
            }
            result = false;
        }
        result
    }

    /// `ObjectRWallGrip(xOffset, yOffset, cPath)`.
    pub fn object_rwall_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let start_x = (entity.xpos >> 16).wrapping_add(x_offset);
        let y_pos = (entity.ypos >> 16) + y_offset;
        let mut x_pos = start_x + 16;
        let mut result = false;
        for _ in 0..3 {
            if self.in_bounds(x_pos, y_pos) && !result {
                let chunk_x = x_pos >> 7;
                let tile_x = (x_pos & 0x7F) >> 4;
                let (_layout_chunk, tile, tile_index) = self.object_chunk(x_pos, y_pos);
                if Self::tile_flag(&tile, c_path) < SOLID_NONE {
                    let row = (y_pos & 15) as usize;
                    let found = match tile.direction {
                        FLIP_NONE => {
                            let mask = self.right_wall_height(c_path, row, tile_index);
                            (mask > -64).then_some(i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        FLIP_X => {
                            let mask = self.left_wall_height(c_path, row, tile_index);
                            (mask < 64)
                                .then_some(15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        FLIP_Y => {
                            let mask = self.right_wall_height(
                                c_path,
                                15 - (y_pos & 15) as usize,
                                tile_index,
                            );
                            (mask > -64).then_some(i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                        _ => {
                            let mask = self.left_wall_height(
                                c_path,
                                15 - (y_pos & 15) as usize,
                                tile_index,
                            );
                            (mask < 64)
                                .then_some(15 - i32::from(mask) + (chunk_x << 7) + (tile_x << 4))
                        }
                    };
                    if let Some(new_x) = found {
                        if let Some(entity) = store.get_mut(slot) {
                            entity.xpos = new_x << 16;
                        }
                        result = true;
                    }
                }
            }
            x_pos -= 16;
        }
        if result {
            let entity_x = store.get_or_blank(slot).xpos >> 16;
            if (entity_x.wrapping_sub(start_x)).wrapping_abs() < 16 {
                if let Some(entity) = store.get_mut(slot) {
                    entity.xpos = (entity_x.wrapping_sub(x_offset)).wrapping_shl(16);
                }
                return result;
            }
            if let Some(entity) = store.get_mut(slot) {
                entity.xpos = (start_x.wrapping_sub(x_offset)).wrapping_shl(16);
            }
            result = false;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn entity_grip_side(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
        left: bool,
        groups: &[TypeGroupList],
    ) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        let m_block_id = entity.values[44];
        let x_offset_screen = if left {
            x_offset.wrapping_sub(16)
        } else {
            x_offset.wrapping_add(16)
        };
        let x_pos = (entity.xpos >> 16).wrapping_add(x_offset_screen);
        let y_pos = (entity.ypos >> 16).wrapping_add(y_offset);
        if m_block_id > 0 {
            let entity_refs = groups
                .get(usize::try_from(m_block_id).unwrap_or(usize::MAX))
                .map(|group| group.entity_refs.clone())
                .unwrap_or_default();
            for entity_ref in entity_refs {
                let Ok(other_slot) = usize::try_from(entity_ref) else {
                    continue;
                };
                let Some(other) = store.get(other_slot).copied() else {
                    continue;
                };
                let other_x = other.xpos >> 16;
                let other_y = other.ypos >> 16;
                let row_overlap = (other_y - 16) <= y_pos && y_pos <= (other_y + 16);
                let hits = if left {
                    [
                        (other_x - 16) <= x_pos && x_pos <= (other_x + 16) && row_overlap,
                        (other_x - 16) <= (x_pos + 16)
                            && (x_pos + 16) <= (other_x + 16)
                            && row_overlap,
                        other_x <= (x_pos + 32) && (x_pos + 32) <= (other_x + 16) && row_overlap,
                    ]
                } else {
                    [
                        (other_x - 16) <= x_pos && x_pos <= (other_x + 16) && row_overlap,
                        (other_x - 16) <= (x_pos + 16)
                            && (x_pos - 16) <= (other_x + 16)
                            && row_overlap,
                        other_x <= (x_pos - 32) && (x_pos - 32) <= (other_x + 16) && row_overlap,
                    ]
                };
                let mut check = 0;
                for hit in hits {
                    if !hit {
                        continue;
                    }
                    let new_x = if left {
                        other
                            .xpos
                            .wrapping_sub(x_offset.wrapping_shl(16))
                            .wrapping_sub(0x100000)
                    } else {
                        other
                            .xpos
                            .wrapping_add(16i32.wrapping_sub(x_offset).wrapping_shl(16))
                    };
                    if let Some(entity) = store.get_mut(slot) {
                        entity.xpos = new_x;
                    }
                    if other.values[0] == 0 {
                        check = 2;
                    } else if check != 2 {
                        check = 1;
                    }
                }
                if check != 0 {
                    return true;
                }
            }
        }
        if left {
            self.object_lwall_grip(store, slot, x_offset, y_offset, c_path)
        } else {
            self.object_rwall_grip(store, slot, x_offset, y_offset, c_path)
        }
    }

    /// `ObjectLEntityGrip(xOffset, yOffset, cPath)` (rev03).
    pub fn object_lentity_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
        groups: &[TypeGroupList],
    ) -> bool {
        self.entity_grip_side(store, slot, x_offset, y_offset, c_path, true, groups)
    }

    /// `ObjectREntityGrip(xOffset, yOffset, cPath)` (rev03).
    pub fn object_rentity_grip(
        &self,
        store: &mut EntityStore,
        slot: usize,
        x_offset: i32,
        y_offset: i32,
        c_path: usize,
        groups: &[TypeGroupList],
    ) -> bool {
        self.entity_grip_side(store, slot, x_offset, y_offset, c_path, false, groups)
    }

    /// `TouchCollision`: sets `checkResult` from the overlap of two boxes.
    #[allow(clippy::too_many_arguments)]
    pub fn touch_collision(
        &self,
        store: &mut EntityStore,
        this_slot: usize,
        mut this_left: i32,
        mut this_top: i32,
        mut this_right: i32,
        mut this_bottom: i32,
        other_slot: usize,
        mut other_left: i32,
        mut other_top: i32,
        mut other_right: i32,
        mut other_bottom: i32,
        resolve_hitbox: &dyn Fn(usize, &Entity) -> Hitbox,
    ) -> bool {
        let Some(this_entity) = store.get(this_slot) else {
            return false;
        };
        let Some(other_entity) = store.get(other_slot) else {
            return false;
        };
        let this_hitbox = resolve_hitbox(this_slot, this_entity);
        let other_hitbox = resolve_hitbox(other_slot, other_entity);
        if this_left == C_BOX {
            this_left = i32::from(this_hitbox.left[0]);
        }
        if this_top == C_BOX {
            this_top = i32::from(this_hitbox.top[0]);
        }
        if this_right == C_BOX {
            this_right = i32::from(this_hitbox.right[0]);
        }
        if this_bottom == C_BOX {
            this_bottom = i32::from(this_hitbox.bottom[0]);
        }
        if other_left == C_BOX {
            other_left = i32::from(other_hitbox.left[0]);
        }
        if other_top == C_BOX {
            other_top = i32::from(other_hitbox.top[0]);
        }
        if other_right == C_BOX {
            other_right = i32::from(other_hitbox.right[0]);
        }
        if other_bottom == C_BOX {
            other_bottom = i32::from(other_hitbox.bottom[0]);
        }
        let this_x = this_entity.xpos >> 16;
        let this_y = this_entity.ypos >> 16;
        let other_x = other_entity.xpos >> 16;
        let other_y = other_entity.ypos >> 16;
        this_left += this_x;
        this_top += this_y;
        this_right += this_x;
        this_bottom += this_y;
        other_left += other_x;
        other_top += other_y;
        other_right += other_x;
        other_bottom += other_y;
        other_right > this_left
            && other_left < this_right
            && other_bottom > this_top
            && other_top < this_bottom
    }

    /// `BoxCollision`: standard solid interaction between two entities.
    ///
    /// Ported from `BoxCollision` in `RSDKv4/Collision.cpp` with the seven temporary sensors
    /// kept local instead of the upstream globals. All position arithmetic wraps exactly like
    /// the C implementation (signed overflow must not panic).
    #[allow(clippy::too_many_arguments)]
    pub fn box_collision(
        &self,
        store: &mut EntityStore,
        this_slot: usize,
        mut this_left: i32,
        mut this_top: i32,
        mut this_right: i32,
        mut this_bottom: i32,
        other_slot: usize,
        mut other_left: i32,
        mut other_top: i32,
        mut other_right: i32,
        mut other_bottom: i32,
        resolve_hitbox: &dyn Fn(usize, &Entity) -> Hitbox,
    ) -> i32 {
        let (Some(this_entity), Some(other_entity)) = (
            store.get(this_slot).copied(),
            store.get(other_slot).copied(),
        ) else {
            return 0;
        };
        let this_hitbox = resolve_hitbox(this_slot, &this_entity);
        let other_hitbox = resolve_hitbox(other_slot, &other_entity);
        if this_left == C_BOX {
            this_left = i32::from(this_hitbox.left[0]);
        }
        if this_top == C_BOX {
            this_top = i32::from(this_hitbox.top[0]);
        }
        if this_right == C_BOX {
            this_right = i32::from(this_hitbox.right[0]);
        }
        if this_bottom == C_BOX {
            this_bottom = i32::from(this_hitbox.bottom[0]);
        }
        if other_left == C_BOX {
            other_left = i32::from(other_hitbox.left[0]);
        }
        if other_top == C_BOX {
            other_top = i32::from(other_hitbox.top[0]);
        }
        if other_right == C_BOX {
            other_right = i32::from(other_hitbox.right[0]);
        }
        if other_bottom == C_BOX {
            other_bottom = i32::from(other_hitbox.bottom[0]);
        }
        let _ = other_top;
        let this_x = this_entity.xpos;
        let this_y = this_entity.ypos;
        this_left = this_left.wrapping_add(this_x >> 16).wrapping_shl(16);
        this_top = this_top.wrapping_add(this_y >> 16).wrapping_shl(16);
        this_right = this_right.wrapping_add(this_x >> 16).wrapping_shl(16);
        this_bottom = this_bottom.wrapping_add(this_y >> 16).wrapping_shl(16);
        other_left = other_left.wrapping_shl(16);
        other_top = other_top.wrapping_shl(16);
        other_right = other_right.wrapping_shl(16);
        other_bottom = other_bottom.wrapping_shl(16);
        let rx = (other_entity.xpos >> 16).wrapping_shl(16);
        let ry = (other_entity.ypos >> 16).wrapping_shl(16);
        let x_dif = if this_entity.xpos > other_entity.xpos {
            this_left.wrapping_sub(other_entity.xpos)
        } else {
            other_entity.xpos.wrapping_sub(this_right)
        };
        let y_dif = if this_entity.ypos <= other_entity.ypos {
            other_entity.ypos.wrapping_sub(this_bottom)
        } else {
            this_top.wrapping_sub(other_entity.ypos)
        };
        if x_dif <= y_dif
            && (other_entity.xvel.wrapping_abs() >> 1) <= other_entity.yvel.wrapping_abs()
        {
            let mut sensors = [CollisionSensor::default(); 7];
            // Floor probes.
            set_ground_sensors(&mut sensors, rx, ry, other_left, other_right, other_bottom);
            if box_ground_collision(
                store,
                other_slot,
                &other_entity,
                &mut sensors,
                [this_left, this_top, this_right],
                other_bottom,
            ) {
                return 1;
            }
            // Ceiling probes.
            set_ceiling_sensors(&mut sensors, rx, ry, other_left, other_right, other_top);
            if box_ceiling_collision(
                store,
                other_slot,
                &other_entity,
                &mut sensors,
                [this_left, this_top, this_right, this_bottom],
                other_top,
            ) {
                return 4;
            }
            // Push left, then right.
            if box_push_left(
                store,
                other_slot,
                &other_entity,
                rx,
                ry,
                other_left,
                other_right,
                other_bottom,
                other_top,
                [this_left, this_top, this_right, this_bottom],
            ) {
                return 2;
            }
            if box_push_right(
                store,
                other_slot,
                &other_entity,
                rx,
                ry,
                other_left,
                other_right,
                other_bottom,
                other_top,
                [this_left, this_top, this_right, this_bottom],
            ) {
                return 3;
            }
            0
        } else {
            let mut sensors = [CollisionSensor::default(); 7];
            // Push left, then right.
            if box_push_left(
                store,
                other_slot,
                &other_entity,
                rx,
                ry,
                other_left,
                other_right,
                other_bottom,
                other_top,
                [this_left, this_top, this_right, this_bottom],
            ) {
                return 2;
            }
            if box_push_right(
                store,
                other_slot,
                &other_entity,
                rx,
                ry,
                other_left,
                other_right,
                other_bottom,
                other_top,
                [this_left, this_top, this_right, this_bottom],
            ) {
                return 3;
            }
            // Floor then ceiling fallback.
            set_ground_sensors(&mut sensors, rx, ry, other_left, other_right, other_bottom);
            if box_ground_collision(
                store,
                other_slot,
                &other_entity,
                &mut sensors,
                [this_left, this_top, this_right],
                other_bottom,
            ) {
                return 1;
            }
            set_ceiling_sensors(&mut sensors, rx, ry, other_left, other_right, other_top);
            if box_ceiling_collision(
                store,
                other_slot,
                &other_entity,
                &mut sensors,
                [this_left, this_top, this_right, this_bottom],
                other_top,
            ) {
                return 4;
            }
            0
        }
    }

    /// `PlatformCollision`: one-way solid-top interaction used by moving platforms.
    ///
    /// Ported from `PlatformCollision` in `RSDKv4/Collision.cpp`; returns upstream's
    /// `checkResult` (0/1).
    #[allow(clippy::too_many_arguments)]
    pub fn platform_collision(
        &self,
        store: &mut EntityStore,
        this_slot: usize,
        mut this_left: i32,
        mut this_top: i32,
        mut this_right: i32,
        mut this_bottom: i32,
        other_slot: usize,
        mut other_left: i32,
        mut other_top: i32,
        mut other_right: i32,
        mut other_bottom: i32,
        resolve_hitbox: &dyn Fn(usize, &Entity) -> Hitbox,
    ) -> i32 {
        let (Some(this_entity), Some(other_entity)) = (
            store.get(this_slot).copied(),
            store.get(other_slot).copied(),
        ) else {
            return 0;
        };
        let this_hitbox = resolve_hitbox(this_slot, &this_entity);
        let other_hitbox = resolve_hitbox(other_slot, &other_entity);
        if this_left == C_BOX {
            this_left = i32::from(this_hitbox.left[0]);
        }
        if this_top == C_BOX {
            this_top = i32::from(this_hitbox.top[0]);
        }
        if this_right == C_BOX {
            this_right = i32::from(this_hitbox.right[0]);
        }
        if this_bottom == C_BOX {
            this_bottom = i32::from(this_hitbox.bottom[0]);
        }
        if other_left == C_BOX {
            other_left = i32::from(other_hitbox.left[0]);
        }
        if other_top == C_BOX {
            other_top = i32::from(other_hitbox.top[0]);
        }
        if other_right == C_BOX {
            other_right = i32::from(other_hitbox.right[0]);
        }
        if other_bottom == C_BOX {
            other_bottom = i32::from(other_hitbox.bottom[0]);
        }
        // Upstream reads `otherTop` while assigning the hitbox but never uses it: the platform
        // routine only needs the bottom edge. Keep the assignment for parity.
        let _ = other_top;
        let this_x = this_entity.xpos;
        let this_y = this_entity.ypos;
        this_left = this_left.wrapping_add(this_x >> 16).wrapping_shl(16);
        this_top = this_top.wrapping_add(this_y >> 16).wrapping_shl(16);
        this_right = this_right.wrapping_add(this_x >> 16).wrapping_shl(16);
        this_bottom = this_bottom.wrapping_add(this_y >> 16).wrapping_shl(16);
        let rx = (other_entity.xpos >> 16).wrapping_shl(16);
        let ry = (other_entity.ypos >> 16).wrapping_shl(16);
        let mut sensors = [CollisionSensor::default(); 5];
        sensors[0].xpos = rx.wrapping_add(other_left.wrapping_shl(16));
        sensors[1].xpos = rx;
        sensors[2].xpos = rx.wrapping_add(other_right.wrapping_shl(16));
        sensors[3].xpos = (rx.wrapping_add(sensors[0].xpos)) >> 1;
        sensors[4].xpos = (sensors[2].xpos.wrapping_add(rx)) >> 1;
        sensors[0].ypos = other_bottom.wrapping_shl(16).wrapping_add(ry);
        for index in 0..5 {
            if this_left < sensors[index].xpos
                && this_right > sensors[index].xpos
                && this_top.wrapping_sub(1) <= sensors[0].ypos
                && this_bottom > sensors[0].ypos
                && other_entity.yvel >= 0
            {
                sensors[index].collided = true;
                if let Some(other) = store.get_mut(other_slot) {
                    other.floor_sensors[index] = 1;
                }
            }
        }
        if sensors[0].collided || sensors[1].collided || sensors[2].collided {
            if let Some(other) = store.get_mut(other_slot) {
                if other.gravity == 0
                    && (other.collision_mode == CMODE_RWALL || other.collision_mode == CMODE_LWALL)
                {
                    other.xvel = 0;
                    other.speed = 0;
                }
                other.ypos = this_top.wrapping_sub(other_bottom.wrapping_shl(16));
                other.gravity = 0;
                other.yvel = 0;
                other.angle = 0;
                other.rotation = 0;
                other.control_lock = 0;
            }
            return 1;
        }
        0
    }

    /// Deterministic movement: fixed-point integration plus a three-sensor floor probe.
    ///
    /// This is a documented simplification of upstream
    /// `FUNC_PROCESSOBJECTMOVEMENT`/`ProcessTileCollisions`: path grip, wall pushing and ceiling
    /// handling are not implemented. Returns whether a floor was found.
    pub fn process_object_movement(&mut self, store: &mut EntityStore, slot: usize) -> bool {
        let Some(entity) = store.get(slot) else {
            return false;
        };
        if entity.tile_collisions == 0 {
            if let Some(entity) = store.get_mut(slot) {
                entity.xpos = entity.xpos.wrapping_add(entity.xvel);
                entity.ypos = entity.ypos.wrapping_add(entity.yvel);
            }
            return false;
        }
        let plane = usize::from(entity.collision_plane);
        let angle = entity.angle;
        let speed = entity.speed;
        let mut tolerance = 15;
        if speed < 0x60000 {
            tolerance = if (angle as i8) == 0 { 8 } else { 15 };
        }
        self.collision_tolerance = tolerance;
        let previous = *store.get_or_blank(slot);
        if let Some(entity) = store.get_mut(slot) {
            entity.xpos = entity.xpos.wrapping_add(entity.xvel);
            entity.ypos = entity.ypos.wrapping_add(entity.yvel);
            entity.floor_sensors = [0; crate::entity::FLOOR_SENSOR_COUNT];
        }
        let moving_down = previous.yvel >= 0;
        let mut on_ground = false;
        if moving_down {
            let mut sensed: [CollisionSensor; 3] = [CollisionSensor::default(); 3];
            for (index, offset) in [-8, 0, 8].into_iter().enumerate() {
                let x = previous
                    .xpos
                    .wrapping_add(offset << 16)
                    .wrapping_add(previous.xvel);
                let y = previous
                    .ypos
                    .wrapping_add(16 << 16)
                    .wrapping_add(previous.yvel);
                let start_y = (y >> 16) - 16;
                sensed[index] = self.find_floor_position(
                    plane,
                    CollisionSensor::new(x, y, previous.angle),
                    start_y,
                );
            }
            if sensed[1].collided {
                on_ground = true;
                if let Some(entity) = store.get_mut(slot) {
                    entity.ypos = (sensed[1].ypos - 16) << 16;
                    entity.yvel = 0;
                    entity.angle = sensed[1].angle;
                    entity.floor_sensors[0] = u8::from(sensed[0].collided);
                    entity.floor_sensors[1] = 1;
                    entity.floor_sensors[2] = u8::from(sensed[2].collided);
                }
            }
        }
        on_ground
    }
}

/// Sets up the five floor probes of `BoxCollision` and clears their collision flags.
fn set_ground_sensors(
    sensors: &mut [CollisionSensor; 7],
    rx: i32,
    ry: i32,
    other_left: i32,
    other_right: i32,
    other_bottom: i32,
) {
    sensors[0].collided = false;
    sensors[1].collided = false;
    sensors[2].collided = false;
    sensors[3].collided = false;
    sensors[4].collided = false;
    sensors[0].xpos = rx.wrapping_add(other_left).wrapping_add(0x20000);
    sensors[1].xpos = rx;
    sensors[2].xpos = rx.wrapping_add(other_right).wrapping_sub(0x20000);
    sensors[3].xpos = (sensors[0].xpos.wrapping_add(rx)) >> 1;
    sensors[4].xpos = (sensors[2].xpos.wrapping_add(rx)) >> 1;
    sensors[0].ypos = ry.wrapping_add(other_bottom);
}

/// Upstream floor branch: other entity lands on `this`. Returns upstream `checkResult == 1`.
fn box_ground_collision(
    store: &mut EntityStore,
    other_slot: usize,
    other: &Entity,
    sensors: &mut [CollisionSensor; 7],
    this_box: [i32; 3],
    other_bottom: i32,
) -> bool {
    let [this_left, this_top, this_right] = this_box;
    if other.yvel >= 0 {
        for index in 0..5 {
            if this_left < sensors[index].xpos
                && this_right > sensors[index].xpos
                && this_top <= sensors[0].ypos
                && this_top > other.ypos.wrapping_sub(other.yvel)
            {
                sensors[index].collided = true;
                if let Some(target) = store.get_mut(other_slot) {
                    target.floor_sensors[index] = 1;
                }
            }
        }
    }
    if sensors[0].collided || sensors[1].collided || sensors[2].collided {
        if let Some(target) = store.get_mut(other_slot) {
            if target.gravity == 0
                && (target.collision_mode == CMODE_RWALL || target.collision_mode == CMODE_LWALL)
            {
                target.xvel = 0;
                target.speed = 0;
            }
            target.ypos = this_top.wrapping_sub(other_bottom);
            target.gravity = 0;
            target.yvel = 0;
            target.angle = 0;
            target.rotation = 0;
            target.control_lock = 0;
        }
        return true;
    }
    false
}

/// Sets up the two ceiling probes of `BoxCollision` and clears their collision flags.
fn set_ceiling_sensors(
    sensors: &mut [CollisionSensor; 7],
    rx: i32,
    ry: i32,
    other_left: i32,
    other_right: i32,
    other_top: i32,
) {
    sensors[0].collided = false;
    sensors[1].collided = false;
    sensors[0].xpos = rx.wrapping_add(other_left).wrapping_add(0x20000);
    sensors[1].xpos = rx.wrapping_add(other_right).wrapping_sub(0x20000);
    sensors[0].ypos = ry.wrapping_add(other_top);
}

/// Upstream ceiling branch. Returns upstream `checkResult == 4`.
fn box_ceiling_collision(
    store: &mut EntityStore,
    other_slot: usize,
    other: &Entity,
    sensors: &mut [CollisionSensor; 7],
    this_box: [i32; 4],
    other_top: i32,
) -> bool {
    let [this_left, this_top, this_right, this_bottom] = this_box;
    let _ = this_top;
    for index in 0..2 {
        if this_left < sensors[1].xpos
            && this_right > sensors[0].xpos
            && this_bottom > sensors[0].ypos
            && this_bottom < other.ypos.wrapping_sub(other.yvel)
        {
            sensors[index].collided = true;
        }
    }
    if sensors[0].collided || sensors[1].collided {
        if let Some(target) = store.get_mut(other_slot) {
            if target.gravity == 1 {
                target.ypos = this_bottom.wrapping_sub(other_top);
            }
            if target.yvel <= 0 {
                target.yvel = 0;
            }
        }
        return true;
    }
    false
}

/// Upstream left-push branch. Returns upstream `checkResult == 2`.
#[allow(clippy::too_many_arguments)]
fn box_push_left(
    store: &mut EntityStore,
    other_slot: usize,
    other: &Entity,
    rx: i32,
    ry: i32,
    other_left: i32,
    other_right: i32,
    other_bottom: i32,
    other_top: i32,
    this_box: [i32; 4],
) -> bool {
    let [this_left, this_top, this_right, this_bottom] = this_box;
    let mut sensors = [CollisionSensor::default(); 2];
    sensors[0].xpos = rx.wrapping_add(other_right);
    sensors[0].ypos = ry.wrapping_add(other_top).wrapping_add(0x20000);
    sensors[1].ypos = ry.wrapping_add(other_bottom).wrapping_sub(0x20000);
    let _ = (other_left, this_right);
    for index in 0..2 {
        if this_left <= sensors[0].xpos
            && this_left > other.xpos.wrapping_sub(other.xvel)
            && this_top < sensors[1].ypos
            && this_bottom > sensors[0].ypos
        {
            sensors[index].collided = true;
        }
    }
    if sensors[0].collided || sensors[1].collided {
        if let Some(target) = store.get_mut(other_slot) {
            target.xpos = this_left.wrapping_sub(other_right);
            if target.xvel > 0 {
                if target.direction == FLIP_NONE {
                    target.pushing = 2;
                }
                target.xvel = 0;
                if target.collision_mode != 0 || target.left == 0 {
                    target.speed = 0;
                } else {
                    target.speed = -0x8000;
                }
            }
        }
        return true;
    }
    false
}

/// Upstream right-push branch. Returns upstream `checkResult == 3`.
#[allow(clippy::too_many_arguments)]
fn box_push_right(
    store: &mut EntityStore,
    other_slot: usize,
    other: &Entity,
    rx: i32,
    ry: i32,
    other_left: i32,
    other_right: i32,
    other_bottom: i32,
    other_top: i32,
    this_box: [i32; 4],
) -> bool {
    let [this_left, this_top, this_right, this_bottom] = this_box;
    let mut sensors = [CollisionSensor::default(); 2];
    sensors[0].xpos = rx.wrapping_add(other_left);
    sensors[0].ypos = ry.wrapping_add(other_top).wrapping_add(0x20000);
    sensors[1].ypos = ry.wrapping_add(other_bottom).wrapping_sub(0x20000);
    let _ = (other_right, this_left);
    for index in 0..2 {
        if this_right > sensors[0].xpos
            && this_right < other.xpos.wrapping_sub(other.xvel)
            && this_top < sensors[1].ypos
            && this_bottom > sensors[0].ypos
        {
            sensors[index].collided = true;
        }
    }
    if sensors[0].collided || sensors[1].collided {
        if let Some(target) = store.get_mut(other_slot) {
            target.xpos = this_right.wrapping_sub(other_left);
            if target.xvel < 0 {
                if target.direction == FLIP_X {
                    target.pushing = 2;
                }
                if target.xvel < -0x10000 {
                    target.xpos = target.xpos.wrapping_add(0x8000);
                }
                target.xvel = 0;
                if target.collision_mode != 0 || target.right == 0 {
                    target.speed = 0;
                } else {
                    target.speed = 0x8000;
                }
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityStore;
    use crate::stage::StageLayout;
    use retro_format_v4::collision::{
        COLLISION_FILE_BYTES, COLLISION_PLANE_COUNT, COLLISION_TILE_BYTES, COLLISION_TILE_COUNT,
    };
    use retro_format_v4::tiles::TILE_SHEET_128_ENTRY_COUNT;

    fn blank_masks() -> CollisionMasks {
        let mut bytes = Vec::with_capacity(COLLISION_FILE_BYTES);
        for _ in 0..COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT {
            // Ceiling bit set (harmless) with low nibble 3 == SOLID_NONE.
            bytes.push(0x33);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&[0u8; 8]);
            bytes.push(0xFF);
            bytes.push(0xFF);
        }
        debug_assert_eq!(bytes.len(), COLLISION_TILE_BYTES * COLLISION_TILE_COUNT * 2);
        CollisionMasks::from_bytes(&bytes).unwrap()
    }

    fn blank_tiles() -> TileSheet128 {
        TileSheet128 {
            entries: vec![
                Tile128 {
                    direction: 0,
                    visual_plane: 0,
                    tile_index: 0,
                    collision_flag_a: 0,
                    collision_flag_b: SOLID_NONE,
                };
                TILE_SHEET_128_ENTRY_COUNT
            ],
        }
    }

    fn solid_floor_masks() -> CollisionMasks {
        let mut bytes = Vec::with_capacity(COLLISION_FILE_BYTES);
        for _ in 0..COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT {
            // Non-ceiling flags (low nibble 0 == SOLID_ALL) with every floor sample at 8.
            bytes.push(0);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&[0x88u8; 8]);
            bytes.push(0xFF);
            bytes.push(0xFF);
        }
        CollisionMasks::from_bytes(&bytes).unwrap()
    }

    fn solid_tiles() -> TileSheet128 {
        TileSheet128 {
            entries: vec![
                Tile128 {
                    direction: 0,
                    visual_plane: 0,
                    tile_index: 0,
                    collision_flag_a: 0,
                    collision_flag_b: 0,
                };
                TILE_SHEET_128_ENTRY_COUNT
            ],
        }
    }

    fn layout() -> StageLayout {
        StageLayout {
            width: 4,
            height: 4,
            tiles: vec![0; 16],
        }
    }

    fn zero_hitbox_fn(_slot: usize, _entity: &Entity) -> Hitbox {
        Hitbox {
            left: [0; 8],
            top: [0; 8],
            right: [0; 8],
            bottom: [0; 8],
        }
    }

    #[test]
    fn solidity_is_selected_by_the_active_plane() {
        // Tile 0 (used through the chunk entry below): plane 0 says SOLID_NONE while plane 1
        // says SOLID_ALL. Tile 1 is the reverse. Only plane-aware solidity can pass both.
        // Entities sit at pixel (64, 64) and (192, 64), i.e. 16x16 tile (4, 4) of chunk 0/1,
        // so the chunk sheet entry is `(chunk << 6) + 4 + (4 << 3)`.
        const TILE_0_ENTRY: usize = 36;
        const TILE_1_ENTRY: usize = 100;
        let mut tiles = blank_tiles();
        tiles.entries[TILE_0_ENTRY] = Tile128 {
            direction: 0,
            visual_plane: 0,
            tile_index: 0,
            collision_flag_a: SOLID_NONE,
            collision_flag_b: SOLID_ALL,
        };
        tiles.entries[TILE_1_ENTRY] = Tile128 {
            direction: 0,
            visual_plane: 0,
            tile_index: 1,
            collision_flag_a: SOLID_ALL,
            collision_flag_b: SOLID_NONE,
        };
        let mut layout = layout();
        layout.tiles[1] = 1;
        // All collision masks have a floor sample at height 8.
        let mut bytes = Vec::with_capacity(COLLISION_FILE_BYTES);
        for _ in 0..COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT {
            bytes.push(0);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&[0x88u8; 8]);
            bytes.push(0xFF);
            bytes.push(0xFF);
        }
        let masks = CollisionMasks::from_bytes(&bytes).unwrap();
        let collision = SceneCollision::new(layout, tiles, masks);

        // Tile 0 at pixel (64, 64).
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 64 << 16, 64 << 16);
        assert!(
            !collision.object_floor_collision(&mut store, 0, 0, 15, 0),
            "plane 0 reads flag_a (SOLID_NONE)"
        );
        let before = store.get(0).unwrap().ypos;
        assert!(
            collision.object_floor_collision(&mut store, 0, 0, 15, 1),
            "plane 1 reads flag_b (SOLID_ALL)"
        );
        assert_ne!(store.get(0).unwrap().ypos, before);

        // Tile 1 at pixel (192, 64).
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 192 << 16, 64 << 16);
        assert!(
            collision.object_floor_collision(&mut store, 0, 0, 15, 0),
            "plane 0 reads flag_a (SOLID_ALL)"
        );
        let before = store.get(0).unwrap().ypos;
        assert!(
            !collision.object_floor_collision(&mut store, 0, 0, 15, 1),
            "plane 1 reads flag_b (SOLID_NONE)"
        );
        assert_eq!(store.get(0).unwrap().ypos, before);
    }

    #[test]
    fn box_collision_wraps_extreme_positions() {
        let collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        for (this_x, this_y, other_x, other_y) in [
            (i32::MIN, 0, i32::MAX, 0),
            (0, i32::MIN, 0, i32::MAX),
            (i32::MIN, i32::MIN, i32::MAX, i32::MAX),
            (i32::MAX, 0, i32::MIN, 0),
        ] {
            let mut store = EntityStore::new();
            store.reset_object_entity(0, 1, 0, this_x, this_y);
            store.reset_object_entity(1, 2, 0, other_x, other_y);
            let result = collision.box_collision(
                &mut store,
                0,
                -8,
                -8,
                8,
                8,
                1,
                -8,
                -8,
                8,
                8,
                &zero_hitbox_fn,
            );
            assert!(
                (0..=4).contains(&result),
                "box collision must wrap, not panic ({this_x}, {this_y}, {other_x}, {other_y})"
            );
        }
    }

    #[test]
    fn box_push_left_uses_the_second_sensor_y() {
        let collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        // Other box (-8, -1, 8, 1) makes sensors[0].ypos (101px) > sensors[1].ypos (99px),
        // the only shape where the per-index bug is observable.
        let other_box = [-8, -1, 8, 1];

        // this_top = 98 < 99 and this_bottom = 102 > 101: upstream pushes left.
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 120 << 16, 100 << 16);
        store.reset_object_entity(1, 2, 0, 100 << 16, 100 << 16);
        let result = collision.box_collision(
            &mut store,
            0,
            -12,
            -2,
            0,
            2,
            1,
            other_box[0],
            other_box[1],
            other_box[2],
            other_box[3],
            &zero_hitbox_fn,
        );
        assert_eq!(result, 2, "this_top/this_bottom overlap the 99..101 slab");
        assert_eq!(store.get(1).unwrap().xpos, 100 << 16);

        // this_top = 100 is inside (99, 101): upstream does not push, the buggy per-index
        // loop (which used sensors[0].ypos = 101 for index 0) would.
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 120 << 16, 100 << 16);
        store.reset_object_entity(1, 2, 0, 100 << 16, 100 << 16);
        let result = collision.box_collision(
            &mut store,
            0,
            -12,
            0,
            0,
            2,
            1,
            other_box[0],
            other_box[1],
            other_box[2],
            other_box[3],
            &zero_hitbox_fn,
        );
        assert_eq!(result, 0, "sensors[1].ypos gates the push");
        assert_eq!(store.get(1).unwrap().xpos, 100 << 16);
    }

    #[test]
    fn platform_collision_lands_on_top() {
        let collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        let mut store = EntityStore::new();
        // Slot 0 is the platform, slot 1 is the falling entity above it.
        store.reset_object_entity(0, 1, 0, 100 << 16, 100 << 16);
        store.reset_object_entity(1, 2, 0, 100 << 16, 90 << 16);
        store.get_mut(1).unwrap().yvel = 0x20000;
        let result = collision.platform_collision(
            &mut store,
            0,
            -8,
            -8,
            8,
            8,
            1,
            -8,
            -8,
            8,
            8,
            &zero_hitbox_fn,
        );
        assert_eq!(result, 1);
        // Platform top (92px) minus the falling entity's bottom offset (8px).
        assert_eq!(store.get(1).unwrap().ypos, 84 << 16);
        assert_eq!(store.get(1).unwrap().yvel, 0);
        assert_eq!(store.get(1).unwrap().floor_sensors[1], 1);
    }

    #[test]
    fn solid_none_floor_never_collides() {
        let collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 64 << 16, 64 << 16);
        assert!(!collision.object_floor_collision(&mut store, 0, 0, 16, 0));
    }

    #[test]
    fn solid_floor_collision_snaps_to_sample_height() {
        let collision = SceneCollision::new(layout(), solid_tiles(), solid_floor_masks());
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 64 << 16, 64 << 16);
        // Probe 15px below the entity centre, inside the same 16x16 tile; the surface is
        // 8px into the tile, so the entity snaps to 72 - 15 = 57.
        let hit = collision.object_floor_collision(&mut store, 0, 0, 15, 0);
        assert!(hit);
        assert_eq!(store.get(0).unwrap().ypos >> 16, 57);
    }

    #[test]
    fn floor_grip_reports_and_snaps() {
        let collision = SceneCollision::new(layout(), solid_tiles(), solid_floor_masks());
        let mut store = EntityStore::new();
        // Probe starts at 52 - 16 = 36; the floor sample at row 2 (32..47) is 8 above the
        // tile top, so the surface (40) is within 16px of the start position (52).
        store.reset_object_entity(0, 1, 0, 64 << 16, 52 << 16);
        let hit = collision.object_floor_grip(&mut store, 0, 0, 0, 0);
        assert!(hit);
        assert_eq!(store.get(0).unwrap().ypos >> 16, 40);
    }

    #[test]
    fn touch_collision_uses_boxes() {
        let collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 100 << 16, 100 << 16);
        store.reset_object_entity(1, 2, 0, 110 << 16, 100 << 16);
        let hitbox = |_: usize, _: &Entity| Hitbox {
            left: [0; 8],
            top: [0; 8],
            right: [0; 8],
            bottom: [0; 8],
        };
        let hit = collision.touch_collision(&mut store, 0, -8, -8, 8, 8, 1, -8, -8, 8, 8, &hitbox);
        assert!(hit);
        store.get_mut(1).unwrap().xpos = 200 << 16;
        let miss = collision.touch_collision(&mut store, 0, -8, -8, 8, 8, 1, -8, -8, 8, 8, &hitbox);
        assert!(!miss);
    }

    #[test]
    fn movement_falls_onto_floor() {
        let mut collision = SceneCollision::new(layout(), solid_tiles(), solid_floor_masks());
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 64 << 16, 60 << 16);
        {
            let entity = store.get_mut(0).unwrap();
            entity.yvel = 0x40000;
            entity.tile_collisions = 1;
        }
        let on_ground = collision.process_object_movement(&mut store, 0);
        assert!(on_ground);
        assert_eq!(store.get(0).unwrap().yvel, 0);
        assert_eq!(store.get(0).unwrap().floor_sensors[1], 1);
    }

    #[test]
    fn movement_without_tile_collisions_integrates() {
        let mut collision = SceneCollision::new(layout(), blank_tiles(), blank_masks());
        let mut store = EntityStore::new();
        store.reset_object_entity(0, 1, 0, 0, 0);
        {
            let entity = store.get_mut(0).unwrap();
            entity.xvel = 0x10000;
            entity.yvel = 0x20000;
            entity.tile_collisions = 0;
        }
        assert!(!collision.process_object_movement(&mut store, 0));
        assert_eq!(
            (store.get(0).unwrap().xpos, store.get(0).unwrap().ypos),
            (0x10000, 0x20000)
        );
    }
}
