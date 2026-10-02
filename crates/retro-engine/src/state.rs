//! Mutable engine state shared with the script host.
//!
//! [`EngineState`] holds everything an engine op can read or write except the script VM
//! registers ([`retro_script::VmState`]) and the compiled script file, which live in
//! [`crate::ScriptRuntime`] so the VM and the host can be borrowed at the same time.

use std::collections::BTreeMap;
use std::sync::Arc;

use retro_format_v4::{AnimationFile, Backgrounds, GameConfig, Hitbox, Scene, StageConfig};
use retro_io::DataSource;
use retro_scene::{
    Camera, EntityStore, MathTables, OBJECT_COUNT, ObjectRegistry, SceneCollision, Screen,
    StageState, TYPEGROUP_COUNT, TypeGroupList,
};

use crate::profile::EngineSettings;
use crate::rng::GlibcRand;

/// Number of tile layers upstream keeps (`LAYER_COUNT`).
pub const LAYER_COUNT: usize = 9;
/// Width of the engine tile-layer buffer (`TILELAYER_CHUNK_W`).
pub const TILE_LAYER_STRIDE: usize = 0x100;
/// Height of the engine tile-layer buffer (`TILELAYER_CHUNK_H`).
pub const TILE_LAYER_HEIGHT: usize = 0x100;
/// Number of parallax entries (`PARALLAX_COUNT`).
pub const PARALLAX_COUNT: usize = 0x100;
/// `ENGINE_MAINGAME`, the only engine mode modelled by M3.
pub const ENGINE_MAINGAME: i32 = 1;

/// Digital input state mirroring upstream `InputState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct InputState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub button_a: bool,
    pub button_b: bool,
    pub button_c: bool,
    pub button_x: bool,
    pub button_y: bool,
    pub button_z: bool,
    pub button_l: bool,
    pub button_r: bool,
    pub start: bool,
    pub select: bool,
}

impl InputState {
    /// Reads the key-down variable for a rev03 input id (170..=183), or `None`.
    #[must_use]
    pub fn down(&self, var: i32) -> Option<bool> {
        let value = match var {
            170 => self.up,
            171 => self.down,
            172 => self.left,
            173 => self.right,
            174 => self.button_a,
            175 => self.button_b,
            176 => self.button_c,
            177 => self.button_x,
            178 => self.button_y,
            179 => self.button_z,
            180 => self.button_l,
            181 => self.button_r,
            182 => self.start,
            183 => self.select,
            _ => return None,
        };
        Some(value)
    }

    /// Reads the key-press variable for a rev03 input id (184..=197), or `None`.
    #[must_use]
    pub fn press(&self, var: i32) -> Option<bool> {
        self.down(var - 14)
    }
}

/// One of the engine's nine tile layers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LayerState {
    pub xsize: i32,
    pub ysize: i32,
    pub layer_type: i32,
    pub angle: i32,
    pub xpos: i32,
    pub ypos: i32,
    pub zpos: i32,
    pub parallax_factor: i32,
    pub scroll_speed: i32,
    pub scroll_pos: i32,
    pub deformation_offset: i32,
    pub deformation_offset_w: i32,
    /// Chunk indices in the engine's `0x100`-wide layout.
    pub tiles: Vec<u16>,
}

impl Default for LayerState {
    fn default() -> Self {
        Self {
            xsize: 0,
            ysize: 0,
            layer_type: 0,
            angle: 0,
            xpos: 0,
            ypos: 0,
            zpos: 0,
            parallax_factor: 0,
            scroll_speed: 0,
            scroll_pos: 0,
            deformation_offset: 0,
            deformation_offset_w: 0,
            tiles: vec![0; TILE_LAYER_STRIDE * TILE_LAYER_HEIGHT],
        }
    }
}

impl LayerState {
    /// Reads a chunk entry with the upstream `x + 0x100 * y` addressing.
    #[must_use]
    pub fn entry(&self, x: i32, y: i32) -> u16 {
        let index = i64::from(x) + 0x100 * i64::from(y);
        usize::try_from(index)
            .ok()
            .and_then(|index| self.tiles.get(index).copied())
            .unwrap_or(0)
    }

    /// Writes a chunk entry with the upstream `x + 0x100 * y` addressing.
    pub fn set_entry(&mut self, x: i32, y: i32, value: u16) {
        let index = i64::from(x) + 0x100 * i64::from(y);
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.tiles.get_mut(index))
        {
            *slot = value;
        }
    }
}

/// Horizontal or vertical parallax table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParallaxState {
    pub parallax_factor: Vec<i32>,
    pub scroll_speed: Vec<i32>,
    pub scroll_pos: Vec<i32>,
}

