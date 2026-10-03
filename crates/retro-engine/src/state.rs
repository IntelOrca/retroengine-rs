//! Mutable engine state shared with the script host.
//!
//! [`EngineState`] holds everything an engine op can read or write except the script VM
//! registers ([`retro_script::VmState`]) and the compiled script file, which live in
//! [`crate::ScriptRuntime`] so the VM and the host can be borrowed at the same time.

use std::collections::BTreeMap;
use std::sync::Arc;

use retro_format_v4::{
    AnimationFile, Backgrounds, GameConfig, Hitbox, Scene, StageConfig, TileSheet16,
};
use retro_io::DataSource;
use retro_render::{ACTIVE_PALETTE, RenderState, Surface};
use retro_scene::{
    Camera, EntityStore, MathTables, OBJECT_COUNT, ObjectRegistry, SceneCollision, Screen,
    StageState, TYPEGROUP_COUNT, TypeGroupList,
};

use crate::audio::AudioState;
use crate::profile::EngineSettings;
use crate::rng::GlibcRand;
use crate::save::SaveState;

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
/// `STAGEMODE_NORMAL`: the stage mode `ProcessStage` enters after finishing `STAGEMODE_LOAD`.
pub const STAGEMODE_NORMAL: i32 = 1;
/// Number of text menus upstream keeps (`TEXTMENU_COUNT`).
pub const TEXT_MENU_COUNT: usize = 0x2;
/// Maximum characters stored per text menu (`TEXTDATA_COUNT`).
pub const TEXT_DATA_COUNT: usize = 0x2800;
/// Maximum rows stored per text menu (`TEXTENTRY_COUNT`).
pub const TEXT_ENTRY_COUNT: usize = 0x200;
/// Number of font characters upstream keeps (`FONTCHAR_COUNT`).
pub const FONT_CHAR_COUNT: usize = 0x400;
/// Number of simultaneous touch points upstream keeps (`touchDown[8]`).
pub const TOUCH_COUNT: usize = 8;

/// One glyph of the legacy v4 bitmap font (`FontCharacter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct FontCharacter {
    /// Character id used by text files (`FontCharacter::id`).
    pub id: i32,
    /// Source x in the current text sheet.
    pub src_x: i32,
    /// Source y in the current text sheet.
    pub src_y: i32,
    /// Glyph width in pixels.
    pub width: i32,
    /// Glyph height in pixels.
    pub height: i32,
    /// Signed pivot x.
    pub pivot_x: i32,
    /// Signed pivot y.
    pub pivot_y: i32,
    /// Horizontal advance.
    pub x_advance: i32,
}

/// One legacy v4 text menu (`TextMenu`), storing rows of character ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TextMenu {
    /// Character ids per row, concatenated (`textData`).
    pub text_data: Vec<u16>,
    /// Start offset of each row in [`TextMenu::text_data`].
    pub entry_start: Vec<i32>,
    /// Character count of each row.
    pub entry_size: Vec<i32>,
    /// Current write position.
    pub text_data_pos: usize,
    /// Number of rows.
    pub row_count: i32,
}

impl TextMenu {
    fn reset(&mut self) {
        self.text_data.clear();
        self.entry_start.clear();
        self.entry_size.clear();
        self.text_data_pos = 0;
        self.row_count = 0;
        self.entry_start.push(0);
        self.entry_size.push(0);
    }

    fn new_row(&mut self) {
        self.row_count += 1;
        if self.row_count as usize >= TEXT_ENTRY_COUNT {
            return;
        }
        self.entry_start.push(self.text_data_pos as i32);
        self.entry_size.push(0);
    }

    fn push_char(&mut self, value: u16) {
        if self.text_data.len() >= TEXT_DATA_COUNT {
            return;
        }
        self.text_data.push(value);
        if let Some(size) = self.entry_size.last_mut() {
            *size += 1;
        }
        self.text_data_pos += 1;
    }
}

/// Re-exported tile layer state (`TileLayer`).
pub use retro_render::LayerState;
/// Re-exported parallax table state (`LineScroll`).
pub use retro_render::ParallaxState;

