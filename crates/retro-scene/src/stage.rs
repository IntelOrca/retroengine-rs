//! Scene tile layout and stage runtime state.
//!
//! Ported from `LoadActLayout` and the stage globals in `RSDKv4/Scene.cpp`
//! (RSDKModding/RSDKv4-Decompilation @ a7f5195). The act file stores 128x128 chunk indices in a
//! `width * height` grid; upstream keeps them in a `0x100`-wide buffer (stride
//! [`TILE_LAYER_CHUNK_WIDTH`]) and collision code indexes with `chunkX + (chunkY << 8)`. This
//! module stores the compact row-major grid instead and translates indices, returning the
//! zero-initialised out-of-range chunk just like the sparse upstream buffer.

use serde::Serialize;

use retro_format_v4::Scene;

/// Width of the engine's chunk buffer (`TILELAYER_CHUNK_W`).
pub const TILE_LAYER_CHUNK_WIDTH: usize = 0x100;
/// Height of the engine's chunk buffer (`TILELAYER_CHUNK_H`).
pub const TILE_LAYER_CHUNK_HEIGHT: usize = 0x100;
/// Side length of a 128x128 chunk in pixels (`CHUNK_SIZE`).
pub const CHUNK_SIZE: i32 = 0x80;
/// Number of 16x16 tiles per chunk edge.
pub const CHUNK_TILE_SIZE: i32 = 8;
/// Number of 16x16 collision tiles (`TILE_COUNT`).
pub const TILE_COUNT: usize = 0x400;

/// The active 128x128 chunk grid of tile layer 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageLayout {
    /// Layout width in 128x128 chunks (`stageLayouts[0].xsize`).
    pub width: u8,
    /// Layout height in 128x128 chunks (`stageLayouts[0].ysize`).
    pub height: u8,
    /// Chunk indices, row-major with stride [`StageLayout::width`].
    pub tiles: Vec<u16>,
}

impl StageLayout {
    /// Copies the layout from a parsed scene.
    #[must_use]
    pub fn from_scene(scene: &Scene) -> Self {
        Self {
            width: scene.width,
            height: scene.height,
            tiles: scene.layout.clone(),
        }
    }

    /// Returns the chunk index at `(chunk_x, chunk_y)` or `0` when outside the layout,
    /// matching the zero-initialised upstream buffer.
    #[must_use]
    pub fn chunk(&self, chunk_x: i32, chunk_y: i32) -> u16 {
        if chunk_x < 0 || chunk_y < 0 {
            return 0;
        }
        let (Ok(x), Ok(y)) = (usize::try_from(chunk_x), usize::try_from(chunk_y)) else {
            return 0;
        };
        if x >= usize::from(self.width) || y >= usize::from(self.height) {
            return 0;
        }
        self.tiles
            .get(y * usize::from(self.width) + x)
            .copied()
            .unwrap_or(0)
    }

    /// Layout width in pixels (`xsize << 7`).
    #[must_use]
    pub fn width_px(&self) -> i32 {
        i32::from(self.width) << 7
    }

    /// Layout height in pixels (`ysize << 7`).
    #[must_use]
    pub fn height_px(&self) -> i32 {
        i32::from(self.height) << 7
    }
}

/// Stage runtime globals written by `LoadActLayout` and read/written by scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageState {
    /// `stage.state`.
    pub state: i32,
    /// `stage.activeList`.
    pub active_list: i32,
    /// `stage.listPos`.
    pub list_pos: i32,
    /// `stage.timeEnabled`.
    pub time_enabled: bool,
    /// Hundredths of a second shown by the HUD.
    pub milliseconds: i32,
    /// Seconds shown by the HUD.
    pub seconds: i32,
    /// Minutes shown by the HUD.
    pub minutes: i32,
    /// Act number.
    pub act_num: i32,
    /// `stage.pauseEnabled`.
    pub pause_enabled: bool,
    /// Stage list size.
    pub list_size: i32,
    /// New camera X boundary 1.
    pub new_x_boundary1: i32,
    /// New camera X boundary 2.
    pub new_x_boundary2: i32,
    /// New camera Y boundary 1.
    pub new_y_boundary1: i32,
    /// New camera Y boundary 2.
    pub new_y_boundary2: i32,
    /// Current camera X boundary 1.
    pub cur_x_boundary1: i32,
    /// Current camera X boundary 2.
    pub cur_x_boundary2: i32,
    /// Current camera Y boundary 1.
    pub cur_y_boundary1: i32,
    /// Current camera Y boundary 2.
    pub cur_y_boundary2: i32,
    /// Water level.
    pub water_level: i32,
    /// Active tile layers (`activeTileLayers[4]`).
    pub active_layers: [i32; 4],
    /// Tile layer midpoint (`tLayerMidPoint`).
    pub mid_point: i32,
    /// Player list position used by the HUD.
    pub player_list_pos: i32,
    /// Debug mode flag.
    pub debug_mode: i32,
    /// Frame counter used by the clock.
    pub frame_counter: i32,
}