impl Default for ParallaxState {
    fn default() -> Self {
        Self {
            parallax_factor: vec![0; PARALLAX_COUNT],
            scroll_speed: vec![0; PARALLAX_COUNT],
            scroll_pos: vec![0; PARALLAX_COUNT],
        }
    }
}

/// Mutable state owned by the engine and mutated by host operations.
pub struct EngineState {
    /// Asset source used by runtime file operations (`LoadAnimation`).
    pub source: Arc<dyn DataSource>,
    /// Resolved engine settings.
    pub settings: EngineSettings,
    /// Parsed `GameConfig.bin`.
    pub game_config: GameConfig,
    /// Folder of the loaded stage (`Zone01`).
    pub stage_folder: String,
    /// Act id of the loaded scene (`1`).
    pub act: String,
    /// Parsed `ActN.bin`.
    pub scene: Scene,
    /// Parsed `StageConfig.bin`.
    pub stage_config: StageConfig,
    /// Tile collision context, absent when the stage has no collision data.
    pub collision: Option<SceneCollision>,
    /// Parsed `Backgrounds.bin`, absent when the stage has none.
    pub backgrounds: Option<Backgrounds>,
    /// Merged object list.
    pub objects: ObjectRegistry,
    /// Entity bank.
    pub entities: EntityStore,
    /// Trig lookup tables.
    pub math: MathTables,
    /// Deterministic glibc-compatible RNG.
    pub rng: GlibcRand,
    /// Camera globals.
    pub camera: Camera,
    /// Screen globals.
    pub screen: Screen,
    /// Stage globals.
    pub stage: StageState,
    /// Nine tile layers.
    pub layers: Vec<LayerState>,
    /// Horizontal parallax table.
    pub h_parallax: ParallaxState,
    /// Vertical parallax table.
    pub v_parallax: ParallaxState,
    /// `processObjectFlag` for the current frame.
    pub process_flags: Vec<bool>,
    /// Type groups rebuilt after each update pass.
    pub type_groups: Vec<TypeGroupList>,
    /// Draw lists per layer.
    pub draw_lists: Vec<Vec<i32>>,
    /// `objectEntityPos`: the entity slot whose event is executing.
    pub object_entity_pos: usize,
    /// `OBJECT_BORDER_X1..Y4`-derived update bounds.
    pub object_borders: [i32; 4],
    /// Input state (headless scripted source; empty by default).
    pub input: InputState,
    /// Key-press input state.
    pub input_press: InputState,
    /// Currently playing music track id.
    pub music_track: i32,
    /// `menu1.selection`.
    pub menu1_selection: i32,
    /// `menu2.selection`.
    pub menu2_selection: i32,
    /// Touchscreen state (`down`, `x`, `y` by index).
    pub touch_down: Vec<i32>,
    /// Touchscreen x positions.
    pub touch_x: Vec<i32>,
    /// Touchscreen y positions.
    pub touch_y: Vec<i32>,
    /// Loaded animation files.
    pub animations: Vec<AnimationFile>,
    /// Animation name to [`EngineState::animations`] index.
    pub animation_ids: BTreeMap<String, usize>,
    /// Loaded sprite sheet names.
    pub sprite_sheets: Vec<String>,
    /// Frames executed so far.
    pub frame: u64,
    /// Engine ops executed, by name.
    pub op_histogram: BTreeMap<String, u64>,
    /// Ops implemented as deterministic stubs, by name.
    pub stub_histogram: BTreeMap<String, u64>,
    /// Set by `LoadStage`; a real scene switch is deferred to the next milestone.
    pub load_stage_requested: bool,
}