/// A sprite frame resolved to draw-ready integers plus its sheet and rotation style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ResolvedFrame {
    sheet: i32,
    pivot_x: i32,
    pivot_y: i32,
    width: i32,
    height: i32,
    spr_x: i32,
    spr_y: i32,
    rotation_style: u8,
    rotation: i32,
}

/// One entry of an object's script frame list (`SpriteFrame`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ScriptFrame {
    /// Signed pivot x.
    pub pivot_x: i32,
    /// Signed pivot y.
    pub pivot_y: i32,
    /// Frame width in pixels.
    pub width: i32,
    /// Frame height in pixels.
    pub height: i32,
    /// Source x in the object's sprite sheet.
    pub spr_x: i32,
    /// Source y in the object's sprite sheet.
    pub spr_y: i32,
}

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

    /// Whether any button is pressed or held (`inputDevice[INPUT_ANY].press || hold`).
    #[must_use]
    pub fn any_button(&self) -> bool {
        self.up
            || self.down
            || self.left
            || self.right
            || self.button_a
            || self.button_b
            || self.button_c
            || self.button_x
            || self.button_y
            || self.button_z
            || self.button_l
            || self.button_r
            || self.start
            || self.select
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
    /// SFX/music tables, lazy asset loading and the output device.
    pub audio: AudioState,
    /// Persistent save RAM and the backing storage.
    pub save: SaveState,
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
    /// Resolved surface ids per loaded animation file, indexed by on-disk sheet index.
    pub animation_sheet_ids: Vec<Vec<i32>>,
    /// Sprite sheet names aligned with [`RenderState::surfaces`] slots.
    pub sprite_sheet_names: Vec<Option<String>>,
    /// Legacy v4 bitmap font glyphs (`fontCharacterList`).
    pub font_characters: Vec<FontCharacter>,
    /// Legacy v4 text menus (`gameMenu`).
    pub text_menus: Vec<TextMenu>,
    /// Sprite sheet used by `DrawText` (`textMenuSurfaceNo`).
    pub text_menu_surface_no: i32,
    /// Per-object-type script frame lists (`scriptFrames` + `frameListOffset`).
    pub object_frames: Vec<Vec<ScriptFrame>>,
    /// Software renderer state (framebuffer, palettes, sheets and tiles).
    pub render: RenderState,
    /// Frames executed so far.
    pub frame: u64,
    /// Engine ops executed, by name.
    pub op_histogram: BTreeMap<String, u64>,
    /// Ops implemented as deterministic stubs, by name.
    pub stub_histogram: BTreeMap<String, u64>,
    /// Set by `LoadStage`; the runtime applies the scene switch at the start of the next frame,
    /// mirroring upstream's `stageMode = STAGEMODE_LOAD` handoff.
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
            h_parallax.entry_count = backgrounds.horizontal.len().min(PARALLAX_COUNT);
            v_parallax.entry_count = backgrounds.vertical.len().min(PARALLAX_COUNT);
            for (index, entry) in backgrounds
                .horizontal
                .iter()
                .take(PARALLAX_COUNT)
                .enumerate()
            {
                h_parallax.parallax_factor[index] = i32::from(entry.parallax_factor);
                h_parallax.scroll_speed[index] = entry.scroll_speed;
                h_parallax.deform[index] = i32::from(entry.deform);
            }
            for (index, entry) in backgrounds.vertical.iter().take(PARALLAX_COUNT).enumerate() {
                v_parallax.parallax_factor[index] = i32::from(entry.parallax_factor);
                v_parallax.scroll_speed[index] = entry.scroll_speed;
                v_parallax.deform[index] = i32::from(entry.deform);
            }
            for (index, layer) in backgrounds.layers.iter().enumerate() {
                if let Some(slot) = layers.get_mut(index + 1) {
                    slot.xsize = i32::from(layer.width);
                    slot.ysize = i32::from(layer.height);
                    slot.layer_type = i32::from(layer.layer_type);
                    slot.parallax_factor = i32::from(layer.parallax_factor);
                    slot.scroll_speed = layer.scroll_speed;
                    for (offset, byte) in layer.line_scroll.iter().enumerate() {
                        if let Some(target) = slot.line_scroll.get_mut(offset) {
                            *target = *byte;
                        }
                    }
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
        let screen = Screen::v4();
        let object_frames = vec![Vec::new(); objects.len().max(1)];
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
            screen,
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
            audio: AudioState::empty(),
            save: SaveState::in_memory(),
            music_track: 0,
            menu1_selection: 0,
            menu2_selection: 0,
            touch_down: vec![0; TOUCH_COUNT],
            touch_x: vec![0; TOUCH_COUNT],
            touch_y: vec![0; TOUCH_COUNT],
            animations: Vec::new(),
            animation_ids: BTreeMap::new(),
            animation_sheet_ids: Vec::new(),
            sprite_sheet_names: Vec::new(),
            font_characters: vec![FontCharacter::default(); FONT_CHAR_COUNT],
            text_menus: vec![TextMenu::default(); TEXT_MENU_COUNT],
            text_menu_surface_no: 0,
            object_frames,
            render: RenderState::new(
                usize::try_from(screen.xsize).unwrap_or(424),
                usize::try_from(screen.ysize).unwrap_or(240),
            ),
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

    /// Applies the `GameConfig.bin` master palette (engine indices `0x00..0x60`) to the active
    /// bank, mirroring `LoadGameConfig`'s `SetPaletteEntry(-1, ...)`.
    pub fn apply_game_palette(&mut self) {
        let colors = self.game_config.palette.clone();
        self.render
            .palette
            .set_bank_entries(ACTIVE_PALETTE, 0, &colors);
    }

    /// Applies the `StageConfig.bin` palette (engine indices `0x60..0x80`).
    pub fn apply_stage_palette(&mut self) {
        let colors = self.stage_config.palette.clone();
        self.render
            .palette
            .set_bank_entries(ACTIVE_PALETTE, 0x60, &colors);
    }

    /// Applies a decoded `16x16Tiles.gif` exactly like `LoadStageGIFFile`: the sheet's
    /// `0x80..0x100` palette entries go to the active bank and every pixel equal to the first
    /// pixel becomes index `0` (transparent).
    pub fn apply_tile_sheet(&mut self, sheet: &TileSheet16) {
        if let Some(colors) = sheet.palette.get(0x80..0x100) {
            self.render
                .palette
                .set_bank_entries(ACTIVE_PALETTE, 0x80, colors);
        }
        let transparent = sheet.pixels.first().copied().unwrap_or(0);
        self.render.tiles.pixels.clear();
        self.render.tiles.pixels.reserve(sheet.pixels.len());
        for index in &sheet.pixels {
            self.render
                .tiles
                .pixels
                .push(if *index == transparent { 0 } else { *index });
        }
    }

    /// `AddGraphicsFile`: resolves a `Data/Sprites/<name>` sheet to a resident surface id,
    /// loading and decoding it on first use. Returns `0` when the sheet cannot be loaded.
    pub fn load_sprite_sheet(&mut self, name: &str) -> i32 {
        if let Some(index) = self
            .sprite_sheet_names
            .iter()
            .position(|slot| slot.as_deref() == Some(name))
            && self
                .render
                .surfaces
                .get(index)
                .is_some_and(|surface| !surface.is_empty())
        {
            return index as i32;
        }
        let path = format!("Data/Sprites/{name}");
        let surface = self
            .source
            .read(&path)
            .ok()
            .and_then(|bytes| retro_image::decode_gif(&bytes).ok())
            .map(|image| Surface::from_indexed(image.width, image.height, image.pixels));
        let Some(surface) = surface else {
            return 0;
        };
        if let Some(index) = self
            .render
            .surfaces
            .iter()
            .position(|existing| existing.is_empty())
        {
            self.render.surfaces[index] = surface;
            if index >= self.sprite_sheet_names.len() {
                self.sprite_sheet_names.resize(index + 1, None);
            }
            self.sprite_sheet_names[index] = Some(name.to_owned());
            return index as i32;
        }
        if self.render.surfaces.len() >= retro_render::SURFACE_COUNT {
            return 0;
        }
        self.render.surfaces.push(surface);
        self.sprite_sheet_names.push(Some(name.to_owned()));
        (self.render.surfaces.len() - 1) as i32
    }

    /// `RemoveGraphicsFile(scriptText, -1)`: blanks every sheet with `name`, keeping its slot.
    pub fn remove_sprite_sheet(&mut self, name: &str) {
        for (index, slot) in self.sprite_sheet_names.iter_mut().enumerate() {
            if slot.as_deref() == Some(name) {
                *slot = None;
                if let Some(surface) = self.render.surfaces.get_mut(index) {
                    *surface = Surface::empty();
                }
            }
        }
    }

    /// `LoadFontFile`: parses 20-byte legacy font records into [`EngineState::font_characters`].
    ///
    /// Record layout: `u32` character id, `u16` srcX, `u16` srcY, `u16` width, `u16` height,
    /// then pivotX, pivotY and xAdvance as a low byte plus a sign-aware high byte, then two
    /// unused bytes. Upstream reads until EOF; this port caps at [`FONT_CHAR_COUNT`].
    pub fn load_font_file(&mut self, path: &str) {
        let Ok(bytes) = self.source.read(path) else {
            return;
        };
        let read_u16 = |offset: usize| -> i32 {
            i32::from(u16::from_le_bytes([
                bytes.get(offset).copied().unwrap_or(0),
                bytes.get(offset + 1).copied().unwrap_or(0),
            ]))
        };
        let read_short = |offset: usize| -> i32 {
            let low = i32::from(bytes.get(offset).copied().unwrap_or(0));
            let high = i32::from(bytes.get(offset + 1).copied().unwrap_or(0));
            if high > 0x80 {
                low + ((high - 0x80) << 8) - 0x8000
            } else {
                low + (high << 8)
            }
        };
        let mut character = 0usize;
        let mut offset = 0usize;
        while offset + 20 <= bytes.len() && character < FONT_CHAR_COUNT {
            let id = u32::from_le_bytes([
                bytes.get(offset).copied().unwrap_or(0),
                bytes.get(offset + 1).copied().unwrap_or(0),
                bytes.get(offset + 2).copied().unwrap_or(0),
                bytes.get(offset + 3).copied().unwrap_or(0),
            ]) as i32;
            self.font_characters[character] = FontCharacter {
                id,
                src_x: read_u16(offset + 4),
                src_y: read_u16(offset + 6),
                width: read_u16(offset + 8),
                height: read_u16(offset + 10),
                pivot_x: read_short(offset + 12),
                pivot_y: read_short(offset + 14),
                x_advance: read_short(offset + 16),
            };
            character += 1;
            offset += 20;
        }
    }

    /// `LoadTextFile`: reads a legacy v4 text file into `menu_index`.
    ///
    /// Files starting with `0xFF` store UTF-16LE codepoints after a skipped format byte,
    /// everything else stores single bytes. Both branches treat `\r` as a row break and ignore
    /// `\n`; `map_code` rewrites each value to the index of the first font character with a
    /// matching id.
    pub fn load_text_file(&mut self, menu_index: usize, path: &str, map_code: bool) {
        let Ok(bytes) = self.source.read(path) else {
            return;
        };
        let font_ids: Vec<i32> = self.font_characters.iter().map(|font| font.id).collect();
        let map = |value: u16| -> u16 {
            if map_code {
                font_ids
                    .iter()
                    .take(1024)
                    .position(|id| *id == i32::from(value))
                    .map_or(0, |index| index as u16)
            } else {
                value
            }
        };
        let Some(menu) = self.text_menus.get_mut(menu_index) else {
            return;
        };
        menu.reset();
        if bytes.starts_with(&[0xFF]) {
            let mut position = 2usize;
            while position + 1 < bytes.len()
                && menu.text_data.len() < TEXT_DATA_COUNT
                && (menu.row_count as usize) < TEXT_ENTRY_COUNT
            {
                let value = u16::from_le_bytes([bytes[position], bytes[position + 1]]);
                position += 2;
                if value == 0x0D {
                    menu.new_row();
                } else if value != 0x0A {
                    menu.push_char(map(value));
                }
            }
        } else {
            for &byte in &bytes {
                if byte == b'\r' {
                    menu.new_row();
                } else if byte != b'\n' {
                    menu.push_char(map(u16::from(byte)));
                }
                if menu.text_data.len() >= TEXT_DATA_COUNT {
                    break;
                }
            }
        }
        menu.row_count += 1;
    }

    /// `GetTextInfo`: reads `TEXTINFO_TEXTDATA` (0), `TEXTINFO_TEXTSIZE` (1) or
    /// `TEXTINFO_ROWCOUNT` (2) from menu `menu_index`.
    #[must_use]
    pub fn text_info(&self, menu_index: usize, info: i32, row: i32, character: i32) -> i32 {
        let Some(menu) = self.text_menus.get(menu_index) else {
            return 0;
        };
        match info {
            0 => {
                let Some(start) = usize::try_from(row)
                    .ok()
                    .and_then(|row| menu.entry_start.get(row))
                else {
                    return 0;
                };
                let Some(offset) = usize::try_from(character).ok() else {
                    return 0;
                };
                start
                    .checked_add(offset as i32)
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| menu.text_data.get(index))
                    .copied()
                    .map_or(0, i32::from)
            }
            1 => usize::try_from(row)
                .ok()
                .and_then(|row| menu.entry_size.get(row))
                .copied()
                .unwrap_or(0),
            2 => menu.row_count,
            _ => 0,
        }
    }

    /// `titleCardWord2`: the index after the last `-` in the act title, or the title length.
    #[must_use]
    pub fn title_card_word2(&self) -> i32 {
        let bytes = self.scene.title.as_bytes();
        let mut word2 = bytes.len();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'-' {
                word2 = index + 1;
            }
        }
        word2 as i32
    }

    /// The script frame list of the object type currently executing, growing the list when a
    /// test or mod registers objects after construction.
    fn current_frames(&mut self) -> Option<&mut Vec<ScriptFrame>> {
        let type_id = self
            .entities
            .get(self.object_entity_pos)
            .map(|entity| usize::from(entity.type_id))?;
        if self.object_frames.len() <= type_id {
            self.object_frames.resize(type_id + 1, Vec::new());
        }
        self.object_frames.get_mut(type_id)
    }

    /// `FUNC_SPRITEFRAME`: appends a script frame during the setup event.
    pub fn add_script_frame(&mut self, frame: ScriptFrame) {
        if let Some(frames) = self.current_frames()
            && frames.len() < 0x1000
        {
            frames.push(frame);
        }
    }

    /// `FUNC_EDITFRAME`: overwrites one script frame.
    pub fn edit_script_frame(&mut self, index: i32, frame: ScriptFrame) {
        if let Ok(index) = usize::try_from(index)
            && let Some(frames) = self.current_frames()
            && let Some(slot) = frames.get_mut(index)
        {
            *slot = frame;
        }
    }

    /// Returns the script frame at `index` for the current object type.
    #[must_use]
    pub fn script_frame(&self, index: i32) -> ScriptFrame {
        let type_id = self
            .entities
            .get(self.object_entity_pos)
            .map(|entity| usize::from(entity.type_id));
        type_id
            .and_then(|type_id| self.object_frames.get(type_id))
            .and_then(|frames| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| frames.get(index))
            })
            .copied()
            .unwrap_or_default()
    }

    /// `DrawObjectAnimation` for `slot`, resolving sheet ids and calling the renderer.
    ///
    /// The caller must have checked `entity.visible` when invoked through
    /// `FUNC_DRAWOBJECTANIMATION`; direct calls draw unconditionally like the upstream helper.
    pub fn draw_object_animation(&mut self, slot: usize) {
        let Some(entity) = self.entities.get(slot).copied() else {
            return;
        };
        let x = (entity.xpos >> 16) - self.screen.x_scroll;
        let y = (entity.ypos >> 16) - self.screen.y_scroll;
        let Some(render_frame) = self.resolve_animation_frame(&entity) else {
            return;
        };
        match render_frame.rotation_style {
            0 => {
                let width = render_frame.width;
                let height = render_frame.height;
                let (px, py, direction) = match entity.direction {
                    1 => (-width - render_frame.pivot_x, render_frame.pivot_y, 1),
                    2 => (render_frame.pivot_x, -height - render_frame.pivot_y, 2),
                    3 => (
                        -width - render_frame.pivot_x,
                        -height - render_frame.pivot_y,
                        3,
                    ),
                    0 => (render_frame.pivot_x, render_frame.pivot_y, 0),
                    _ => return,
                };
                self.render.draw_sprite_flipped(
                    render_frame.sheet,
                    x + px,
                    y + py,
                    width,
                    height,
                    render_frame.spr_x,
                    render_frame.spr_y,
                    direction,
                );
            }
            _ => {
                self.render.draw_sprite_rotated(
                    render_frame.sheet,
                    entity.direction,
                    x,
                    y,
                    -render_frame.pivot_x,
                    -render_frame.pivot_y,
                    render_frame.spr_x,
                    render_frame.spr_y,
                    render_frame.width,
                    render_frame.height,
                    render_frame.rotation,
                );
            }
        }
    }

    /// Resolves the sprite frame an entity should draw, including rotation-style frame selection.
    fn resolve_animation_frame(&self, entity: &retro_scene::Entity) -> Option<ResolvedFrame> {
        let entry = self.objects.get(usize::from(entity.type_id))?;
        let animation_index = entry.animation_file?;
        let file = self.animations.get(animation_index)?;
        let animation = file.animations.get(usize::from(entity.animation))?;
        let rotation_style = animation.rotation_style;
        let frame_index = usize::from(entity.frame);
        let rotation = match rotation_style {
            0 => {
                let frame = animation.frames.get(frame_index)?;
                return Some(self.resolved_frame(animation_index, frame, 0, rotation_style));
            }
            1 => entity.rotation,
            2 => {
                if entity.rotation >= 0x100 {
                    0x200 - ((0x214 - entity.rotation) >> 6 << 6)
                } else {
                    (entity.rotation + 20) >> 6 << 6
                }
            }
            3 => {
                let rotation_index = if entity.rotation >= 0x100 {
                    8 - ((532 - entity.rotation) >> 6)
                } else {
                    (entity.rotation + 20) >> 6
                };
                let mut frame_id = i32::from(entity.frame);
                let rotation = match rotation_index {
                    0 | 8 => 0,
                    1 => {
                        frame_id += i32::from(animation.playback_frame_count);
                        if entity.direction != 0 { 0 } else { 0x80 }
                    }
                    2 => 0x80,
                    3 => {
                        frame_id += i32::from(animation.playback_frame_count);
                        if entity.direction != 0 { 0x80 } else { 0x100 }
                    }
                    4 => 0x100,
                    5 => {
                        frame_id += i32::from(animation.playback_frame_count);
                        if entity.direction != 0 { 0x100 } else { 384 }
                    }
                    6 => 384,
                    7 => {
                        frame_id += i32::from(animation.playback_frame_count);
                        if entity.direction != 0 { 384 } else { 0 }
                    }
                    _ => 0,
                };
                let frame = usize::try_from(frame_id)
                    .ok()
                    .and_then(|index| animation.frames.get(index))?;
                return Some(self.resolved_frame(animation_index, frame, rotation, rotation_style));
            }
            _ => return None,
        };
        let frame = animation.frames.get(frame_index)?;
        Some(self.resolved_frame(animation_index, frame, rotation, rotation_style))
    }

    /// Copies one animation frame into draw-ready integers and resolves its sheet id.
    fn resolved_frame(
        &self,
        animation_index: usize,
        frame: &retro_format_v4::AnimationFrame,
        rotation: i32,
        rotation_style: u8,
    ) -> ResolvedFrame {
        let sheet = self
            .animation_sheet_ids
            .get(animation_index)
            .and_then(|ids| ids.get(usize::from(frame.sheet_index)))
            .copied()
            .unwrap_or(0);
        ResolvedFrame {
            sheet,
            pivot_x: i32::from(frame.pivot_x),
            pivot_y: i32::from(frame.pivot_y),
            width: i32::from(frame.width),
            height: i32::from(frame.height),
            spr_x: i32::from(frame.x),
            spr_y: i32::from(frame.y),
            rotation_style,
            rotation,
        }
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