impl StageState {
    /// Applies the values `LoadActLayout` sets for `scene`.
    #[must_use]
    pub fn from_scene(scene: &Scene) -> Self {
        let width_px = i32::from(scene.width) << 7;
        let height_px = i32::from(scene.height) << 7;
        Self {
            state: 0,
            active_list: 0,
            list_pos: 0,
            time_enabled: false,
            milliseconds: 0,
            seconds: 0,
            minutes: 0,
            act_num: 0,
            pause_enabled: false,
            list_size: 0,
            new_x_boundary1: 0,
            new_x_boundary2: width_px,
            new_y_boundary1: 0,
            new_y_boundary2: height_px,
            cur_x_boundary1: 0,
            cur_x_boundary2: width_px,
            cur_y_boundary1: 0,
            cur_y_boundary2: height_px,
            water_level: height_px + 128,
            active_layers: scene.active_layers.map(i32::from),
            mid_point: i32::from(scene.mid_point),
            player_list_pos: 0,
            debug_mode: 0,
            frame_counter: 0,
        }
    }

    /// Reads the stage variable with the given rev03 variable id, or `None` for ids outside the
    /// `stage.*` block.
    #[must_use]
    pub fn read(&self, var: i32, array_index: i32) -> Option<i32> {
        let value = match var {
            121 => self.state,
            122 => self.active_list,
            123 => self.list_pos,
            124 => i32::from(self.time_enabled),
            125 => self.milliseconds,
            126 => self.seconds,
            127 => self.minutes,
            128 => self.act_num,
            129 => i32::from(self.pause_enabled),
            130 => self.list_size,
            131 => self.new_x_boundary1,
            132 => self.new_x_boundary2,
            133 => self.new_y_boundary1,
            134 => self.new_y_boundary2,
            135 => self.cur_x_boundary1,
            136 => self.cur_x_boundary2,
            137 => self.cur_y_boundary1,
            138 => self.cur_y_boundary2,
            143 => self.water_level,
            144 => *self.active_layers.get(usize::try_from(array_index).ok()?)?,
            145 => self.mid_point,
            146 => self.player_list_pos,
            147 => self.debug_mode,
            _ => return None,
        };
        Some(value)
    }

    /// Writes the stage variable with the given rev03 variable id. Returns `false` when the id is
    /// not a stage variable.
    pub fn write(&mut self, var: i32, array_index: i32, value: i32) -> bool {
        match var {
            121 => self.state = value,
            122 => self.active_list = value,
            123 => self.list_pos = value,
            124 => self.time_enabled = value != 0,
            125 => self.milliseconds = value,
            126 => self.seconds = value,
            127 => self.minutes = value,
            128 => self.act_num = value,
            129 => self.pause_enabled = value != 0,
            130 => self.list_size = value,
            131 => self.new_x_boundary1 = value,
            132 => self.new_x_boundary2 = value,
            133 => self.new_y_boundary1 = value,
            134 => self.new_y_boundary2 = value,
            135 => self.cur_x_boundary1 = value,
            136 => self.cur_x_boundary2 = value,
            137 => self.cur_y_boundary1 = value,
            138 => self.cur_y_boundary2 = value,
            143 => self.water_level = value,
            144 => {
                if let Some(slot) = usize::try_from(array_index)
                    .ok()
                    .and_then(|index| self.active_layers.get_mut(index))
                {
                    *slot = value;
                }
            }
            145 => self.mid_point = value,
            146 => self.player_list_pos = value,
            147 => self.debug_mode = value,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_format_v4::Scene;

    fn scene() -> Scene {
        Scene {
            title: "TEST".to_owned(),
            active_layers: [1, 9, 0, 0],
            mid_point: 3,
            width: 2,
            height: 2,
            layout: vec![10, 11, 12, 13],
            entities: Vec::new(),
        }
    }

    #[test]
    fn layout_translates_upstream_indices() {
        let layout = StageLayout::from_scene(&scene());
        assert_eq!(layout.chunk(0, 0), 10);
        assert_eq!(layout.chunk(1, 0), 11);
        assert_eq!(layout.chunk(0, 1), 12);
        assert_eq!(layout.chunk(1, 1), 13);
        assert_eq!(layout.chunk(2, 0), 0);
        assert_eq!(layout.chunk(-1, 0), 0);
        assert_eq!(layout.width_px(), 256);
        assert_eq!(layout.height_px(), 256);
    }

    #[test]
    fn stage_state_seeds_boundaries_from_scene() {
        let state = StageState::from_scene(&scene());
        assert_eq!(state.cur_x_boundary2, 256);
        assert_eq!(state.cur_y_boundary2, 256);
        assert_eq!(state.water_level, 384);
        assert_eq!(state.active_layers, [1, 9, 0, 0]);
        assert_eq!(state.mid_point, 3);
    }

    #[test]
    fn stage_variables_round_trip() {
        let mut state = StageState::from_scene(&scene());
        for var in 121..=147 {
            let before = state.read(var, 0);
            if let Some(value) = before {
                assert!(state.write(var, 0, value + 1));
                assert_eq!(state.read(var, 0), Some(value + 1));
            }
        }
        assert_eq!(state.read(120, 0), None);
        assert_eq!(state.read(148, 0), None);
        assert!(!state.write(120, 0, 1));
        assert_eq!(state.read(144, 9), None);
    }
}