impl EngineState {
    /// Assembles the state for a loaded scene.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        source: Arc<dyn DataSource>,
        settings: EngineSettings,
        game_config: GameConfig,
        stage_folder: String,
        act: String,
        scene: Scene,
        stage_config: StageConfig,
        collision: Option<SceneCollision>,
        backgrounds: Option<Backgrounds>,
        objects: ObjectRegistry,
        rng: GlibcRand,
    ) -> Self {
        let mut layers = vec![LayerState::default(); LAYER_COUNT];
        if let Some(main) = layers.first_mut() {
            main.xsize = i32::from(scene.width);
            main.ysize = i32::from(scene.height);
            main.layer_type = 1; // LAYER_HSCROLL
            for y in 0..i32::from(scene.height) {
                for x in 0..i32::from(scene.width) {
                    let chunk = scene
                        .layout
                        .get(usize::try_from(y * i32::from(scene.width) + x).unwrap_or(usize::MAX))
                        .copied()
                        .unwrap_or(0);
                    main.set_entry(x, y, chunk);
                }
            }
        }
        let stage = StageState::from_scene(&scene);
        let mut h_parallax = ParallaxState::default();
        let mut v_parallax = ParallaxState::default();
        if let Some(backgrounds) = &backgrounds {
            for (index, entry) in backgrounds
                .horizontal
                .iter()
                .take(PARALLAX_COUNT)
                .enumerate()
            {
                h_parallax.parallax_factor[index] = i32::from(entry.parallax_factor);
                h_parallax.scroll_speed[index] = entry.scroll_speed;
            }
            for (index, entry) in backgrounds.vertical.iter().take(PARALLAX_COUNT).enumerate() {
                v_parallax.parallax_factor[index] = i32::from(entry.parallax_factor);
                v_parallax.scroll_speed[index] = entry.scroll_speed;
            }
            for (index, layer) in backgrounds.layers.iter().enumerate() {
                if let Some(slot) = layers.get_mut(index + 1) {
                    slot.xsize = i32::from(layer.width);
                    slot.ysize = i32::from(layer.height);
                    slot.layer_type = i32::from(layer.layer_type);
                    slot.parallax_factor = i32::from(layer.parallax_factor);
                    slot.scroll_speed = layer.scroll_speed;
                    for y in 0..i32::from(layer.height) {
                        for x in 0..i32::from(layer.width) {
                            let chunk = layer
                                .layout
                                .get(
                                    usize::try_from(y * i32::from(layer.width) + x)
                                        .unwrap_or(usize::MAX),
                                )
                                .copied()
                                .unwrap_or(0);
                            slot.set_entry(x, y, chunk);
                        }
                    }
                }
            }
        }
        Self {
            source,
            settings,
            game_config,
            stage_folder,
            act,
            scene,
            stage_config,
            collision,
            backgrounds,
            objects,
            entities: EntityStore::new(),
            math: MathTables::new(),
            rng,
            camera: Camera::scene_load(),
            screen: Screen::v4(),
            stage,
            layers,
            h_parallax,
            v_parallax,
            process_flags: vec![false; retro_scene::ENTITY_COUNT],
            type_groups: vec![TypeGroupList::default(); TYPEGROUP_COUNT],
            draw_lists: vec![Vec::new(); retro_scene::DRAWLAYER_COUNT],
            object_entity_pos: 0,
            object_borders: [0x80, 424 + 0x80, 0x20, 424 + 0x20],
            input: InputState::default(),
            input_press: InputState::default(),
            music_track: 0,
            menu1_selection: 0,
            menu2_selection: 0,
            touch_down: vec![0; 4],
            touch_x: vec![0; 4],
            touch_y: vec![0; 4],
            animations: Vec::new(),
            animation_ids: BTreeMap::new(),
            sprite_sheets: Vec::new(),
            frame: 0,
            op_histogram: BTreeMap::new(),
            stub_histogram: BTreeMap::new(),
            load_stage_requested: false,
        }
    }

    /// Records an engine op in the histogram.
    pub fn record_op(&mut self, op: &str) {
        *self.op_histogram.entry(op.to_owned()).or_insert(0) += 1;
    }

    /// Records an op running as a deterministic stub.
    pub fn record_stub(&mut self, op: &str) {
        self.record_op(op);
        *self.stub_histogram.entry(op.to_owned()).or_insert(0) += 1;
    }

    /// Resolves the hitbox of entity `slot` through its object animation, falling back to a
    /// zero hitbox when the animation is missing.
    #[must_use]
    pub fn hitbox_for(&self, slot: usize) -> Hitbox {
        match self.entities.get(slot) {
            Some(entity) => hitbox_from(&self.objects, &self.animations, entity),
            None => zero_hitbox(),
        }
    }

    /// Returns the number of registered object types.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len().max(OBJECT_COUNT)
    }
}

/// A hitbox with all extents zero, used when no animation is assigned.
#[must_use]
pub fn zero_hitbox() -> Hitbox {
    Hitbox {
        left: [0; 8],
        top: [0; 8],
        right: [0; 8],
        bottom: [0; 8],
    }
}

/// Resolves the current hitbox of `entity` through its object and animation file.
#[must_use]
pub fn hitbox_from(
    objects: &ObjectRegistry,
    animations: &[AnimationFile],
    entity: &retro_scene::Entity,
) -> Hitbox {
    let zero = zero_hitbox();
    let Some(entry) = objects.get(usize::from(entity.type_id)) else {
        return zero;
    };
    let Some(animation_file) = entry.animation_file else {
        return zero;
    };
    let Some(file) = animations.get(animation_file) else {
        return zero;
    };
    let Some(animation) = file.animations.get(usize::from(entity.animation)) else {
        return zero;
    };
    let Some(frame) = animation.frames.get(usize::from(entity.frame)) else {
        return zero;
    };
    file.hitboxes
        .get(usize::from(frame.hitbox_id))
        .cloned()
        .unwrap_or(zero)
}
