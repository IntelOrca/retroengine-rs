//! Engine-side script host: variables, `foreach` iteration and engine operations.
//!
//! This is the M3 port of the `ProcessScript` engine switches in `RSDKv4/Script.cpp` plus the
//! collision routines in `RSDKv4/Collision.cpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195).
//! Ops that only affect menus or 3D are deterministic stubs: they record themselves in
//! [`EngineState::stub_histogram`] and never touch entity or scene state.
//!
//! M5 wires the audio and save operations for real: `SetMusicTrack`/`PlayMusic`/`StopMusic`/
//! `PauseMusic`/`ResumeMusic`/`SwapMusicTrack` and `PlaySfx`/`StopSfx`/`SetSfxAttributes` drive
//! [`crate::audio::AudioState`], and `ReadSaveRAM`/`WriteSaveRAM` plus the `saveRAM` array
//! variable drive [`crate::save::SaveState`].
//!
//! Known gaps (documented, deterministic):
//!
//! * `BoxCollision2` (upstream's "barely used in S2" variant) and the 3D matrix/vertex ops are
//!   explicit stubs (see [`EngineState::stub_histogram`]); `TouchCollision`, `BoxCollision`,
//!   `PlatformCollision`, `Get16x16TileInfo`, `Set16x16TileInfo` and `Copy16x16Tile` are fully
//!   ported.
//! * `stage.deformationData0..3` are script-visible views of
//!   [`retro_render::RenderState::deform_data`]; `SetLayerDeformation` fills them and the tile
//!   layer renderers sample them.
//! * The legacy v4 text system (`LoadFontFile`/`LoadTextFile`/`GetTextInfo`/`DrawText`) and the
//!   title/HUD number and act-name draws are ported for rev00..rev03; the newer menu ops
//!   (`DrawMenu`, `SetupMenu`, ...) remain stubs.
//! * `LoadStage` records a deferred scene-load request; the runtime applies it at the start of
//!   the next frame, matching `FUNC_LOADSTAGE` + `ProcessStage`'s `STAGEMODE_LOAD`.

use retro_format_v4::{AnimationFile, Hitbox};
use retro_scene::collision::{
    C_PLATFORM, C_SOLID, C_SOLID2, C_TOUCH, CSIDE_FLOOR, CSIDE_LENTITY, CSIDE_LWALL, CSIDE_RENTITY,
    CSIDE_ROOF, CSIDE_RWALL,
};
use retro_scene::{ENTITY_COUNT, TEMPENTITY_START};
use retro_script::{Op, ScriptError, ScriptEvent, ScriptHost, VmState};

use crate::state::{ENGINE_MAINGAME, EngineState, LayerState, PARALLAX_COUNT, ScriptFrame};

/// First rev03 object variable id (`object.entityPos`).
const VAR_OBJECT_ENTITY_POS: i32 = 19;
/// Last rev03 object variable id (`object.spriteSheet`).
const VAR_OBJECT_LAST: i32 = 72;
/// First rev03 object value id (`object.value0`).
const VAR_VALUE0: i32 = 73;
/// Last rev03 object value id (`object.value47`).
const VAR_VALUE47: i32 = 120;
/// First rev03 stage variable id (`stage.state`).
const VAR_STAGE_FIRST: i32 = 121;
/// Last rev03 stage variable id (`stage.debugMode`).
const VAR_STAGE_LAST: i32 = 147;
/// First rev03 deformation table variable (`stage.deformationData0`).
const VAR_STAGE_DEFORM_FIRST: i32 = 139;
/// Last rev03 deformation table variable (`stage.deformationData3`).
const VAR_STAGE_DEFORM_LAST: i32 = 142;
/// `stage.entityPos`.
const VAR_STAGE_ENTITY_POS: i32 = 148;
/// First rev03 screen variable id (`screen.cameraEnabled`).
const VAR_SCREEN_FIRST: i32 = 149;
/// Last rev03 screen variable id (`screen.adjustCameraY`).
const VAR_SCREEN_LAST: i32 = 163;
/// `touchscreen.down`.
const VAR_TOUCH_DOWN: i32 = 164;
/// `music.volume`.
const VAR_MUSIC_VOLUME: i32 = 167;
/// `music.currentTrack`.
const VAR_MUSIC_TRACK: i32 = 168;
/// `music.position`.
const VAR_MUSIC_POSITION: i32 = 169;
/// `keyDown.up`.
const VAR_KEYDOWN_FIRST: i32 = 170;
/// `keyDown.select`.
const VAR_KEYDOWN_LAST: i32 = 183;
/// `keyPress.up`.
const VAR_KEYPRESS_FIRST: i32 = 184;
/// `keyPress.select`.
const VAR_KEYPRESS_LAST: i32 = 197;
/// `menu1.selection`.
const VAR_MENU1: i32 = 198;
/// `menu2.selection`.
const VAR_MENU2: i32 = 199;
/// First rev03 tile layer variable (`tileLayer.xsize`).
const VAR_TILELAYER_FIRST: i32 = 200;
/// Last rev03 tile layer variable (`tileLayer.deformationOffsetW`).
const VAR_TILELAYER_LAST: i32 = 211;
/// First rev03 horizontal parallax variable.
const VAR_HPARALLAX_FIRST: i32 = 212;
/// Last rev03 horizontal parallax variable.
const VAR_HPARALLAX_LAST: i32 = 214;
/// First rev03 vertical parallax variable.
const VAR_VPARALLAX_FIRST: i32 = 215;
/// Last rev03 vertical parallax variable.
const VAR_VPARALLAX_LAST: i32 = 217;
/// `engine.state`.
const VAR_ENGINE_STATE: i32 = 236;
/// `engine.language`.
const VAR_ENGINE_LANGUAGE: i32 = 237;
/// `engine.onlineActive`.
const VAR_ENGINE_ONLINE_ACTIVE: i32 = 238;
/// `engine.sfxVolume`.
const VAR_ENGINE_SFX_VOLUME: i32 = 239;
/// `engine.bgmVolume`.
const VAR_ENGINE_BGM_VOLUME: i32 = 240;
/// `saveRAM`.
const VAR_SAVE_RAM: i32 = 235;
/// `engine.trialMode`.
const VAR_ENGINE_TRIAL_MODE: i32 = 241;
/// `engine.deviceType`.
const VAR_ENGINE_DEVICE_TYPE: i32 = 242;
/// `screen.currentID`.
const VAR_SCREEN_CURRENT_ID: i32 = 243;
/// `camera.enabled`.
const VAR_CAMERA_FIRST: i32 = 244;
/// `camera.adjustY`.
const VAR_CAMERA_LAST: i32 = 249;
/// `engine.hapticsEnabled`.
const VAR_HAPTICS_ENABLED: i32 = 250;

/// `OBJECT_BORDER_Y1`.
const OBJECT_BORDER_Y1: i32 = 0x100;
/// `OBJECT_BORDER_Y3`.
const OBJECT_BORDER_Y3: i32 = 0x80;

/// `PRIORITY_BOUNDS_SMALL`.
const PRIORITY_BOUNDS_SMALL: u8 = 6;
/// `PRIORITY_ACTIVE_SMALL`.
const PRIORITY_ACTIVE_SMALL: u8 = 7;

/// `TILEINFO_INDEX`.
const TILEINFO_INDEX: i32 = 0;
/// `TILEINFO_DIRECTION`.
const TILEINFO_DIRECTION: i32 = 1;
/// `TILEINFO_VISUALPLANE`.
const TILEINFO_VISUALPLANE: i32 = 2;
/// `TILEINFO_SOLIDITYA`.
const TILEINFO_SOLIDITYA: i32 = 3;
/// `TILEINFO_SOLIDITYB`.
const TILEINFO_SOLIDITYB: i32 = 4;
/// `TILEINFO_FLAGSA`.
const TILEINFO_FLAGSA: i32 = 5;
/// `TILEINFO_ANGLEA`.
const TILEINFO_ANGLEA: i32 = 6;
/// `TILEINFO_FLAGSB`.
const TILEINFO_FLAGSB: i32 = 7;
/// `TILEINFO_ANGLEB`.
const TILEINFO_ANGLEB: i32 = 8;

/// Script host borrowing the mutable engine state.
pub struct EngineHost<'a> {
    /// Engine state.
    pub state: &'a mut EngineState,
}

impl EngineHost<'_> {
    fn hitbox(&self, slot: usize) -> Hitbox {
        self.state.hitbox_for(slot)
    }

    fn read_object_var(&self, var: i32, array_index: i32) -> i32 {
        let slot = usize::try_from(array_index).ok();
        let entity = slot
            .and_then(|slot| self.state.entities.get(slot))
            .copied()
            .unwrap_or_default();
        match var {
            VAR_OBJECT_ENTITY_POS => array_index,
            20 => i32::from(entity.group_id),
            21 => i32::from(entity.type_id),
            22 => i32::from(entity.property_value),
            23 => entity.xpos,
            24 => entity.ypos,
            25 => entity.xpos >> 16,
            26 => entity.ypos >> 16,
            27 => entity.xvel,
            28 => entity.yvel,
            29 => entity.speed,
            30 => entity.state,
            31 => entity.rotation,
            32 => entity.scale,
            33 => i32::from(entity.priority),
            34 => i32::from(entity.draw_order),
            35 => i32::from(entity.direction),
            36 => i32::from(entity.ink_effect),
            37 => entity.alpha,
            38 => i32::from(entity.frame),
            39 => i32::from(entity.animation),
            40 => i32::from(entity.prev_animation),
            41 => entity.animation_speed,
            42 => entity.animation_timer,
            43 => entity.angle,
            44 => entity.look_pos_x,
            45 => entity.look_pos_y,
            46 => i32::from(entity.collision_mode),
            47 => i32::from(entity.collision_plane),
            48 => i32::from(entity.control_mode),
            49 => i32::from(entity.control_lock),
            50 => i32::from(entity.pushing),
            51 => i32::from(entity.visible),
            52 => i32::from(entity.tile_collisions),
            53 => i32::from(entity.object_interactions),
            54 => i32::from(entity.gravity),
            55 => i32::from(entity.up),
            56 => i32::from(entity.down),
            57 => i32::from(entity.left),
            58 => i32::from(entity.right),
            59 => i32::from(entity.jump_press),
            60 => i32::from(entity.jump_hold),
            61 => i32::from(entity.scroll_tracking),
            62 => i32::from(entity.floor_sensors[0]),
            63 => i32::from(entity.floor_sensors[1]),
            64 => i32::from(entity.floor_sensors[2]),
            65 => i32::from(entity.floor_sensors[3]),
            66 => i32::from(entity.floor_sensors[4]),
            67 => i32::from(self.hitbox_or_zero(slot).left[0]),
            68 => i32::from(self.hitbox_or_zero(slot).top[0]),
            69 => i32::from(self.hitbox_or_zero(slot).right[0]),
            70 => i32::from(self.hitbox_or_zero(slot).bottom[0]),
            71 => i32::from(self.out_of_bounds(&entity)),
            72 => self
                .state
                .objects
                .get(usize::from(entity.type_id))
                .map(|entry| entry.sprite_sheet_id)
                .unwrap_or(0),
            _ => 0,
        }
    }

    fn hitbox_or_zero(&self, slot: Option<usize>) -> Hitbox {
        match slot {
            Some(slot) => self.hitbox(slot),
            None => crate::state::zero_hitbox(),
        }
    }

    fn out_of_bounds(&self, entity: &retro_scene::Entity) -> bool {
        let x = entity.xpos >> 16;
        let y = entity.ypos >> 16;
        let small =
            entity.priority == PRIORITY_BOUNDS_SMALL || entity.priority == PRIORITY_ACTIVE_SMALL;
        let (x1, x2, y1, y2) = if small {
            (
                self.state.object_borders[2],
                self.state.object_borders[3],
                OBJECT_BORDER_Y3,
                self.state.screen.ysize + OBJECT_BORDER_Y3,
            )
        } else {
            (
                self.state.object_borders[0],
                self.state.object_borders[1],
                OBJECT_BORDER_Y1,
                self.state.screen.ysize + OBJECT_BORDER_Y1,
            )
        };
        let bound_l = self.state.screen.x_scroll.wrapping_sub(x1);
        let bound_r = self.state.screen.x_scroll.wrapping_add(x2);
        let bound_t = self.state.screen.y_scroll.wrapping_sub(y1);
        let bound_b = self.state.screen.y_scroll.wrapping_add(y2);
        x <= bound_l || x >= bound_r || y <= bound_t || y >= bound_b
    }

    fn write_object_var(&mut self, var: i32, array_index: i32, value: i32) {
        let Ok(slot) = usize::try_from(array_index) else {
            return;
        };
        let Some(entity) = self.state.entities.get_mut(slot) else {
            return;
        };
        match var {
            20 => entity.group_id = value as u16,
            21 => entity.type_id = value as u8,
            22 => entity.property_value = value as u8,
            23 => entity.xpos = value,
            24 => entity.ypos = value,
            25 => entity.xpos = value << 16,
            26 => entity.ypos = value << 16,
            27 => entity.xvel = value,
            28 => entity.yvel = value,
            29 => entity.speed = value,
            30 => entity.state = value,
            31 => entity.rotation = value,
            32 => entity.scale = value,
            33 => entity.priority = value as u8,
            34 => entity.draw_order = value as u8,
            35 => entity.direction = value as u8,
            36 => entity.ink_effect = value as u8,
            37 => entity.alpha = value,
            38 => entity.frame = value as u8,
            39 => entity.animation = value as u8,
            40 => entity.prev_animation = value as u8,
            41 => entity.animation_speed = value,
            42 => entity.animation_timer = value,
            43 => entity.angle = value,
            44 => entity.look_pos_x = value,
            45 => entity.look_pos_y = value,
            46 => entity.collision_mode = value as u8,
            47 => entity.collision_plane = value as u8,
            48 => entity.control_mode = value as i8,
            49 => entity.control_lock = value as u8,
            50 => entity.pushing = value as u8,
            51 => entity.visible = value as u8,
            52 => entity.tile_collisions = value as u8,
            53 => entity.object_interactions = value as u8,
            54 => entity.gravity = value as u8,
            55 => entity.up = value as u8,
            56 => entity.down = value as u8,
            57 => entity.left = value as u8,
            58 => entity.right = value as u8,
            59 => entity.jump_press = value as u8,
            60 => entity.jump_hold = value as u8,
            61 => entity.scroll_tracking = value as u8,
            62 => entity.floor_sensors[0] = value as u8,
            63 => entity.floor_sensors[1] = value as u8,
            64 => entity.floor_sensors[2] = value as u8,
            65 => entity.floor_sensors[3] = value as u8,
            66 => entity.floor_sensors[4] = value as u8,
            _ => {}
        }
    }

    fn read_screen_var(&self, var: i32, array_index: i32) -> i32 {
        match var {
            149 => self.state.camera.enabled,
            150 => self.state.camera.target,
            151 => self.state.camera.style,
            152 => self.state.camera.xpos,
            153 => self.state.camera.ypos,
            154 => usize::try_from(array_index)
                .ok()
                .and_then(|index| self.state.draw_lists.get(index))
                .map(|list| list.len() as i32)
                .unwrap_or(0),
            155 => self.state.screen.center_x(),
            156 => self.state.screen.center_y(),
            157 => self.state.screen.xsize,
            158 => self.state.screen.ysize,
            159 => self.state.screen.x_scroll,
            160 => self.state.screen.y_scroll,
            161 => self.state.camera.shake_x,
            162 => self.state.camera.shake_y,
            163 => self.state.camera.adjust_y,
            _ => 0,
        }
    }

    fn write_screen_var(&mut self, var: i32, value: i32) {
        match var {
            149 => self.state.camera.enabled = value,
            150 => self.state.camera.target = value,
            151 => self.state.camera.style = value,
            152 => self.state.camera.xpos = value,
            153 => self.state.camera.ypos = value,
            159 => self.state.screen.x_scroll = value,
            160 => self.state.screen.y_scroll = value,
            161 => self.state.camera.shake_x = value,
            162 => self.state.camera.shake_y = value,
            163 => self.state.camera.adjust_y = value,
            _ => {}
        }
    }

    fn read_tile_layer_var(&self, var: i32, array_index: i32) -> i32 {
        let Ok(index) = usize::try_from(array_index) else {
            return 0;
        };
        let Some(layer) = self.state.layers.get(index) else {
            return 0;
        };
        match var {
            200 => layer.xsize,
            201 => layer.ysize,
            202 => layer.layer_type,
            203 => layer.angle,
            204 => layer.xpos,
            205 => layer.ypos,
            206 => layer.zpos,
            207 => layer.parallax_factor,
            208 => layer.scroll_speed,
            209 => layer.scroll_pos,
            210 => layer.deformation_offset,
            211 => layer.deformation_offset_w,
            _ => 0,
        }
    }

    fn write_tile_layer_var(&mut self, var: i32, array_index: i32, value: i32) {
        let Ok(index) = usize::try_from(array_index) else {
            return;
        };
        let Some(layer) = self.state.layers.get_mut(index) else {
            return;
        };
        match var {
            200 => layer.xsize = value,
            201 => layer.ysize = value,
            202 => layer.layer_type = value,
            203 => layer.angle = value,
            204 => layer.xpos = value,
            205 => layer.ypos = value,
            206 => layer.zpos = value,
            207 => layer.parallax_factor = value,
            208 => layer.scroll_speed = value,
            209 => layer.scroll_pos = value,
            210 => layer.deformation_offset = value,
            211 => layer.deformation_offset_w = value,
            _ => {}
        }
    }

    /// `stage.deformationData0..3` reads: upstream indexes `bgDeformationDataN[arrayVal]`
    /// (`Script.cpp:3958-3961`).
    fn read_stage_deform_var(&self, var: i32, array_index: i32) -> i32 {
        let Ok(index) = usize::try_from(array_index) else {
            return 0;
        };
        let table = usize::try_from(var - VAR_STAGE_DEFORM_FIRST).unwrap_or(0);
        self.state
            .render
            .deform_data
            .get(table)
            .and_then(|data| data.get(index))
            .copied()
            .unwrap_or(0)
    }

    /// `stage.deformationData0..3` writes (`Script.cpp:6128-6131`).
    fn write_stage_deform_var(&mut self, var: i32, array_index: i32, value: i32) {
        let Ok(index) = usize::try_from(array_index) else {
            return;
        };
        let table = usize::try_from(var - VAR_STAGE_DEFORM_FIRST).unwrap_or(0);
        if let Some(slot) = self
            .state
            .render
            .deform_data
            .get_mut(table)
            .and_then(|data| data.get_mut(index))
        {
            *slot = value;
        }
    }

    fn read_parallax_var(&self, var: i32, array_index: i32) -> i32 {
        let Ok(index) = usize::try_from(array_index) else {
            return 0;
        };
        let (table, field) = if (VAR_HPARALLAX_FIRST..=VAR_HPARALLAX_LAST).contains(&var) {
            (&self.state.h_parallax, var - VAR_HPARALLAX_FIRST)
        } else if (VAR_VPARALLAX_FIRST..=VAR_VPARALLAX_LAST).contains(&var) {
            (&self.state.v_parallax, var - VAR_VPARALLAX_FIRST)
        } else {
            return 0;
        };
        match field {
            0 => table.parallax_factor.get(index).copied().unwrap_or(0),
            1 => table.scroll_speed.get(index).copied().unwrap_or(0),
            _ => table.scroll_pos.get(index).copied().unwrap_or(0),
        }
    }

    fn write_parallax_var(&mut self, var: i32, array_index: i32, value: i32) {
        let Ok(index) = usize::try_from(array_index) else {
            return;
        };
        if index >= PARALLAX_COUNT {
            return;
        }
        let (table, field) = if (VAR_HPARALLAX_FIRST..=VAR_HPARALLAX_LAST).contains(&var) {
            (&mut self.state.h_parallax, var - VAR_HPARALLAX_FIRST)
        } else if (VAR_VPARALLAX_FIRST..=VAR_VPARALLAX_LAST).contains(&var) {
            (&mut self.state.v_parallax, var - VAR_VPARALLAX_FIRST)
        } else {
            return;
        };
        match field {
            0 => table.parallax_factor[index] = value,
            1 => table.scroll_speed[index] = value,
            _ => table.scroll_pos[index] = value,
        }
    }

    fn read_camera_var(&self, var: i32, array_index: i32) -> i32 {
        if array_index != 0 {
            return 0;
        }
        match var {
            VAR_CAMERA_FIRST => self.state.camera.enabled,
            245 => self.state.camera.target,
            246 => self.state.camera.style,
            247 => self.state.camera.xpos,
            248 => self.state.camera.ypos,
            VAR_CAMERA_LAST => self.state.camera.adjust_y,
            _ => 0,
        }
    }

    fn write_camera_var(&mut self, var: i32, array_index: i32, value: i32) {
        if array_index != 0 {
            return;
        }
        match var {
            VAR_CAMERA_FIRST => self.state.camera.enabled = value,
            245 => self.state.camera.target = value,
            246 => self.state.camera.style = value,
            247 => self.state.camera.xpos = value,
            248 => self.state.camera.ypos = value,
            VAR_CAMERA_LAST => self.state.camera.adjust_y = value,
            _ => {}
        }
    }

    fn adjust_camera_style(&mut self) {
        // The rev03 scripts set `screen.cameraStyle`; the legacy `screen.cameraEnabled`
        // globals alias the single camera.
        if self.state.camera.style < 0 {
            self.state.camera.style = 0;
        }
    }

    fn layer_mut(&mut self, index: i32) -> Option<&mut LayerState> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.state.layers.get_mut(index))
    }

    /// `FUNC_LOADANIMATION` implementation: read `Data/Animations/<name>` and assign it to the
    /// current object type.
    fn load_animation(&mut self, name: &str) {
        if let Some(index) = self.state.animation_ids.get(name).copied() {
            let type_id = self
                .state
                .entities
                .get(self.state.object_entity_pos)
                .map(|entity| usize::from(entity.type_id));
            if let Some(entry) = type_id.and_then(|type_id| self.state.objects.get_mut(type_id)) {
                entry.animation_file = Some(index);
            }
            return;
        }
        let path = format!("Data/Animations/{name}");
        let bytes = self.state.source.read(&path).ok();
        let Some(bytes) = bytes else {
            return;
        };
        let Ok(file) = AnimationFile::from_bytes(&bytes) else {
            return;
        };
        // Upstream `LoadAnimationFile` loads every sheet through `AddGraphicsFile` while
        // reading the animation, so the frame sheet ids are resolved at load time.
        let sheet_ids: Vec<i32> = file
            .sheets
            .iter()
            .map(|sheet| self.state.load_sprite_sheet(sheet))
            .collect();
        let index = self.state.animations.len();
        self.state.animation_ids.insert(name.to_owned(), index);
        self.state.animations.push(file);
        self.state.animation_sheet_ids.push(sheet_ids);
        let type_id = self
            .state
            .entities
            .get(self.state.object_entity_pos)
            .map(|entity| usize::from(entity.type_id));
        if let Some(entry) = type_id.and_then(|type_id| self.state.objects.get_mut(type_id)) {
            entry.animation_file = Some(index);
        }
    }

    /// `FUNC_PROCESSANIMATION` implementation.
    fn process_animation(&mut self) {
        let slot = self.state.object_entity_pos;
        let Some(mut entity) = self.state.entities.get(slot).copied() else {
            return;
        };
        let Some(entry) = self.state.objects.get(usize::from(entity.type_id)) else {
            return;
        };
        let Some(animation_index) = entry.animation_file else {
            return;
        };
        let Some(file) = self.state.animations.get(animation_index) else {
            return;
        };
        let Some(animation) = file.animations.get(usize::from(entity.animation)) else {
            return;
        };
        if entity.animation_speed <= 0 {
            entity.animation_timer += i32::from(animation.speed);
        } else {
            if entity.animation_speed > 0xF0 {
                entity.animation_speed = 0xF0;
            }
            entity.animation_timer += entity.animation_speed;
        }
        if entity.animation != entity.prev_animation {
            entity.prev_animation = entity.animation;
            entity.frame = 0;
            entity.animation_timer = 0;
            entity.animation_speed = 0;
        }
        if entity.animation_timer >= 0xF0 {
            entity.animation_timer -= 0xF0;
            entity.frame = entity.frame.wrapping_add(1);
        }
        if entity.frame >= animation.playback_frame_count {
            entity.frame = animation.loop_point;
        }
        if let Some(target) = self.state.entities.get_mut(slot) {
            *target = entity;
        }
    }

    fn process_object_control(&mut self) {
        let slot = self.state.object_entity_pos;
        let Some(mut entity) = self.state.entities.get(slot).copied() else {
            return;
        };
        if entity.control_mode == 0 {
            entity.up = u8::from(self.state.input.up);
            entity.down = u8::from(self.state.input.down);
            if !self.state.input.left || !self.state.input.right {
                entity.left = u8::from(self.state.input.left);
                entity.right = u8::from(self.state.input.right);
            } else {
                entity.left = 0;
                entity.right = 0;
            }
            entity.jump_hold = u8::from(
                self.state.input.button_c || self.state.input.button_b || self.state.input.button_a,
            );
            entity.jump_press = u8::from(
                self.state.input_press.button_c
                    || self.state.input_press.button_b
                    || self.state.input_press.button_a,
            );
        }
        if let Some(target) = self.state.entities.get_mut(slot) {
            *target = entity;
        }
    }
}

impl ScriptHost for EngineHost<'_> {
    fn engine_op(&mut self, op: Op, state: &mut VmState) -> Result<(), ScriptError> {
        let operands = state.operands;
        match op {
            Op::Rand => {
                state.operands[0] = crate::rng::glibc_rand_range(&mut self.state.rng, operands[1]);
                self.state.record_op("Rand");
            }
            Op::Sin => {
                state.operands[0] = self.state.math.sin512(operands[1]);
                self.state.record_op("Sin");
            }
            Op::Cos => {
                state.operands[0] = self.state.math.cos512(operands[1]);
                self.state.record_op("Cos");
            }
            Op::Sin256 => {
                state.operands[0] = self.state.math.sin256(operands[1]);
                self.state.record_op("Sin256");
            }
            Op::Cos256 => {
                state.operands[0] = self.state.math.cos256(operands[1]);
                self.state.record_op("Cos256");
            }
            Op::ATan2 => {
                state.operands[0] = self.state.math.atan2(operands[1], operands[2]);
                self.state.record_op("ATan2");
            }
            Op::LoadSpriteSheet => {
                let name = state.script_text.clone();
                self.state.record_op("LoadSpriteSheet");
                let index = self.state.load_sprite_sheet(&name);
                let type_id = self
                    .state
                    .entities
                    .get(self.state.object_entity_pos)
                    .map(|entity| usize::from(entity.type_id));
                if let Some(entry) = type_id.and_then(|type_id| self.state.objects.get_mut(type_id))
                {
                    entry.sprite_sheet_id = index;
                }
            }
            Op::RemoveSpriteSheet => {
                let name = state.script_text.clone();
                self.state.record_op("RemoveSpriteSheet");
                self.state.remove_sprite_sheet(&name);
            }
            Op::LoadAnimation => {
                let name = state.script_text.clone();
                self.state.record_op("LoadAnimation");
                self.load_animation(&name);
            }
            Op::SpriteFrame => {
                self.state.record_op("SpriteFrame");
                if state.current_event == ScriptEvent::Setup {
                    self.state.add_script_frame(ScriptFrame {
                        pivot_x: operands[0],
                        pivot_y: operands[1],
                        width: operands[2],
                        height: operands[3],
                        spr_x: operands[4],
                        spr_y: operands[5],
                    });
                }
            }
            Op::EditFrame => {
                self.state.record_op("EditFrame");
                self.state.edit_script_frame(
                    operands[0],
                    ScriptFrame {
                        pivot_x: operands[1],
                        pivot_y: operands[2],
                        width: operands[3],
                        height: operands[4],
                        spr_x: operands[5],
                        spr_y: operands[6],
                    },
                );
            }
            Op::LoadPalette => {
                let name = state.script_text.clone();
                self.state.record_op("LoadPalette");
                self.load_palette(&name, operands[1], operands[2], operands[3], operands[4]);
            }
            Op::RotatePalette => {
                self.state.record_op("RotatePalette");
                self.state.render.palette.rotate_palette(
                    operands[0],
                    usize::from(operands[1] as u8),
                    usize::from(operands[2] as u8),
                    operands[3] != 0,
                );
            }
            Op::SetScreenFade => {
                self.state.record_op("SetScreenFade");
                self.state.render.set_fade(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3] as u16,
                );
            }
            Op::SetActivePalette => {
                self.state.record_op("SetActivePalette");
                self.state.render.palette.set_active_palette(
                    i32::from(operands[0] as u8),
                    operands[1],
                    operands[2],
                );
            }
            Op::SetPaletteFade => {
                self.state.record_op("SetPaletteFade");
                self.state.render.palette.set_palette_fade(
                    i32::from(operands[0] as u8),
                    i32::from(operands[1] as u8),
                    i32::from(operands[2] as u8),
                    operands[3] as u16,
                    operands[4].max(0) as usize,
                    operands[5].max(0) as usize,
                );
            }
            Op::SetPaletteEntry => {
                self.state.record_op("SetPaletteEntry");
                self.state.render.palette.set_entry_packed(
                    i32::from(operands[0] as u8),
                    usize::from(operands[1] as u8),
                    operands[2] as u32,
                );
            }
            Op::GetPaletteEntry => {
                self.state.record_op("GetPaletteEntry");
                state.operands[2] =
                    self.state.render.palette.get_entry_packed(
                        i32::from(operands[0] as u8),
                        usize::from(operands[1] as u8),
                    ) as i32;
            }
            Op::CopyPalette => {
                self.state.record_op("CopyPalette");
                self.state.render.palette.copy_palette(
                    i32::from(operands[0] as u8),
                    usize::from(operands[1] as u8),
                    i32::from(operands[2] as u8),
                    usize::from(operands[3] as u8),
                    usize::from(operands[4] as u8),
                );
            }
            Op::ClearScreen => {
                self.state.record_op("ClearScreen");
                self.state.render.clear_screen(operands[0] as u8);
            }
            Op::DrawTintRect => {
                self.state.record_op("DrawTintRect");
                self.state.render.draw_tint_rect(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                );
            }
            Op::DrawRect => {
                self.state.record_op("DrawRect");
                self.state.render.draw_rect(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                    operands[6],
                    operands[7],
                );
            }
            Op::DrawSprite => {
                self.state.record_op("DrawSprite");
                self.draw_script_sprite(operands[0], SpriteDrawPosition::Entity, 0, 0);
            }
            Op::DrawSpriteXY => {
                self.state.record_op("DrawSpriteXY");
                self.draw_script_sprite(
                    operands[0],
                    SpriteDrawPosition::World,
                    operands[1],
                    operands[2],
                );
            }
            Op::DrawSpriteScreenXY => {
                self.state.record_op("DrawSpriteScreenXY");
                self.draw_script_sprite(
                    operands[0],
                    SpriteDrawPosition::Screen,
                    operands[1],
                    operands[2],
                );
            }
            Op::DrawSpriteFX => {
                self.state.record_op("DrawSpriteFX");
                self.draw_script_sprite_fx(
                    operands[0],
                    operands[1],
                    SpriteDrawPosition::World,
                    operands[2],
                    operands[3],
                );
            }
            Op::DrawSpriteScreenFX => {
                self.state.record_op("DrawSpriteScreenFX");
                self.draw_script_sprite_fx(
                    operands[0],
                    operands[1],
                    SpriteDrawPosition::Screen,
                    operands[2],
                    operands[3],
                );
            }
            Op::DrawObjectAnimation => {
                self.state.record_op("DrawObjectAnimation");
                let slot = self.state.object_entity_pos;
                let visible = self
                    .state
                    .entities
                    .get(slot)
                    .is_some_and(|entity| entity.visible != 0);
                if visible {
                    self.state.draw_object_animation(slot);
                }
            }
            Op::SetLayerDeformation => {
                self.state.record_op("SetLayerDeformation");
                self.set_layer_deformation(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                );
            }
            Op::GetAnimationByName => {
                let name = state.script_text.clone();
                let mut result = 0i32;
                let type_id = self
                    .state
                    .entities
                    .get(self.state.object_entity_pos)
                    .map(|entity| usize::from(entity.type_id));
                let animation_index = type_id
                    .and_then(|type_id| self.state.objects.get(type_id))
                    .and_then(|entry| entry.animation_file);
                if let Some(file) =
                    animation_index.and_then(|index| self.state.animations.get(index))
                {
                    for (index, animation) in file.animations.iter().enumerate() {
                        if animation.name == name {
                            result = index as i32;
                            break;
                        }
                    }
                }
                state.operands[0] = result;
                self.state.record_op("GetAnimationByName");
            }
            Op::ResetObjectEntity => {
                self.state.entities.reset_object_entity(
                    usize::try_from(operands[0]).unwrap_or(usize::MAX),
                    operands[1] as u8,
                    operands[2] as u8,
                    operands[3],
                    operands[4],
                );
                self.state.record_op("ResetObjectEntity");
            }
            Op::CreateTempObject => {
                let cursor = &mut state.array_position[8];
                let clamped = if (TEMPENTITY_START as i32..ENTITY_COUNT as i32).contains(cursor) {
                    *cursor as usize
                } else {
                    TEMPENTITY_START
                };
                let mut slot_cursor = clamped;
                self.state.entities.create_temp_object(
                    &mut slot_cursor,
                    operands[0] as u8,
                    operands[1] as u8,
                    operands[2],
                    operands[3],
                );
                state.array_position[8] = slot_cursor as i32;
                self.state.record_op("CreateTempObject");
            }
            Op::GetObjectValue => {
                let value = if operands[1] < 48 {
                    usize::try_from(operands[2])
                        .ok()
                        .and_then(|slot| self.state.entities.get(slot))
                        .and_then(|entity| {
                            usize::try_from(operands[1]).ok().map(|i| entity.values[i])
                        })
                        .unwrap_or(0)
                } else {
                    0
                };
                state.operands[0] = value;
                self.state.record_op("GetObjectValue");
            }
            Op::SetObjectValue => {
                if operands[1] < 48
                    && let Ok(value_index) = usize::try_from(operands[1])
                    && let Ok(slot) = usize::try_from(operands[2])
                    && let Some(entity) = self.state.entities.get_mut(slot)
                {
                    entity.values[value_index] = operands[0];
                }
                self.state.record_op("SetObjectValue");
            }
            Op::CopyObject => {
                self.state.entities.copy_objects(
                    usize::try_from(operands[0]).unwrap_or(usize::MAX),
                    usize::try_from(operands[1]).unwrap_or(usize::MAX),
                    operands[2],
                );
                self.state.record_op("CopyObject");
            }
            Op::BoxCollisionTest => {
                let result = self.box_collision_test(operands);
                state.check_result = result;
            }
            Op::ProcessObjectMovement => {
                let slot = self.state.object_entity_pos;
                let objects = &self.state.objects;
                let animations = &self.state.animations;
                let hitbox = |_slot: usize, entity: &retro_scene::Entity| {
                    crate::state::hitbox_from(objects, animations, entity)
                };
                let result = self.state.collision.as_mut().and_then(|collision| {
                    collision.process_object_movement(&mut self.state.entities, slot, &hitbox)
                });
                if let Some(result) = result {
                    state.check_result = result;
                }
                self.state.record_op("ProcessObjectMovement");
            }
            Op::ProcessObjectControl => {
                self.state.record_op("ProcessObjectControl");
                self.process_object_control();
            }
            Op::ProcessAnimation => {
                self.state.record_op("ProcessAnimation");
                self.process_animation();
            }
            Op::ObjectTileCollision => {
                self.state.record_op("ObjectTileCollision");
                state.check_result = i32::from(self.object_tile_collision(operands));
            }
            Op::ObjectTileGrip => {
                self.state.record_op("ObjectTileGrip");
                state.check_result = i32::from(self.object_tile_grip(operands));
            }
            Op::SetObjectRange => {
                let width = operands[0];
                let offset = (width >> 1).wrapping_sub(self.state.screen.center_x());
                self.state.object_borders = [
                    offset.wrapping_add(0x80),
                    width.wrapping_add(0x80).wrapping_sub(offset),
                    offset.wrapping_add(0x20),
                    width.wrapping_add(0x20).wrapping_sub(offset),
                ];
                self.state.record_op("SetObjectRange");
            }
            Op::CheckCameraProximity => {
                state.check_result = 0;
                if operands[2] > 0 && operands[3] > 0 {
                    let dx = operands[0]
                        .wrapping_sub(self.state.camera.xpos)
                        .wrapping_abs();
                    let dy = operands[1]
                        .wrapping_sub(self.state.camera.ypos)
                        .wrapping_abs();
                    state.check_result = i32::from(dx < operands[2] && dy < operands[3]);
                } else if operands[2] > 0 {
                    let dx = operands[0]
                        .wrapping_sub(self.state.camera.xpos)
                        .wrapping_abs();
                    state.check_result = i32::from(dx < operands[2]);
                } else if operands[3] > 0 {
                    let dy = operands[1]
                        .wrapping_sub(self.state.camera.ypos)
                        .wrapping_abs();
                    state.check_result = i32::from(dy < operands[3]);
                }
                self.state.record_op("CheckCameraProximity");
            }
            Op::CheckCurrentStageFolder => {
                let text = state.script_text.clone();
                let folder = self.state.stage_folder.clone();
                let mut result = folder == text;
                if !result && folder.len() > text.len() {
                    result = folder
                        .get(folder.len() - text.len()..)
                        .is_some_and(|suffix| suffix == text);
                }
                state.check_result = i32::from(result);
                self.state.record_op("CheckCurrentStageFolder");
            }
            Op::CheckTouchRect => {
                // Upstream starts at -1, scans every touch and keeps the LAST match
                // (`Script.cpp:5233-5242`).
                state.check_result = -1;
                for index in 0..self.state.touch_down.len() {
                    let down = self.state.touch_down[index] != 0;
                    let x = self.state.touch_x[index];
                    let y = self.state.touch_y[index];
                    if down
                        && x > operands[0]
                        && x < operands[2]
                        && y > operands[1]
                        && y < operands[3]
                    {
                        state.check_result = index as i32;
                    }
                }
                self.state.record_op("CheckTouchRect");
            }
            Op::GetTileLayerEntry => {
                let value = self
                    .state
                    .layers
                    .get(usize::try_from(operands[1]).unwrap_or(usize::MAX))
                    .map(|layer| layer.entry(operands[2], operands[3]))
                    .unwrap_or(0);
                state.operands[0] = i32::from(value);
                self.state.record_op("GetTileLayerEntry");
            }
            Op::SetTileLayerEntry => {
                if let Some(layer) = self.layer_mut(operands[1]) {
                    layer.set_entry(operands[2], operands[3], operands[0] as u16);
                }
                self.state.record_op("SetTileLayerEntry");
            }
            Op::GetBit => {
                state.operands[0] = (operands[1] & (1 << operands[2])) >> operands[2];
                self.state.record_op("GetBit");
            }
            Op::SetBit => {
                if operands[2] <= 0 {
                    state.operands[0] &= !(1 << operands[1]);
                } else {
                    state.operands[0] |= 1 << operands[1];
                }
                self.state.record_op("SetBit");
            }
            Op::ClearDrawList => {
                if let Some(list) = usize::try_from(operands[0])
                    .ok()
                    .and_then(|index| self.state.draw_lists.get_mut(index))
                {
                    list.clear();
                }
                self.state.record_op("ClearDrawList");
            }
            Op::AddDrawListEntityRef => {
                if let Some(list) = usize::try_from(operands[0])
                    .ok()
                    .and_then(|index| self.state.draw_lists.get_mut(index))
                {
                    list.push(operands[1]);
                }
                self.state.record_op("AddDrawListEntityRef");
            }
            Op::GetDrawListEntityRef => {
                let value = usize::try_from(operands[1])
                    .ok()
                    .and_then(|index| self.state.draw_lists.get(index))
                    .and_then(|list| usize::try_from(operands[2]).ok().and_then(|i| list.get(i)))
                    .copied()
                    .unwrap_or(0);
                state.operands[0] = value;
                self.state.record_op("GetDrawListEntityRef");
            }
            Op::SetDrawListEntityRef => {
                if let Some(list) = usize::try_from(operands[1])
                    .ok()
                    .and_then(|index| self.state.draw_lists.get_mut(index))
                    && let Some(slot) = usize::try_from(operands[2])
                        .ok()
                        .and_then(|index| list.get_mut(index))
                {
                    *slot = operands[0];
                }
                self.state.record_op("SetDrawListEntityRef");
            }
            Op::Get16x16TileInfo => {
                state.operands[4] = operands[1] >> 7;
                state.operands[5] = operands[2] >> 7;
                let chunk = self
                    .state
                    .layers
                    .first()
                    .map(|layer| layer.entry(state.operands[4], state.operands[5]))
                    .unwrap_or(0);
                let index = (usize::from(chunk) << 6)
                    + usize::try_from((operands[1] & 0x7F) >> 4).unwrap_or(0)
                    + 8 * usize::try_from((operands[2] & 0x7F) >> 4).unwrap_or(0);
                state.operands[6] = index as i32;
                let value = match operands[3] {
                    TILEINFO_INDEX => self
                        .state
                        .collision
                        .as_ref()
                        .and_then(|collision| collision.tiles.entries.get(index))
                        .map(|tile| i32::from(tile.tile_index))
                        .unwrap_or(0),
                    TILEINFO_DIRECTION => self
                        .state
                        .collision
                        .as_ref()
                        .and_then(|collision| collision.tiles.entries.get(index))
                        .map(|tile| i32::from(tile.direction))
                        .unwrap_or(0),
                    TILEINFO_VISUALPLANE => self
                        .state
                        .collision
                        .as_ref()
                        .and_then(|collision| collision.tiles.entries.get(index))
                        .map(|tile| i32::from(tile.visual_plane))
                        .unwrap_or(0),
                    TILEINFO_SOLIDITYA => self
                        .state
                        .collision
                        .as_ref()
                        .and_then(|collision| collision.tiles.entries.get(index))
                        .map(|tile| i32::from(tile.collision_flag_a))
                        .unwrap_or(0),
                    TILEINFO_SOLIDITYB => self
                        .state
                        .collision
                        .as_ref()
                        .and_then(|collision| collision.tiles.entries.get(index))
                        .map(|tile| i32::from(tile.collision_flag_b))
                        .unwrap_or(0),
                    TILEINFO_FLAGSA | TILEINFO_ANGLEA | TILEINFO_FLAGSB | TILEINFO_ANGLEB => {
                        let plane = if matches!(operands[3], TILEINFO_FLAGSA | TILEINFO_ANGLEA) {
                            0
                        } else {
                            1
                        };
                        let tile_index = self
                            .state
                            .collision
                            .as_ref()
                            .and_then(|collision| collision.tiles.entries.get(index))
                            .map(|tile| usize::from(tile.tile_index))
                            .unwrap_or(0);
                        let mask = self
                            .state
                            .collision
                            .as_ref()
                            .and_then(|collision| collision.mask_tile(plane, tile_index));
                        match operands[3] {
                            TILEINFO_FLAGSA | TILEINFO_FLAGSB => {
                                mask.map(|tile| i32::from(tile.flags)).unwrap_or(0)
                            }
                            _ => mask.map(|tile| tile.angle as i32).unwrap_or(0),
                        }
                    }
                    _ => 0,
                };
                state.operands[0] = value;
                self.state.record_op("Get16x16TileInfo");
            }
            Op::Set16x16TileInfo => {
                self.state.record_op("Set16x16TileInfo");
                let (chunk_x, chunk_y, chunk) =
                    self.set_16x16_tile_info(operands[0], operands[1], operands[2], operands[3]);
                state.operands[4] = chunk_x;
                state.operands[5] = chunk_y;
                state.operands[6] = chunk;
            }
            Op::Copy16x16Tile => {
                self.state.record_op("Copy16x16Tile");
                self.copy_16x16_tile(operands[0], operands[1]);
            }
            Op::ReadSaveRAM => {
                // `ReadSaveRAMData`: SData.bin, then SGame.bin; false when neither exists.
                self.state.record_op("ReadSaveRAM");
                state.check_result = i32::from(self.state.save.load_save_ram());
            }
            Op::WriteSaveRAM => {
                // `WriteSaveRAMData`: atomic write back to the file that was loaded.
                self.state.record_op("WriteSaveRAM");
                state.check_result = i32::from(self.state.save.write_save_ram());
            }
            Op::LoadStage => {
                // `FUNC_LOADSTAGE` only flips the stage mode (`stageMode = STAGEMODE_LOAD`); the
                // actual teardown/load happens at the start of the next `ProcessStage` call.
                self.state.record_op("LoadStage");
                self.state.load_stage_requested = true;
            }
            Op::GetTextInfo => {
                self.state.record_op("GetTextInfo");
                state.operands[0] = self.state.text_info(
                    usize::try_from(operands[1]).unwrap_or(usize::MAX),
                    operands[2],
                    operands[3],
                    operands[4],
                );
            }
            Op::LoadTextFile => {
                let path = state.script_text.clone();
                self.state.record_op("LoadTextFile");
                self.state.load_text_file(
                    usize::try_from(operands[0]).unwrap_or(usize::MAX),
                    &path,
                    operands[2] != 0,
                );
            }
            Op::LoadFontFile => {
                let path = state.script_text.clone();
                self.state.record_op("LoadFontFile");
                self.state.load_font_file(&path);
            }
            Op::DrawText => {
                self.state.record_op("DrawText");
                self.state.text_menu_surface_no = self.current_sheet_id();
                self.draw_bitmap_text(
                    usize::try_from(operands[0]).unwrap_or(usize::MAX),
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                    operands[6],
                );
            }
            Op::DrawNumbers => {
                self.state.record_op("DrawNumbers");
                self.draw_numbers(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                    operands[6] != 0,
                );
            }
            Op::DrawActName => {
                self.state.record_op("DrawActName");
                self.draw_act_name(
                    operands[0],
                    operands[1],
                    operands[2],
                    operands[3],
                    operands[4],
                    operands[5],
                    operands[6],
                );
            }
            Op::GetVersionNumber => {
                self.state.record_stub(stub_name(op));
            }
            Op::SetMusicTrack => {
                // Upstream: `operands[2] <= 1` is the loop flag with loop point 0, otherwise it
                // is the loop point and the track loops (`Script.cpp:5029-5035`).
                self.state.record_op("SetMusicTrack");
                let file = state.script_text.clone();
                let loop_operand = operands[2];
                if loop_operand <= 1 {
                    self.state
                        .audio
                        .set_track(operands[1], &file, loop_operand != 0, 0);
                } else {
                    self.state
                        .audio
                        .set_track(operands[1], &file, true, loop_operand);
                }
            }
            Op::PlayMusic => {
                self.state.record_op("PlayMusic");
                // Upstream sets `trackID = currentMusicTrack` inside `LoadMusic` only after the
                // Vorbis stream opens (`Audio.cpp:500-508`).
                if self.state.audio.play_music(operands[0]) {
                    self.state.music_track = operands[0];
                }
            }
            Op::StopMusic => {
                self.state.record_op("StopMusic");
                self.state.audio.stop_music();
            }
            Op::PauseMusic => {
                self.state.record_op("PauseMusic");
                self.state.audio.pause_music();
            }
            Op::ResumeMusic => {
                self.state.record_op("ResumeMusic");
                self.state.audio.resume_music();
            }
            Op::SwapMusicTrack => {
                // `operands[2]` is either the loop flag (<= 1) or the loop point; the ratio in
                // `operands[3]` is accepted but unused by the deterministic mixer.
                self.state.record_op("SwapMusicTrack");
                let file = state.script_text.clone();
                let loop_point = if operands[2] <= 1 { 0 } else { operands[2] };
                if self
                    .state
                    .audio
                    .swap_music_track(operands[1], &file, loop_point)
                {
                    self.state.music_track = operands[1];
                }
            }
            Op::PlaySfx => {
                self.state.record_op("PlaySfx");
                self.state.audio.play_sfx(operands[0], operands[1] != 0);
            }
            Op::StopSfx => {
                self.state.record_op("StopSfx");
                self.state.audio.stop_sfx(operands[0]);
            }
            Op::SetSfxAttributes => {
                self.state.record_op("SetSfxAttributes");
                self.state
                    .audio
                    .set_sfx_attributes(operands[0], operands[1], operands[2]);
            }
            Op::CallNativeFunction | Op::CallNativeFunction2 | Op::CallNativeFunction4 => {
                self.state.record_stub(stub_name(op));
            }
            Op::Print => {
                self.state.record_stub(stub_name(op));
            }
            Op::DrawMenu
            | Op::Draw3DScene
            | Op::SetupMenu
            | Op::AddMenuEntry
            | Op::EditMenuEntry
            | Op::SetIdentityMatrix
            | Op::MatrixMultiply
            | Op::MatrixTranslateXYZ
            | Op::MatrixScaleXYZ
            | Op::MatrixRotateX
            | Op::MatrixRotateY
            | Op::MatrixRotateZ
            | Op::MatrixRotateXYZ
            | Op::MatrixInverse
            | Op::TransformVertices
            | Op::SetScreenCount
            | Op::SetScreenVertices
            | Op::GetInputDeviceID
            | Op::GetFilteredInputDeviceID
            | Op::GetInputDeviceType
            | Op::IsInputDeviceAssigned
            | Op::AssignInputSlotToDevice
            | Op::IsInputSlotAssigned
            | Op::ResetInputSlotAssignments => {
                self.state.record_stub(stub_name(op));
            }
            _ => {
                self.state.record_stub(stub_name(op));
            }
        }
        Ok(())
    }

    fn read_engine_var(
        &mut self,
        var: i32,
        array_index: i32,
        _state: &mut VmState,
    ) -> Result<i32, ScriptError> {
        let value = if (VAR_OBJECT_ENTITY_POS..=VAR_OBJECT_LAST).contains(&var) {
            self.read_object_var(var, array_index)
        } else if (VAR_VALUE0..=VAR_VALUE47).contains(&var) {
            usize::try_from(array_index)
                .ok()
                .and_then(|slot| self.state.entities.get(slot))
                .and_then(|entity| {
                    usize::try_from(var - VAR_VALUE0)
                        .ok()
                        .map(|index| entity.values[index])
                })
                .unwrap_or(0)
        } else if (VAR_STAGE_DEFORM_FIRST..=VAR_STAGE_DEFORM_LAST).contains(&var) {
            self.read_stage_deform_var(var, array_index)
        } else if (VAR_STAGE_FIRST..=VAR_STAGE_LAST).contains(&var) {
            self.state.stage.read(var, array_index).unwrap_or(0)
        } else if var == VAR_STAGE_ENTITY_POS {
            self.state.object_entity_pos as i32
        } else if (VAR_SCREEN_FIRST..=VAR_SCREEN_LAST).contains(&var) {
            self.read_screen_var(var, array_index)
        } else if var == VAR_TOUCH_DOWN {
            usize::try_from(array_index)
                .ok()
                .and_then(|index| self.state.touch_down.get(index))
                .copied()
                .unwrap_or(0)
        } else if var == VAR_TOUCH_DOWN + 1 {
            usize::try_from(array_index)
                .ok()
                .and_then(|index| self.state.touch_x.get(index))
                .copied()
                .unwrap_or(0)
        } else if var == VAR_TOUCH_DOWN + 2 {
            usize::try_from(array_index)
                .ok()
                .and_then(|index| self.state.touch_y.get(index))
                .copied()
                .unwrap_or(0)
        } else if var == VAR_SAVE_RAM {
            self.state.save.read_word(array_index)
        } else if var == VAR_MUSIC_VOLUME {
            // `masterVolume` (`SetMusicVolume`).
            i32::from(self.state.audio.music_volume())
        } else if var == VAR_MUSIC_TRACK {
            self.state.music_track
        } else if var == VAR_MUSIC_POSITION {
            // `musicPosition` / `ov_pcm_tell`, in source PCM frames.
            self.state.audio.music_position() as i32
        } else if (VAR_KEYDOWN_FIRST..=VAR_KEYDOWN_LAST).contains(&var) {
            // Rev03 Origins: `inputCheck = arrayVal <= 1`, so higher slots read false.
            i32::from(array_index <= 1 && self.state.input.down(var).unwrap_or(false))
        } else if (VAR_KEYPRESS_FIRST..=VAR_KEYPRESS_LAST).contains(&var) {
            i32::from(array_index <= 1 && self.state.input_press.press(var).unwrap_or(false))
        } else if var == VAR_MENU1 {
            self.state.menu1_selection
        } else if var == VAR_MENU2 {
            self.state.menu2_selection
        } else if (VAR_TILELAYER_FIRST..=VAR_TILELAYER_LAST).contains(&var) {
            self.read_tile_layer_var(var, array_index)
        } else if (VAR_HPARALLAX_FIRST..=VAR_VPARALLAX_LAST).contains(&var) {
            self.read_parallax_var(var, array_index)
        } else if var == VAR_ENGINE_STATE {
            ENGINE_MAINGAME
        } else if var == VAR_ENGINE_SFX_VOLUME {
            i32::from(self.state.audio.sfx_volume())
        } else if var == VAR_ENGINE_BGM_VOLUME {
            i32::from(self.state.audio.music_volume())
        } else if (VAR_CAMERA_FIRST..=VAR_CAMERA_LAST).contains(&var) {
            self.read_camera_var(var, array_index)
        } else {
            let _ = (
                VAR_ENGINE_LANGUAGE,
                VAR_ENGINE_ONLINE_ACTIVE,
                VAR_ENGINE_TRIAL_MODE,
                VAR_ENGINE_DEVICE_TYPE,
                VAR_SCREEN_CURRENT_ID,
                VAR_HAPTICS_ENABLED,
            );
            0
        };
        Ok(value)
    }

    fn write_engine_var(
        &mut self,
        var: i32,
        array_index: i32,
        value: i32,
        _state: &mut VmState,
    ) -> Result<(), ScriptError> {
        if (VAR_OBJECT_ENTITY_POS..=VAR_OBJECT_LAST).contains(&var) {
            self.write_object_var(var, array_index, value);
        } else if (VAR_VALUE0..=VAR_VALUE47).contains(&var) {
            if let Ok(slot) = usize::try_from(array_index)
                && let Ok(index) = usize::try_from(var - VAR_VALUE0)
                && let Some(entity) = self.state.entities.get_mut(slot)
            {
                entity.values[index] = value;
            }
        } else if (VAR_STAGE_DEFORM_FIRST..=VAR_STAGE_DEFORM_LAST).contains(&var) {
            self.write_stage_deform_var(var, array_index, value);
        } else if (VAR_STAGE_FIRST..=VAR_STAGE_LAST).contains(&var) {
            self.state.stage.write(var, array_index, value);
        } else if (VAR_SCREEN_FIRST..=VAR_SCREEN_LAST).contains(&var) {
            self.write_screen_var(var, value);
        } else if (VAR_TILELAYER_FIRST..=VAR_TILELAYER_LAST).contains(&var) {
            self.write_tile_layer_var(var, array_index, value);
        } else if (VAR_HPARALLAX_FIRST..=VAR_VPARALLAX_LAST).contains(&var) {
            self.write_parallax_var(var, array_index, value);
        } else if (VAR_CAMERA_FIRST..=VAR_CAMERA_LAST).contains(&var) {
            self.write_camera_var(var, array_index, value);
        } else if var == VAR_SAVE_RAM {
            // `saveRAM[arrayVal] = value`; persistence happens on `WriteSaveRAM`.
            self.state.save.write_word(array_index, value);
        } else if var == VAR_MUSIC_VOLUME {
            // `SetMusicVolume` (`Audio.cpp:240`); `music.position` stays read-only.
            self.state.audio.set_music_volume_level(value);
        } else if var == VAR_ENGINE_SFX_VOLUME {
            self.state.audio.set_sfx_volume_level(value);
        } else if var == VAR_ENGINE_BGM_VOLUME {
            self.state.audio.set_music_volume_level(value);
        }
        // Input and touchscreen globals are read-only.
        if (VAR_SCREEN_FIRST..=VAR_SCREEN_LAST).contains(&var) {
            self.adjust_camera_style();
        }
        Ok(())
    }

    fn object_entity_pos(&self) -> i32 {
        self.state.object_entity_pos as i32
    }

    fn foreach_next(
        &mut self,
        op: Op,
        selector: i32,
        loop_index: i32,
        event: ScriptEvent,
        _state: &mut VmState,
    ) -> Result<Option<i32>, ScriptError> {
        match op {
            Op::ForEachActive => {
                let list = usize::try_from(selector)
                    .ok()
                    .and_then(|index| self.state.type_groups.get(index));
                let index = usize::try_from(loop_index).ok();
                Ok(list
                    .zip(index)
                    .and_then(|(list, index)| list.entity_refs.get(index).copied()))
            }
            Op::ForEachAll => {
                if selector < 0 || selector >= retro_scene::OBJECT_COUNT as i32 {
                    return Ok(None);
                }
                let bound = if event == ScriptEvent::Setup {
                    TEMPENTITY_START
                } else {
                    ENTITY_COUNT
                };
                let mut index = usize::try_from(loop_index).unwrap_or(0);
                while index < bound {
                    let matches = self
                        .state
                        .entities
                        .get(index)
                        .is_some_and(|entity| i32::from(entity.type_id) == selector);
                    if matches {
                        return Ok(Some(index as i32));
                    }
                    index += 1;
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

/// Human-readable static name for an op, used by the stub histogram.
fn stub_name(op: Op) -> &'static str {
    match op {
        Op::Set16x16TileInfo => "Set16x16TileInfo",
        Op::Copy16x16Tile => "Copy16x16Tile",
        Op::GetPaletteEntry => "GetPaletteEntry",
        Op::SetPaletteEntry => "SetPaletteEntry",
        Op::ReadSaveRAM => "ReadSaveRAM",
        Op::WriteSaveRAM => "WriteSaveRAM",
        Op::GetTextInfo => "GetTextInfo",
        Op::LoadTextFile => "LoadTextFile",
        Op::LoadFontFile => "LoadFontFile",
        Op::DrawText => "DrawText",
        Op::GetVersionNumber => "GetVersionNumber",
        Op::SetMusicTrack => "SetMusicTrack",
        Op::PlayMusic => "PlayMusic",
        Op::StopMusic => "StopMusic",
        Op::PauseMusic => "PauseMusic",
        Op::ResumeMusic => "ResumeMusic",
        Op::SwapMusicTrack => "SwapMusicTrack",
        Op::PlaySfx => "PlaySfx",
        Op::StopSfx => "StopSfx",
        Op::SetSfxAttributes => "SetSfxAttributes",
        Op::CallNativeFunction => "CallNativeFunction",
        Op::CallNativeFunction2 => "CallNativeFunction2",
        Op::CallNativeFunction4 => "CallNativeFunction4",
        Op::Print => "Print",
        Op::DrawSprite => "DrawSprite",
        Op::DrawSpriteXY => "DrawSpriteXY",
        Op::DrawSpriteScreenXY => "DrawSpriteScreenXY",
        Op::DrawTintRect => "DrawTintRect",
        Op::DrawNumbers => "DrawNumbers",
        Op::DrawActName => "DrawActName",
        Op::DrawMenu => "DrawMenu",
        Op::DrawRect => "DrawRect",
        Op::DrawSpriteFX => "DrawSpriteFX",
        Op::DrawSpriteScreenFX => "DrawSpriteScreenFX",
        Op::Draw3DScene => "Draw3DScene",
        Op::DrawObjectAnimation => "DrawObjectAnimation",
        Op::SpriteFrame => "SpriteFrame",
        Op::EditFrame => "EditFrame",
        Op::LoadPalette => "LoadPalette",
        Op::RotatePalette => "RotatePalette",
        Op::SetScreenFade => "SetScreenFade",
        Op::SetActivePalette => "SetActivePalette",
        Op::SetPaletteFade => "SetPaletteFade",
        Op::CopyPalette => "CopyPalette",
        Op::ClearScreen => "ClearScreen",
        Op::SetupMenu => "SetupMenu",
        Op::AddMenuEntry => "AddMenuEntry",
        Op::EditMenuEntry => "EditMenuEntry",
        Op::RemoveSpriteSheet => "RemoveSpriteSheet",
        Op::SetIdentityMatrix => "SetIdentityMatrix",
        Op::MatrixMultiply => "MatrixMultiply",
        Op::MatrixTranslateXYZ => "MatrixTranslateXYZ",
        Op::MatrixScaleXYZ => "MatrixScaleXYZ",
        Op::MatrixRotateX => "MatrixRotateX",
        Op::MatrixRotateY => "MatrixRotateY",
        Op::MatrixRotateZ => "MatrixRotateZ",
        Op::MatrixRotateXYZ => "MatrixRotateXYZ",
        Op::MatrixInverse => "MatrixInverse",
        Op::TransformVertices => "TransformVertices",
        Op::SetLayerDeformation => "SetLayerDeformation",
        Op::SetScreenCount => "SetScreenCount",
        Op::SetScreenVertices => "SetScreenVertices",
        Op::GetInputDeviceID => "GetInputDeviceID",
        Op::GetFilteredInputDeviceID => "GetFilteredInputDeviceID",
        Op::GetInputDeviceType => "GetInputDeviceType",
        Op::IsInputDeviceAssigned => "IsInputDeviceAssigned",
        Op::AssignInputSlotToDevice => "AssignInputSlotToDevice",
        Op::IsInputSlotAssigned => "IsInputSlotAssigned",
        Op::ResetInputSlotAssignments => "ResetInputSlotAssignments",
        _ => "Unimplemented",
    }
}

impl EngineHost<'_> {
    fn box_collision_test(&mut self, operands: [i32; 16]) -> i32 {
        let collision_type = operands[0];
        let this_slot = usize::try_from(operands[1]).unwrap_or(usize::MAX);
        let other_slot = usize::try_from(operands[6]).unwrap_or(usize::MAX);
        match collision_type {
            C_TOUCH => {
                self.state.record_op("BoxCollisionTest");
                let hit =
                    self.with_collision_entities(|collision, entities, objects, animations| {
                        let hitbox = |_slot: usize, entity: &retro_scene::Entity| {
                            crate::state::hitbox_from(objects, animations, entity)
                        };
                        collision.touch_collision(
                            entities,
                            this_slot,
                            operands[2],
                            operands[3],
                            operands[4],
                            operands[5],
                            other_slot,
                            operands[7],
                            operands[8],
                            operands[9],
                            operands[10],
                            &hitbox,
                        )
                    });
                i32::from(hit)
            }
            C_SOLID => {
                self.state.record_op("BoxCollisionTest");
                self.with_collision_entities(|collision, entities, objects, animations| {
                    let hitbox = |_slot: usize, entity: &retro_scene::Entity| {
                        crate::state::hitbox_from(objects, animations, entity)
                    };
                    collision.box_collision(
                        entities,
                        this_slot,
                        operands[2],
                        operands[3],
                        operands[4],
                        operands[5],
                        other_slot,
                        operands[7],
                        operands[8],
                        operands[9],
                        operands[10],
                        &hitbox,
                    )
                })
            }
            C_PLATFORM => {
                self.state.record_op("BoxCollisionTest");
                self.with_collision_entities(|collision, entities, objects, animations| {
                    let hitbox = |_slot: usize, entity: &retro_scene::Entity| {
                        crate::state::hitbox_from(objects, animations, entity)
                    };
                    collision.platform_collision(
                        entities,
                        this_slot,
                        operands[2],
                        operands[3],
                        operands[4],
                        operands[5],
                        other_slot,
                        operands[7],
                        operands[8],
                        operands[9],
                        operands[10],
                        &hitbox,
                    )
                })
            }
            C_SOLID2 => {
                // `BoxCollision2` is a separate ~300-line routine that upstream itself notes
                // is "barely used in S2"; it is an explicit M3 stub.
                self.state.record_stub("BoxCollision2");
                0
            }
            _ => 0,
        }
    }

    fn with_collision_entities<T>(
        &mut self,
        run: impl FnOnce(
            &mut retro_scene::SceneCollision,
            &mut retro_scene::EntityStore,
            &retro_scene::ObjectRegistry,
            &[AnimationFile],
        ) -> T,
    ) -> T
    where
        T: Default,
    {
        let Some(collision) = self.state.collision.as_mut() else {
            return T::default();
        };
        run(
            collision,
            &mut self.state.entities,
            &self.state.objects,
            &self.state.animations,
        )
    }

    fn object_tile_collision(&mut self, operands: [i32; 16]) -> bool {
        let slot = self.state.object_entity_pos;
        let side = operands[0];
        let Some(collision) = self.state.collision.as_mut() else {
            return false;
        };
        let entities = &mut self.state.entities;
        match side {
            CSIDE_FLOOR => collision.object_floor_collision(
                entities,
                slot,
                operands[1],
                operands[2],
                operands[3] as usize,
            ),
            CSIDE_LWALL => collision.object_lwall_collision(
                entities,
                slot,
                operands[1],
                operands[2],
                operands[3] as usize,
            ),
            CSIDE_RWALL => collision.object_rwall_collision(
                entities,
                slot,
                operands[1] - 1,
                operands[2],
                operands[3] as usize,
            ),
            CSIDE_ROOF => collision.object_roof_collision(
                entities,
                slot,
                operands[1],
                operands[2] - 1,
                operands[3] as usize,
            ),
            CSIDE_LENTITY => {
                let plane = usize::try_from(operands[1])
                    .ok()
                    .and_then(|other| entities.get(other))
                    .map(|entity| usize::from(entity.collision_plane))
                    .unwrap_or(0);
                collision.object_lwall_collision(entities, slot, operands[2], 0, plane)
            }
            CSIDE_RENTITY => {
                let plane = usize::try_from(operands[1])
                    .ok()
                    .and_then(|other| entities.get(other))
                    .map(|entity| usize::from(entity.collision_plane))
                    .unwrap_or(0);
                collision.object_lwall_collision(entities, slot, operands[2] - 1, 0, plane)
            }
            _ => false,
        }
    }

    fn object_tile_grip(&mut self, operands: [i32; 16]) -> bool {
        let slot = self.state.object_entity_pos;
        let side = operands[0];
        let groups = std::mem::take(&mut self.state.type_groups);
        let result = {
            let Some(collision) = self.state.collision.as_mut() else {
                self.state.type_groups = groups;
                return false;
            };
            let entities = &mut self.state.entities;
            match side {
                CSIDE_FLOOR => collision.object_floor_grip(
                    entities,
                    slot,
                    operands[1],
                    operands[2],
                    operands[3] as usize,
                ),
                CSIDE_LWALL => collision.object_lwall_grip(
                    entities,
                    slot,
                    operands[1],
                    operands[2],
                    operands[3] as usize,
                ),
                CSIDE_RWALL => collision.object_rwall_grip(
                    entities,
                    slot,
                    operands[1] - 1,
                    operands[2],
                    operands[3] as usize,
                ),
                CSIDE_ROOF => collision.object_roof_grip(
                    entities,
                    slot,
                    operands[1],
                    operands[2] - 1,
                    operands[3] as usize,
                ),
                CSIDE_LENTITY => collision.object_lentity_grip(
                    entities,
                    slot,
                    operands[1],
                    operands[2],
                    operands[3] as usize,
                    &groups,
                ),
                CSIDE_RENTITY => collision.object_rentity_grip(
                    entities,
                    slot,
                    operands[1],
                    operands[2],
                    operands[3] as usize,
                    &groups,
                ),
                _ => false,
            }
        };
        self.state.type_groups = groups;
        result
    }
}

/// Where a script sprite draw takes its position from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteDrawPosition {
    /// `DrawSprite`: the current entity's position.
    Entity,
    /// `DrawSpriteXY`: an operand position in 16.16 world space.
    World,
    /// `DrawSpriteScreenXY`: an operand position in screen pixels.
    Screen,
}

impl EngineHost<'_> {
    fn current_entity(&self) -> retro_scene::Entity {
        self.state
            .entities
            .get(self.state.object_entity_pos)
            .copied()
            .unwrap_or_default()
    }

    fn current_sheet_id(&self) -> i32 {
        let entity = self.current_entity();
        self.state
            .objects
            .get(usize::from(entity.type_id))
            .map(|entry| entry.sprite_sheet_id)
            .unwrap_or(0)
    }

    fn sprite_base(&self, position: SpriteDrawPosition, x: i32, y: i32) -> (i32, i32) {
        match position {
            SpriteDrawPosition::Entity => {
                let entity = self.current_entity();
                (
                    (entity.xpos >> 16) - self.state.screen.x_scroll,
                    (entity.ypos >> 16) - self.state.screen.y_scroll,
                )
            }
            SpriteDrawPosition::World => (
                (x >> 16) - self.state.screen.x_scroll,
                (y >> 16) - self.state.screen.y_scroll,
            ),
            SpriteDrawPosition::Screen => (x, y),
        }
    }

    /// `DrawSprite`/`DrawSpriteXY`/`DrawSpriteScreenXY`.
    fn draw_script_sprite(
        &mut self,
        frame_index: i32,
        position: SpriteDrawPosition,
        x: i32,
        y: i32,
    ) {
        let frame = self.state.script_frame(frame_index);
        let (base_x, base_y) = self.sprite_base(position, x, y);
        let sheet = self.current_sheet_id();
        self.state.render.draw_sprite(
            sheet,
            base_x + frame.pivot_x,
            base_y + frame.pivot_y,
            frame.width,
            frame.height,
            frame.spr_x,
            frame.spr_y,
        );
    }

    /// `DrawSpriteFX`/`DrawSpriteScreenFX`.
    fn draw_script_sprite_fx(
        &mut self,
        frame_index: i32,
        fx: i32,
        position: SpriteDrawPosition,
        x: i32,
        y: i32,
    ) {
        let frame = self.state.script_frame(frame_index);
        let entity = self.current_entity();
        let (base_x, base_y) = self.sprite_base(position, x, y);
        let sheet = self.current_sheet_id();
        match fx {
            0 => self.state.render.draw_sprite_scaled(
                sheet,
                entity.direction,
                base_x,
                base_y,
                -frame.pivot_x,
                -frame.pivot_y,
                entity.scale,
                entity.scale,
                frame.width,
                frame.height,
                frame.spr_x,
                frame.spr_y,
            ),
            1 => self.state.render.draw_sprite_rotated(
                sheet,
                entity.direction,
                base_x,
                base_y,
                -frame.pivot_x,
                -frame.pivot_y,
                frame.spr_x,
                frame.spr_y,
                frame.width,
                frame.height,
                entity.rotation,
            ),
            2 => self.state.render.draw_sprite_rotozoom(
                sheet,
                entity.direction,
                base_x,
                base_y,
                -frame.pivot_x,
                -frame.pivot_y,
                frame.spr_x,
                frame.spr_y,
                frame.width,
                frame.height,
                entity.rotation,
                entity.scale,
            ),
            3 => {
                let draw_x = base_x + frame.pivot_x;
                let draw_y = base_y + frame.pivot_y;
                match entity.ink_effect {
                    0 => self.state.render.draw_sprite(
                        sheet,
                        draw_x,
                        draw_y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                    ),
                    1 => self.state.render.draw_blended_sprite(
                        sheet,
                        draw_x,
                        draw_y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                    ),
                    2 => self.state.render.draw_alpha_blended_sprite(
                        sheet,
                        draw_x,
                        draw_y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                        entity.alpha,
                    ),
                    3 => self.state.render.draw_additive_blended_sprite(
                        sheet,
                        draw_x,
                        draw_y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                        entity.alpha,
                    ),
                    4 => self.state.render.draw_subtractive_blended_sprite(
                        sheet,
                        draw_x,
                        draw_y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                        entity.alpha,
                    ),
                    _ => {}
                }
            }
            4 => {
                // Upstream only uses the tint mask under `INK_ALPHA` (`Script.cpp:4778`).
                if entity.ink_effect == 2 {
                    self.state.render.draw_scaled_tint_mask(
                        sheet,
                        entity.direction,
                        base_x,
                        base_y,
                        -frame.pivot_x,
                        -frame.pivot_y,
                        entity.scale,
                        entity.scale,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                    );
                } else {
                    self.state.render.draw_sprite_scaled(
                        sheet,
                        entity.direction,
                        base_x,
                        base_y,
                        -frame.pivot_x,
                        -frame.pivot_y,
                        entity.scale,
                        entity.scale,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                    );
                }
            }
            5 => {
                let (draw_x, draw_y, direction) = match entity.direction {
                    1 => (
                        base_x - frame.width - frame.pivot_x,
                        base_y + frame.pivot_y,
                        retro_render::FLIP_X,
                    ),
                    2 => (
                        base_x + frame.pivot_x,
                        base_y - frame.height - frame.pivot_y,
                        retro_render::FLIP_Y,
                    ),
                    3 => (
                        base_x - frame.width - frame.pivot_x,
                        base_y - frame.height - frame.pivot_y,
                        retro_render::FLIP_XY,
                    ),
                    _ => (
                        base_x + frame.pivot_x,
                        base_y + frame.pivot_y,
                        retro_render::FLIP_NONE,
                    ),
                };
                self.state.render.draw_sprite_flipped(
                    sheet,
                    draw_x,
                    draw_y,
                    frame.width,
                    frame.height,
                    frame.spr_x,
                    frame.spr_y,
                    direction,
                );
            }
            _ => {}
        }
    }

    /// `LoadPalette`: reads `Data/Palettes/<name>` and writes `start..end` into a bank.
    fn load_palette(
        &mut self,
        name: &str,
        palette_id: i32,
        start_palette_index: i32,
        start_index: i32,
        end_index: i32,
    ) {
        let path = format!("Data/Palettes/{name}");
        let bytes = self.state.source.read(&path).ok();
        let Some(bytes) = bytes else {
            return;
        };
        let Ok(palette) = retro_format_v4::Palette::from_bytes(&bytes) else {
            return;
        };
        let colors: Vec<[u8; 3]> = (start_index..end_index)
            .map(|index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| palette.colors.get(index).copied())
                    .unwrap_or([0, 0, 0])
            })
            .collect();
        if colors.is_empty() {
            return;
        }
        // Upstream `LoadPalette` maps 0 and out-of-range ids to the active bank.
        let bank = if palette_id > 0 && palette_id < retro_render::PALETTE_BANKS as i32 {
            palette_id
        } else {
            retro_render::ACTIVE_PALETTE
        };
        self.state.render.palette.set_bank_entries(
            bank,
            usize::try_from(start_palette_index)
                .unwrap_or(0)
                .min(retro_render::PALETTE_COLORS - 1),
            &colors,
        );
    }

    /// `SetLayerDeformation`.
    fn set_layer_deformation(
        &mut self,
        selected_def: i32,
        wave_length: i32,
        wave_width: i32,
        wave_type: i32,
        y_pos: i32,
        wave_size: i32,
    ) {
        let Ok(table) = usize::try_from(selected_def) else {
            return;
        };
        if table >= 4 {
            return;
        }
        let wave_length = wave_length.max(1);
        let shift = 9;
        if wave_type == 1 {
            for (id, offset) in (y_pos.max(0)..).zip(0..wave_size.max(0)) {
                let angle = ((offset << 9) / wave_length) & 0x1FF;
                let value = wave_width.wrapping_mul(self.state.math.sin512(angle)) >> shift;
                if let Ok(id) = usize::try_from(id)
                    && let Some(slot) = self.state.render.deform_data[table].get_mut(id)
                {
                    *slot = value;
                }
            }
        } else {
            let mut id = 0i32;
            let mut angle_index = 0i32;
            while angle_index < 0x200 * 0x100 {
                let angle = (angle_index / wave_length) & 0x1FF;
                let mut value = wave_width.wrapping_mul(self.state.math.sin512(angle)) >> shift;
                if value >= wave_width {
                    value = wave_width - 1;
                }
                if let Ok(id) = usize::try_from(id)
                    && let Some(slot) = self.state.render.deform_data[table].get_mut(id)
                {
                    *slot = value;
                }
                id += 1;
                angle_index += 0x200;
            }
        }
        // Upstream mirrors the first `DEFORM_STORE` entries into the tail so the deformation
        // pointer can run past the wave length (`Scene.cpp:1450-1462`).
        let data = &mut self.state.render.deform_data[table];
        for index in retro_render::DEFORM_STORE..retro_render::DEFORM_COUNT {
            if let Some(source) = data.get(index - retro_render::DEFORM_STORE).copied()
                && let Some(slot) = data.get_mut(index)
            {
                *slot = source;
            }
        }
    }

    /// `Set16x16TileInfo`: writes chunk metadata and returns `(chunkX, chunkY, chunk)` for the
    /// script operand outputs.
    fn set_16x16_tile_info(&mut self, value: i32, x: i32, y: i32, info: i32) -> (i32, i32, i32) {
        let chunk_x = x >> 7;
        let chunk_y = y >> 7;
        let base = i32::from(
            self.state
                .layers
                .first()
                .map(|layer| layer.entry(chunk_x, chunk_y))
                .unwrap_or(0),
        ) << 6;
        let chunk = base + ((x & 0x7F) >> 4) + 8 * ((y & 0x7F) >> 4);
        match info {
            TILEINFO_INDEX => {
                if let Some(entry) = usize::try_from(chunk)
                    .ok()
                    .and_then(|index| self.state.render.tiles.chunks.get_mut(index))
                {
                    entry.gfx_data_pos = value << 8;
                }
            }
            TILEINFO_DIRECTION => {
                if let Some(entry) = usize::try_from(chunk)
                    .ok()
                    .and_then(|index| self.state.render.tiles.chunks.get_mut(index))
                {
                    entry.direction = value as u8;
                }
            }
            TILEINFO_VISUALPLANE => {
                if let Some(entry) = usize::try_from(chunk)
                    .ok()
                    .and_then(|index| self.state.render.tiles.chunks.get_mut(index))
                {
                    entry.visual_plane = value as u8;
                }
            }
            TILEINFO_SOLIDITYA | TILEINFO_SOLIDITYB => {
                if let Some(entry) = usize::try_from(chunk)
                    .ok()
                    .and_then(|index| self.state.collision.as_mut()?.tiles.entries.get_mut(index))
                {
                    if info == TILEINFO_SOLIDITYA {
                        entry.collision_flag_a = value as u8;
                    } else {
                        entry.collision_flag_b = value as u8;
                    }
                }
            }
            TILEINFO_FLAGSA | TILEINFO_ANGLEA => {
                let tile_index = usize::try_from(chunk)
                    .ok()
                    .and_then(|index| self.state.collision.as_ref()?.tiles.entries.get(index))
                    .map(|tile| usize::from(tile.tile_index))
                    .unwrap_or(0);
                if let Some(entry) = self
                    .state
                    .collision
                    .as_mut()
                    .and_then(|collision| collision.masks.planes[1].tiles.get_mut(tile_index))
                {
                    if info == TILEINFO_FLAGSA {
                        entry.flags = value as u8;
                    } else {
                        entry.angle = value as u32;
                    }
                }
            }
            _ => {}
        }
        (chunk_x, chunk_y, chunk)
    }

    /// `Copy16x16Tile`: copies one 256-byte 16x16 tile (`Scene.hpp:254-260`).
    fn copy_16x16_tile(&mut self, dest: i32, src: i32) {
        const TILE_BYTES: usize = 256;
        let (Ok(dest), Ok(src)) = (usize::try_from(dest), usize::try_from(src)) else {
            return;
        };
        let (Some(dest_start), Some(src_start)) =
            (dest.checked_mul(TILE_BYTES), src.checked_mul(TILE_BYTES))
        else {
            return;
        };
        let Some(source) = self
            .state
            .render
            .tiles
            .pixels
            .get(src_start..src_start + TILE_BYTES)
        else {
            return;
        };
        let source: Vec<u8> = source.to_vec();
        if let Some(target) = self
            .state
            .render
            .tiles
            .pixels
            .get_mut(dest_start..dest_start + TILE_BYTES)
        {
            target.copy_from_slice(&source);
        }
    }

    /// `DrawNumbers`: draws `digits` decimal digits of `value` right-to-left at `(x, y)`.
    #[allow(clippy::too_many_arguments)]
    fn draw_numbers(
        &mut self,
        frame_base: i32,
        mut x: i32,
        y: i32,
        value: i32,
        digits: i32,
        spacing: i32,
        fixed_width: bool,
    ) {
        let sheet = self.current_sheet_id();
        let mut remaining = digits;
        let mut divisor = 10i32;
        if fixed_width {
            while remaining > 0 {
                let frame_id = (value % divisor) / (divisor / 10) + frame_base;
                let frame = self.state.script_frame(frame_id);
                self.state.render.draw_sprite(
                    sheet,
                    frame.pivot_x + x,
                    frame.pivot_y + y,
                    frame.width,
                    frame.height,
                    frame.spr_x,
                    frame.spr_y,
                );
                x = x.wrapping_sub(spacing);
                divisor = divisor.wrapping_mul(10);
                remaining -= 1;
            }
        } else {
            let mut extra = 10i32;
            if value != 0 {
                extra = 10i32.wrapping_mul(value);
            }
            while remaining > 0 {
                if extra >= divisor {
                    let frame_id = (value % divisor) / (divisor / 10) + frame_base;
                    let frame = self.state.script_frame(frame_id);
                    self.state.render.draw_sprite(
                        sheet,
                        frame.pivot_x + x,
                        frame.pivot_y + y,
                        frame.width,
                        frame.height,
                        frame.spr_x,
                        frame.spr_y,
                    );
                }
                x = x.wrapping_sub(spacing);
                divisor = divisor.wrapping_mul(10);
                remaining -= 1;
            }
        }
    }

    /// `DrawActName`: draws the act title card words from `titleCardText`.
    ///
    /// Modes match `Script.cpp:4506`: 0 draws word 1 right-aligned, 1 draws word 1 left-aligned
    /// and 2 draws word 2 from `titleCardWord2`. `lowercase` shifts the frame base by 26 after
    /// the first glyph so the second half of the frame list supplies lowercase letters.
    #[allow(clippy::too_many_arguments)]
    fn draw_act_name(
        &mut self,
        mut frame_base: i32,
        mut x: i32,
        y: i32,
        mode: i32,
        lowercase: i32,
        space_width: i32,
        spacing: i32,
    ) {
        let title = self.state.scene.title.clone();
        let text = title.as_bytes();
        let normalize = |byte: u8, mode: i32| -> i32 {
            let mut character = i32::from(byte);
            if character == b' ' as i32 {
                character = if mode == 2 { 0 } else { -1 };
            } else if character == b'-' as i32 {
                character = 0;
            }
            if (i32::from(b'0')..=i32::from(b'9')).contains(&character) {
                character -= 22;
            }
            if character > i32::from(b'9') && character < i32::from(b'f') {
                character -= i32::from(b'A');
            }
            character
        };
        match mode {
            0 => {
                // Find the last character of word 1, then draw backwards from the right edge.
                let mut char_id = 0usize;
                while let Some(next) = text.get(char_id + 1)
                    && *next != b'-'
                    && *next != 0
                {
                    char_id += 1;
                }
                let mut index = char_id as i32;
                while index >= 0 {
                    let Some(byte) = usize::try_from(index)
                        .ok()
                        .and_then(|index| text.get(index))
                        .copied()
                    else {
                        break;
                    };
                    let character = normalize(byte, mode);
                    if character <= -1 {
                        x = x.wrapping_sub(space_width + spacing);
                    } else {
                        let frame = self.state.script_frame(character + frame_base);
                        let sheet = self.current_sheet_id();
                        x = x.wrapping_sub(frame.width + spacing);
                        self.state.render.draw_sprite(
                            sheet,
                            x + frame.pivot_x,
                            y + frame.pivot_y,
                            frame.width,
                            frame.height,
                            frame.spr_x,
                            frame.spr_y,
                        );
                    }
                    index -= 1;
                }
            }
            1 | 2 => {
                let mut char_id = if mode == 2 {
                    usize::try_from(self.state.title_card_word2()).unwrap_or(usize::MAX)
                } else {
                    0
                };
                if lowercase == 1 && text.get(char_id).copied().unwrap_or(0) != 0 {
                    let byte = text.get(char_id).copied().unwrap_or(0);
                    let character = normalize(byte, mode);
                    x = self.draw_act_left(character + frame_base, x, y, space_width, spacing);
                    frame_base += 26;
                    char_id += 1;
                }
                while let Some(byte) = text.get(char_id).copied() {
                    if byte == 0 || (mode == 1 && byte == b'-') {
                        break;
                    }
                    let character = normalize(byte, mode);
                    x = self.draw_act_left(character + frame_base, x, y, space_width, spacing);
                    char_id += 1;
                }
            }
            _ => {}
        }
    }

    /// Draws one left-aligned act-name glyph and returns the advanced x position.
    fn draw_act_left(
        &mut self,
        frame_id: i32,
        x: i32,
        y: i32,
        space_width: i32,
        spacing: i32,
    ) -> i32 {
        if frame_id <= -1 {
            return x.wrapping_add(space_width + spacing);
        }
        let frame = self.state.script_frame(frame_id);
        let sheet = self.current_sheet_id();
        self.state.render.draw_sprite(
            sheet,
            x + frame.pivot_x,
            y + frame.pivot_y,
            frame.width,
            frame.height,
            frame.spr_x,
            frame.spr_y,
        );
        x.wrapping_add(frame.width + spacing)
    }

    /// `DrawBitmapText`: draws a text menu row range with the legacy bitmap font.
    #[allow(clippy::too_many_arguments)]
    fn draw_bitmap_text(
        &mut self,
        menu_index: usize,
        x_pos: i32,
        y_pos: i32,
        scale: i32,
        spacing: i32,
        row_start: i32,
        mut row_count: i32,
    ) {
        let sheet = self.state.text_menu_surface_no;
        let (row_count_max, rows) = match self.state.text_menus.get(menu_index) {
            Some(menu) => (
                menu.row_count,
                menu.entry_start
                    .iter()
                    .zip(menu.entry_size.iter())
                    .map(|(start, size)| (*start, *size))
                    .collect::<Vec<_>>(),
            ),
            None => return,
        };
        if row_count < 0 {
            row_count = row_count_max;
        }
        if row_start + row_count > row_count_max {
            row_count = row_count_max - row_start;
        }
        let mut y = y_pos << 9;
        let mut row = row_start;
        while row_count > 0 {
            let mut x = x_pos << 9;
            let (start, size) = rows
                .get(usize::try_from(row).unwrap_or(usize::MAX))
                .copied()
                .unwrap_or((0, 0));
            for index in 0..size {
                let character = self
                    .state
                    .text_menus
                    .get(menu_index)
                    .and_then(|menu| {
                        usize::try_from(start + index)
                            .ok()
                            .and_then(|i| menu.text_data.get(i))
                    })
                    .copied()
                    .unwrap_or(0) as usize;
                let font = self
                    .state
                    .font_characters
                    .get(character)
                    .copied()
                    .unwrap_or_default();
                self.state.render.draw_sprite_scaled(
                    sheet,
                    retro_render::FLIP_NONE,
                    x >> 9,
                    y >> 9,
                    -font.pivot_x,
                    -font.pivot_y,
                    scale,
                    scale,
                    font.width,
                    font.height,
                    font.src_x,
                    font.src_y,
                );
                x = x.wrapping_add(font.x_advance.wrapping_mul(scale));
            }
            y = y.wrapping_add(spacing.wrapping_mul(scale));
            row += 1;
            row_count -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::EngineSettings;
    use crate::rng::GlibcRand;
    use retro_format_v4::collision::{
        COLLISION_FILE_BYTES, COLLISION_PLANE_COUNT, COLLISION_TILE_BYTES, COLLISION_TILE_COUNT,
    };
    use retro_format_v4::tiles::TILE_SHEET_128_ENTRY_COUNT;
    use retro_format_v4::{CollisionMasks, GameConfig, Scene, StageConfig, Tile128, TileSheet128};
    use retro_io::MemorySource;
    use retro_platform::Storage;
    use retro_scene::ObjectRegistry;
    use retro_script::{PlatformMode, V4Revision};
    use std::sync::Arc;

    fn minimal_game_config() -> GameConfig {
        GameConfig {
            title: "Test".to_owned(),
            subtitle: String::new(),
            palette: vec![[0, 0, 0]; retro_format_v4::gameconfig::PALETTE_COUNT],
            objects: Vec::new(),
            global_variables: Vec::new(),
            sound_effects: Vec::new(),
            players: Vec::new(),
            categories: retro_format_v4::gameconfig::CATEGORY_NAMES
                .iter()
                .map(|name| retro_format_v4::SceneCategory {
                    name: (*name).to_owned(),
                    scenes: Vec::new(),
                })
                .collect(),
        }
    }

    fn minimal_scene() -> Scene {
        Scene {
            title: "Test".to_owned(),
            active_layers: [9, 9, 9, 9],
            mid_point: 3,
            width: 1,
            height: 1,
            layout: vec![0],
            entities: Vec::new(),
        }
    }

    /// A collision context whose first 16x16 tile is solid with a floor sample at height 8.
    fn collision() -> retro_scene::SceneCollision {
        let mut bytes = Vec::with_capacity(COLLISION_FILE_BYTES);
        for index in 0..COLLISION_TILE_COUNT * COLLISION_PLANE_COUNT {
            if index == 0 {
                bytes.push(0); // non-ceiling, SOLID_ALL
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&[0x88; 8]);
                bytes.push(0xFF);
                bytes.push(0xFF);
            } else {
                bytes.push(0x33); // SOLID_NONE
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&[0u8; 8]);
                bytes.push(0xFF);
                bytes.push(0xFF);
            }
        }
        assert_eq!(bytes.len(), COLLISION_TILE_BYTES * COLLISION_TILE_COUNT * 2);
        let masks = CollisionMasks::from_bytes(&bytes).unwrap();
        let tiles = TileSheet128 {
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
        };
        retro_scene::SceneCollision::new(
            retro_scene::StageLayout::from_scene(&minimal_scene()),
            tiles,
            masks,
        )
    }

    fn test_state(with_collision: bool) -> EngineState {
        EngineState::new(
            Arc::new(MemorySource::new()),
            EngineSettings {
                profile: crate::RuntimeProfile::V4Legacy,
                platform: PlatformMode::Origins,
                revision: V4Revision::Rev03,
                force_scripts: false,
                dim_limit_frames: 18000,
            },
            minimal_game_config(),
            "Zone01".to_owned(),
            "1".to_owned(),
            minimal_scene(),
            StageConfig {
                load_global_objects: false,
                palette: vec![[0, 0, 0]; retro_format_v4::stageconfig::STAGE_PALETTE_COUNT],
                sound_effects: Vec::new(),
                objects: Vec::new(),
            },
            with_collision.then(collision),
            None,
            ObjectRegistry::new(),
            GlibcRand::new(1),
        )
    }

    #[test]
    fn host_reads_and_writes_entity_fields() {
        let mut state = test_state(false);
        state.entities.reset_object_entity(7, 3, 9, 100, 200);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        assert_eq!(
            host.read_engine_var(23, 7, &mut vm_state).unwrap(),
            100,
            "object.xpos"
        );
        assert_eq!(
            host.read_engine_var(21, 7, &mut vm_state).unwrap(),
            3,
            "object.type"
        );
        host.write_engine_var(23, 7, 500, &mut vm_state).unwrap();
        assert_eq!(host.state.entities.get(7).unwrap().xpos, 500);
    }

    #[test]
    fn host_entity_pos_and_object_values() {
        let mut state = test_state(false);
        state.object_entity_pos = 5;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        assert_eq!(
            host.read_engine_var(19, 5, &mut vm_state).unwrap(),
            5,
            "object.entityPos resolves to the VM-provided slot"
        );

        vm_state.operands[0] = 77;
        vm_state.operands[1] = 3;
        vm_state.operands[2] = 5;
        host.engine_op(Op::SetObjectValue, &mut vm_state).unwrap();
        assert_eq!(host.state.entities.get(5).unwrap().values[3], 77);
        vm_state.operands[0] = 0;
        host.engine_op(Op::GetObjectValue, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[0], 77);
    }

    #[test]
    fn host_reset_object_entity_matches_upstream() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 32;
        vm_state.operands[1] = 4;
        vm_state.operands[2] = 2;
        vm_state.operands[3] = 16;
        vm_state.operands[4] = 32;
        host.engine_op(Op::ResetObjectEntity, &mut vm_state)
            .unwrap();
        let entity = host.state.entities.get(32).unwrap();
        assert_eq!(entity.type_id, 4);
        assert_eq!(entity.property_value, 2);
        assert_eq!((entity.xpos, entity.ypos), (16, 32));
        assert_eq!(entity.scale, 512);
        assert_eq!(entity.visible, 1);
    }

    #[test]
    fn host_tile_collision_on_tiny_mask() {
        let mut state = test_state(true);
        state
            .entities
            .reset_object_entity(0, 1, 0, 64 << 16, 64 << 16);
        state.object_entity_pos = 0;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = CSIDE_FLOOR;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 15;
        vm_state.operands[3] = 0;
        host.engine_op(Op::ObjectTileCollision, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 1);
        assert_eq!(host.state.entities.get(0).unwrap().ypos >> 16, 57);
    }

    #[test]
    fn host_foreach_iterates_type_groups_in_slot_order() {
        let mut state = test_state(false);
        state.entities.reset_object_entity(5, 9, 0, 0, 0);
        state.entities.reset_object_entity(9, 9, 0, 0, 0);
        let mut flags = vec![false; retro_scene::ENTITY_COUNT];
        flags[5] = true;
        flags[9] = true;
        state
            .entities
            .build_type_groups(&flags, &mut state.type_groups, retro_scene::OBJECT_COUNT);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        assert_eq!(
            host.foreach_next(Op::ForEachActive, 9, 0, ScriptEvent::Main, &mut vm_state)
                .unwrap(),
            Some(5)
        );
        assert_eq!(
            host.foreach_next(Op::ForEachActive, 9, 1, ScriptEvent::Main, &mut vm_state)
                .unwrap(),
            Some(9)
        );
        assert_eq!(
            host.foreach_next(Op::ForEachActive, 9, 2, ScriptEvent::Main, &mut vm_state)
                .unwrap(),
            None
        );
        assert_eq!(
            host.foreach_next(Op::ForEachAll, 9, 4, ScriptEvent::Main, &mut vm_state)
                .unwrap(),
            Some(5)
        );
    }

    #[test]
    fn check_camera_proximity_handles_partial_ranges() {
        let mut state = test_state(false);
        state.camera.xpos = 100;
        state.camera.ypos = 200;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();

        // Both axes: inside.
        vm_state.operands[0] = 110;
        vm_state.operands[1] = 210;
        vm_state.operands[2] = 20;
        vm_state.operands[3] = 20;
        host.engine_op(Op::CheckCameraProximity, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 1);

        // Only x: y distance is huge, x passes.
        vm_state.operands[1] = i32::MAX;
        vm_state.operands[3] = 0;
        host.engine_op(Op::CheckCameraProximity, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 1);

        // Only y: x distance is huge, y passes.
        vm_state.operands[0] = i32::MIN;
        vm_state.operands[1] = 210;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 20;
        host.engine_op(Op::CheckCameraProximity, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 1);

        // Only x and failing.
        vm_state.operands[0] = i32::MIN;
        vm_state.operands[2] = 5;
        vm_state.operands[3] = 0;
        host.engine_op(Op::CheckCameraProximity, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 0);

        // Neither range: stays false.
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 0;
        host.engine_op(Op::CheckCameraProximity, &mut vm_state)
            .unwrap();
        assert_eq!(vm_state.check_result, 0);
    }

    #[test]
    fn check_touch_rect_keeps_the_last_matching_touch() {
        let mut state = test_state(false);
        assert_eq!(state.touch_down.len(), crate::state::TOUCH_COUNT);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 100;
        vm_state.operands[3] = 100;
        host.engine_op(Op::CheckTouchRect, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, -1, "no touches");

        // Two touches inside the rect: upstream scans all slots and keeps the last match.
        for slot in [2usize, 5] {
            host.state.touch_down[slot] = 1;
            host.state.touch_x[slot] = 50;
            host.state.touch_y[slot] = 50;
        }
        host.engine_op(Op::CheckTouchRect, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 5);

        // Moving the later touch out of the rect falls back to the earlier match.
        host.state.touch_x[5] = 500;
        host.engine_op(Op::CheckTouchRect, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 2);
    }

    #[test]
    fn read_save_ram_uses_the_save_store_and_saveram_variable() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        host.engine_op(Op::ReadSaveRAM, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 0, "in-memory store starts empty");

        let mut storage = retro_platform::headless::MemoryStorage::new();
        storage
            .write(retro_format_v4::userdata::SaveRam::SAVE_PATH, &[0u8; 16])
            .unwrap();
        host.state.save = crate::save::SaveState::open(Box::new(storage)).unwrap();
        host.engine_op(Op::ReadSaveRAM, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 1, "SGame.bin present");

        assert_eq!(
            host.read_engine_var(VAR_SAVE_RAM, 2, &mut vm_state)
                .unwrap(),
            0
        );
        host.write_engine_var(VAR_SAVE_RAM, 2, 77, &mut vm_state)
            .unwrap();
        assert_eq!(
            host.read_engine_var(VAR_SAVE_RAM, 2, &mut vm_state)
                .unwrap(),
            77
        );
        host.engine_op(Op::WriteSaveRAM, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 1);
        assert!(
            host.state
                .save
                .storage()
                .exists(retro_format_v4::userdata::SaveRam::SAVE_PATH)
        );
        assert_eq!(host.state.stub_histogram.get("ReadSaveRAM"), None);
        assert_eq!(host.state.op_histogram.get("ReadSaveRAM"), Some(&2));
    }

    #[test]
    fn music_variables_read_and_write_through_the_mixer() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();

        assert_eq!(
            host.read_engine_var(VAR_MUSIC_VOLUME, 0, &mut vm_state)
                .unwrap(),
            100
        );
        host.write_engine_var(VAR_MUSIC_VOLUME, 0, 25, &mut vm_state)
            .unwrap();
        assert_eq!(
            host.read_engine_var(VAR_MUSIC_VOLUME, 0, &mut vm_state)
                .unwrap(),
            25
        );
        host.write_engine_var(VAR_MUSIC_VOLUME, 0, 500, &mut vm_state)
            .unwrap();
        assert_eq!(
            host.read_engine_var(VAR_MUSIC_VOLUME, 0, &mut vm_state)
                .unwrap(),
            100,
            "SetMusicVolume clamps to MAX_VOLUME"
        );
        host.write_engine_var(VAR_ENGINE_SFX_VOLUME, 0, 40, &mut vm_state)
            .unwrap();
        assert_eq!(
            host.read_engine_var(VAR_ENGINE_SFX_VOLUME, 0, &mut vm_state)
                .unwrap(),
            40
        );
        host.write_engine_var(VAR_ENGINE_BGM_VOLUME, 0, 30, &mut vm_state)
            .unwrap();
        assert_eq!(
            host.read_engine_var(VAR_ENGINE_BGM_VOLUME, 0, &mut vm_state)
                .unwrap(),
            30
        );
        assert_eq!(
            host.read_engine_var(VAR_MUSIC_POSITION, 0, &mut vm_state)
                .unwrap(),
            0,
            "no stream is playing"
        );
    }

    #[test]
    fn play_music_sets_track_id_only_after_a_successful_load() {
        let mut state = test_state(false);
        state.audio.set_track(3, "Missing.ogg", true, 0);
        state.music_track = 9;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 3;
        host.engine_op(Op::PlayMusic, &mut vm_state).unwrap();
        assert_eq!(
            host.state.music_track, 9,
            "a missing stream must not update trackID (LoadMusic only sets it on success)"
        );
        assert_eq!(host.state.op_histogram.get("PlayMusic"), Some(&1));
    }

    #[test]
    fn box_collision2_is_reported_as_an_explicit_stub() {
        let mut state = test_state(true);
        state
            .entities
            .reset_object_entity(0, 1, 0, 64 << 16, 64 << 16);
        state
            .entities
            .reset_object_entity(1, 2, 0, 80 << 16, 64 << 16);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = C_SOLID2;
        vm_state.operands[1] = 0;
        vm_state.operands[6] = 1;
        host.engine_op(Op::BoxCollisionTest, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 0);
        assert_eq!(host.state.stub_histogram.get("BoxCollision2"), Some(&1));
        assert!(!host.state.op_histogram.contains_key("BoxCollisionTest"));
    }

    #[test]
    fn box_collision_test_platform_lands_the_falling_entity() {
        let mut state = test_state(true);
        state
            .entities
            .reset_object_entity(0, 1, 0, 100 << 16, 100 << 16);
        state
            .entities
            .reset_object_entity(1, 2, 0, 100 << 16, 90 << 16);
        state.entities.get_mut(1).unwrap().yvel = 0x20000;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = C_PLATFORM;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = -8;
        vm_state.operands[3] = -8;
        vm_state.operands[4] = 8;
        vm_state.operands[5] = 8;
        vm_state.operands[6] = 1;
        vm_state.operands[7] = -8;
        vm_state.operands[8] = -8;
        vm_state.operands[9] = 8;
        vm_state.operands[10] = 8;
        host.engine_op(Op::BoxCollisionTest, &mut vm_state).unwrap();
        assert_eq!(vm_state.check_result, 1);
        assert_eq!(host.state.entities.get(1).unwrap().ypos, 84 << 16);
        assert_eq!(host.state.op_histogram.get("BoxCollisionTest"), Some(&1));
    }

    fn animation_state() -> EngineState {
        use retro_format_v4::{Animation, AnimationFile, AnimationFrame};

        let mut state = test_state(false);
        let frame = |x: u8| AnimationFrame {
            sheet_index: 0,
            sheet: String::new(),
            hitbox_id: 0,
            x,
            y: 0,
            width: 16,
            height: 16,
            pivot_x: 0,
            pivot_y: 0,
        };
        let file = AnimationFile {
            sheets: Vec::new(),
            animations: vec![
                Animation {
                    name: "Idle".to_owned(),
                    frame_count: 3,
                    playback_frame_count: 3,
                    speed: 0x10,
                    loop_point: 1,
                    rotation_style: 0,
                    frames: vec![frame(0), frame(16), frame(32)],
                },
                Animation {
                    name: "Walk".to_owned(),
                    frame_count: 2,
                    playback_frame_count: 2,
                    speed: 0x20,
                    loop_point: 0,
                    rotation_style: 0,
                    frames: vec![frame(0), frame(16)],
                },
            ],
            hitboxes: Vec::new(),
        };
        state.animations.push(file);
        state.animation_sheet_ids.push(Vec::new());
        state
            .objects
            .push("Animated", Vec::new(), Default::default());
        if let Some(entry) = state.objects.get_mut(1) {
            entry.animation_file = Some(0);
        }
        state.object_frames.resize(state.objects.len(), Vec::new());
        state.entities.reset_object_entity(0, 1, 0, 0, 0);
        state.object_entity_pos = 0;
        state
    }

    #[test]
    fn process_animation_steps_frames_and_loops_like_upstream() {
        let mut state = animation_state();
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        // Default speed 0 uses the animation speed 0x10; a frame advances every 15 calls.
        for step in 1..=15 {
            host.engine_op(Op::ProcessAnimation, &mut vm_state).unwrap();
            let entity = host.state.entities.get(0).unwrap();
            let expected = i32::from(step == 15);
            assert_eq!(entity.frame, expected as u8, "step {step}");
            assert_eq!(entity.animation_timer, (step * 0x10) % 0xF0, "timer {step}");
        }
        // Frame 3 is out of range and loops back to the loop point (1).
        for _ in 0..30 {
            host.engine_op(Op::ProcessAnimation, &mut vm_state).unwrap();
        }
        assert_eq!(host.state.entities.get(0).unwrap().frame, 1);
    }

    #[test]
    fn process_animation_switching_animation_resets_phase() {
        let mut state = animation_state();
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        for _ in 0..20 {
            host.engine_op(Op::ProcessAnimation, &mut vm_state).unwrap();
        }
        assert_eq!(host.state.entities.get(0).unwrap().frame, 1);
        host.state.entities.get_mut(0).unwrap().animation = 1;
        host.engine_op(Op::ProcessAnimation, &mut vm_state).unwrap();
        let entity = host.state.entities.get(0).unwrap();
        assert_eq!(entity.prev_animation, 1);
        assert_eq!(entity.frame, 0);
        assert_eq!(entity.animation_timer, 0);
        assert_eq!(entity.animation_speed, 0);
    }

    #[test]
    fn sprite_frame_and_edit_frame_build_the_object_frame_list() {
        let mut state = test_state(false);
        state.objects.push("Test", Vec::new(), Default::default());
        state.entities.reset_object_entity(7, 1, 0, 0, 0);
        state.object_entity_pos = 7;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState {
            current_event: ScriptEvent::Setup,
            ..VmState::default()
        };
        for (pivot, spr) in [(1, 10), (2, 20)] {
            vm_state.operands[0] = pivot;
            vm_state.operands[1] = -pivot;
            vm_state.operands[2] = 16;
            vm_state.operands[3] = 16;
            vm_state.operands[4] = spr;
            vm_state.operands[5] = spr + 1;
            host.engine_op(Op::SpriteFrame, &mut vm_state).unwrap();
        }
        assert_eq!(host.state.script_frame(1).spr_x, 20);
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 5;
        vm_state.operands[2] = 6;
        vm_state.operands[3] = 7;
        vm_state.operands[4] = 8;
        vm_state.operands[5] = 9;
        vm_state.operands[6] = 10;
        host.engine_op(Op::EditFrame, &mut vm_state).unwrap();
        assert_eq!(
            host.state.script_frame(0),
            ScriptFrame {
                pivot_x: 5,
                pivot_y: 6,
                width: 7,
                height: 8,
                spr_x: 9,
                spr_y: 10
            }
        );

        // Outside the setup event `SpriteFrame` is ignored.
        vm_state.current_event = ScriptEvent::Main;
        vm_state.operands[0] = 3;
        host.engine_op(Op::SpriteFrame, &mut vm_state).unwrap();
        assert_eq!(host.state.object_frames[1].len(), 2);
    }

    #[test]
    fn palette_ops_route_into_the_software_palette() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 3;
        vm_state.operands[2] = 0x11_22_33;
        host.engine_op(Op::SetPaletteEntry, &mut vm_state).unwrap();
        vm_state.operands[2] = 0;
        host.engine_op(Op::GetPaletteEntry, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[2], 0x11_22_33);

        vm_state.operands[0] = 4;
        vm_state.operands[1] = 60;
        vm_state.operands[2] = 0xFF;
        host.engine_op(Op::SetActivePalette, &mut vm_state).unwrap();
        assert_eq!(host.state.render.palette.line_buffer[60], 4);
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 4;
        vm_state.operands[2] = 0;
        host.engine_op(Op::GetPaletteEntry, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[2], 0, "active bank has no entry 3 yet");
    }

    #[test]
    fn draw_rect_and_tint_rect_touch_the_framebuffer() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 4;
        vm_state.operands[3] = 2;
        vm_state.operands[4] = 255;
        vm_state.operands[5] = 0;
        vm_state.operands[6] = 0;
        vm_state.operands[7] = 255;
        host.engine_op(Op::DrawRect, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(3, 1), 0xF800);
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 1;
        vm_state.operands[3] = 1;
        host.engine_op(Op::DrawTintRect, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(0, 0), 0x8410);
        assert_eq!(host.state.render.framebuffer.get(3, 1), 0xF800);
    }

    #[test]
    fn set_screen_fade_draws_on_the_next_stage_draw() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 0xFF;
        host.engine_op(Op::SetScreenFade, &mut vm_state).unwrap();
        assert_eq!(host.state.render.fade_mode, 1);
        assert_eq!(host.state.render.fade_a, 0xFF);
        host.state.render.draw_fade();
        assert_eq!(host.state.render.framebuffer.get(0, 0), 0x0000);
    }

    /// Registers a second object type with a sheet and frame list, ready for sprite ops.
    fn sprite_state(frame_count: usize) -> EngineState {
        let mut state = test_state(false);
        state
            .objects
            .push("SpriteObject", Vec::new(), Default::default());
        state.object_frames.resize(state.objects.len(), Vec::new());
        state.object_frames[1] = (0..frame_count)
            .map(|index| ScriptFrame {
                pivot_x: 0,
                pivot_y: 0,
                width: 1,
                height: 1,
                spr_x: index as i32,
                spr_y: 0,
            })
            .collect();
        if let Some(entry) = state.objects.get_mut(1) {
            entry.sprite_sheet_id = 0;
        }
        state
            .render
            .surfaces
            .push(retro_render::Surface::from_indexed(
                frame_count.max(1) as u16,
                1,
                vec![1; frame_count.max(1)],
            ));
        state.render.palette.set_bank_entry(0, 1, 255, 0, 0);
        state.entities.reset_object_entity(0, 1, 0, 0, 0);
        state.object_entity_pos = 0;
        state
    }

    #[test]
    fn copy_16x16_tile_copies_the_whole_256_byte_tile() {
        let mut state = test_state(false);
        let mut source = vec![0u8; retro_render::TILE_SET_16_SIZE];
        for (offset, byte) in source[256..512].iter_mut().enumerate() {
            *byte = (offset % 251) as u8 + 1;
        }
        state.render.tiles.pixels = source;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 2;
        vm_state.operands[1] = 1;
        host.engine_op(Op::Copy16x16Tile, &mut vm_state).unwrap();
        let pixels = &host.state.render.tiles.pixels;
        assert_eq!(&pixels[512..768], &pixels[256..512]);
        assert_ne!(pixels[512], 0);
    }

    #[test]
    fn set_16x16_tile_info_updates_chunk_metadata_and_outputs() {
        let mut state = test_state(false);
        state.render.tiles.chunks = vec![retro_render::ChunkEntry::default(); 1];
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 2;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = TILEINFO_INDEX;
        host.engine_op(Op::Set16x16TileInfo, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[4], 0, "chunkX");
        assert_eq!(vm_state.operands[5], 0, "chunkY");
        assert_eq!(vm_state.operands[6], 0, "chunk");
        assert_eq!(host.state.render.tiles.chunks[0].gfx_data_pos, 2 << 8);

        vm_state.operands[0] = 3;
        vm_state.operands[3] = TILEINFO_DIRECTION;
        host.engine_op(Op::Set16x16TileInfo, &mut vm_state).unwrap();
        assert_eq!(host.state.render.tiles.chunks[0].direction, 3);
        vm_state.operands[0] = 1;
        vm_state.operands[3] = TILEINFO_VISUALPLANE;
        host.engine_op(Op::Set16x16TileInfo, &mut vm_state).unwrap();
        assert_eq!(host.state.render.tiles.chunks[0].visual_plane, 1);
        assert!(host.state.stub_histogram.is_empty());
    }

    #[test]
    fn draw_numbers_uses_the_script_frame_list() {
        let mut state = sprite_state(10);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0; // frame base
        vm_state.operands[1] = 4; // x
        vm_state.operands[2] = 2; // y
        vm_state.operands[3] = 7; // value
        vm_state.operands[4] = 1; // digits
        vm_state.operands[5] = 8; // spacing
        vm_state.operands[6] = 0; // auto width
        host.engine_op(Op::DrawNumbers, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(4, 2), 0xF800);
        assert_eq!(host.state.render.framebuffer.get(3, 2), 0);
        assert_eq!(host.state.op_histogram.get("DrawNumbers"), Some(&1));
    }

    #[test]
    fn draw_act_name_renders_title_card_letters() {
        let mut state = sprite_state(2);
        state.scene.title = "AB".to_owned();
        state.entities.get_mut(0).unwrap().visible = 1;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 1; // mode 1: word 1 left aligned
        vm_state.operands[4] = 0; // uppercase only
        vm_state.operands[5] = 8;
        vm_state.operands[6] = 0;
        host.engine_op(Op::DrawActName, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(0, 0), 0xF800);
        assert_eq!(host.state.render.framebuffer.get(1, 0), 0xF800);
        assert_eq!(host.state.op_histogram.get("DrawActName"), Some(&1));
    }

    #[test]
    fn fx_tint_only_masks_under_alpha_ink() {
        // INK_ALPHA (2) tints the framebuffer under opaque pixels.
        let mut state = sprite_state(1);
        state.entities.get_mut(0).unwrap().ink_effect = 2;
        state.render.framebuffer.set(0, 0, 0x07E0);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0; // frame
        vm_state.operands[1] = 4; // FX_TINT
        vm_state.operands[2] = 0; // x << 16
        vm_state.operands[3] = 0; // y << 16
        host.engine_op(Op::DrawSpriteFX, &mut vm_state).unwrap();
        assert_eq!(
            host.state.render.framebuffer.get(0, 0),
            host.state.render.lookup.tint(0x07E0)
        );

        // INK_BLEND (1) uses the palette blit instead.
        let mut state = sprite_state(1);
        state.entities.get_mut(0).unwrap().ink_effect = 1;
        state.render.framebuffer.set(0, 0, 0x07E0);
        let mut host = EngineHost { state: &mut state };
        host.engine_op(Op::DrawSpriteFX, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(0, 0), 0xF800);
    }

    #[test]
    fn set_layer_deformation_mirrors_the_store_tail() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0; // DEFORM_FG
        vm_state.operands[1] = 8; // wave length
        vm_state.operands[2] = 10; // wave width
        vm_state.operands[3] = 0; // wave type
        vm_state.operands[4] = 0;
        vm_state.operands[5] = 0;
        host.engine_op(Op::SetLayerDeformation, &mut vm_state)
            .unwrap();
        let data = &host.state.render.deform_data[0];
        assert!(data[..256].iter().any(|value| *value != 0));
        for index in retro_render::DEFORM_STORE..retro_render::DEFORM_COUNT {
            assert_eq!(data[index], data[index - retro_render::DEFORM_STORE]);
        }
    }

    #[test]
    fn text_ops_load_a_font_text_and_draw_glyphs() {
        let mut state = sprite_state(1);
        // Upstream indexes `fontCharacterList` by the text-data value, so the record for 'A'
        // (65) must live at index 65.
        let mut font = vec![0u8; 65 * 20];
        for id in [65u32, 66] {
            font.extend_from_slice(&id.to_le_bytes());
            font.extend_from_slice(&0u16.to_le_bytes()); // src x
            font.extend_from_slice(&0u16.to_le_bytes()); // src y
            font.extend_from_slice(&1u16.to_le_bytes()); // width
            font.extend_from_slice(&1u16.to_le_bytes()); // height
            font.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]); // pivots, advance, unused
        }
        let mut source = MemorySource::new();
        source.insert("Data/Game/Font.bin", font);
        source.insert("Data/Game/Text.bin", b"A\rB".to_vec());
        state.source = Arc::new(source);

        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState {
            script_text: "Data/Game/Font.bin".to_owned(),
            ..VmState::default()
        };
        host.engine_op(Op::LoadFontFile, &mut vm_state).unwrap();
        assert_eq!(host.state.font_characters[65].id, 65);
        assert_eq!(host.state.font_characters[65].x_advance, 1);

        vm_state.script_text = "Data/Game/Text.bin".to_owned();
        vm_state.operands[0] = 0;
        vm_state.operands[2] = 0;
        host.engine_op(Op::LoadTextFile, &mut vm_state).unwrap();
        // `rowCount` is a true row count upstream: `LoadTextFile` bumps it on every `\r` and
        // then once more before returning (`Text.cpp:98,226-238`), and `DrawBitmapText`
        // iterates `rowStart..rowStart+rowCount` (`Drawing.cpp:4381-4399`). `"A\rB"` therefore
        // has two rows, not a last index of one.
        assert_eq!(host.state.text_menus[0].row_count, 2);
        assert_eq!(host.state.text_menus[0].text_data, vec![65, 66]);
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0; // TEXTINFO_TEXTDATA
        vm_state.operands[3] = 1;
        vm_state.operands[4] = 0;
        host.engine_op(Op::GetTextInfo, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[0], 66);
        vm_state.operands[2] = 1; // TEXTINFO_TEXTSIZE
        host.engine_op(Op::GetTextInfo, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[0], 1);
        vm_state.operands[2] = 2; // TEXTINFO_ROWCOUNT
        host.engine_op(Op::GetTextInfo, &mut vm_state).unwrap();
        assert_eq!(vm_state.operands[0], 2);

        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 512;
        vm_state.operands[4] = 4;
        vm_state.operands[5] = 0;
        vm_state.operands[6] = -1;
        host.engine_op(Op::DrawText, &mut vm_state).unwrap();
        assert_eq!(host.state.render.framebuffer.get(0, 0), 0xF800);
        assert_eq!(host.state.render.framebuffer.get(0, 4), 0xF800);
    }

    #[test]
    fn load_stage_op_queues_a_deferred_request() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        host.engine_op(Op::LoadStage, &mut vm_state).unwrap();
        assert!(host.state.load_stage_requested);
        assert_eq!(host.state.op_histogram.get("LoadStage"), Some(&1));
        assert!(
            !host.state.stub_histogram.contains_key("LoadStage"),
            "LoadStage is implemented, not a stub"
        );
    }

    #[test]
    fn stage_deformation_data_variables_round_trip() {
        let mut state = test_state(false);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        // `stage.deformationData0..3` are rev03 variable ids 139..=142 and index the render
        // state's four deformation tables.
        for (offset, var) in (139..=142).enumerate() {
            host.write_engine_var(var, 5, 100 + offset as i32, &mut vm_state)
                .unwrap();
            assert_eq!(
                host.state.render.deform_data[offset][5],
                100 + offset as i32,
                "deformationData{offset} write"
            );
            assert_eq!(
                host.read_engine_var(var, 5, &mut vm_state).unwrap(),
                100 + offset as i32,
                "deformationData{offset} read"
            );
        }
        // Out-of-range indices are ignored, not panics.
        host.write_engine_var(139, -1, 7, &mut vm_state).unwrap();
        assert_eq!(host.read_engine_var(139, -1, &mut vm_state).unwrap(), 0);
    }

    #[test]
    fn camera_follow_keeps_script_scroll_without_a_target() {
        // Title screens keep `cameraTarget == -1`; `screen.xoffset` must survive the frame.
        let mut state = test_state(false);
        state.screen.x_scroll = 44;
        state.screen.y_scroll = -3;
        state.camera.enabled = 1;
        state.camera.target = -1;
        state.camera.xpos = 0;
        state.camera.ypos = 0;
        crate::runtime::follow_camera(&mut state);
        assert_eq!((state.screen.x_scroll, state.screen.y_scroll), (44, -3));
        assert_eq!((state.camera.xpos, state.camera.ypos), (0, 0));
    }

    #[test]
    fn camera_follow_recomputes_scroll_from_the_target() {
        // A boundary large enough that the follow is not clamped to a 1x1 scene.
        let mut state = test_state(false);
        state.stage.cur_x_boundary1 = 0;
        state.stage.cur_x_boundary2 = 10_000;
        state.stage.new_x_boundary1 = 0;
        state.stage.new_x_boundary2 = 10_000;
        state.stage.cur_y_boundary1 = 0;
        state.stage.cur_y_boundary2 = 10_000;
        state.stage.new_y_boundary1 = 0;
        state.stage.new_y_boundary2 = 10_000;
        state.screen.x_scroll = 44;
        state.screen.y_scroll = 7;
        state.camera.enabled = 1;
        state.camera.target = 0;
        state
            .entities
            .reset_object_entity(0, 1, 0, 212 << 16, 120 << 16);
        crate::runtime::follow_camera(&mut state);
        // `SetPlayerScreenPosition` centres the camera on the target horizontally; vertically
        // it clamps the camera to `curYBoundary1 + SCREEN_SCROLL_UP` (104), which makes the
        // derived `yScrollOffset` equal `curYBoundary1`.
        assert_eq!((state.camera.xpos, state.camera.ypos), (212, 104));
        assert_eq!((state.screen.x_scroll, state.screen.y_scroll), (0, 0));

        // A disabled camera leaves the script scroll untouched even with a live target.
        state.screen.x_scroll = 44;
        state.camera.enabled = 0;
        crate::runtime::follow_camera(&mut state);
        assert_eq!(state.screen.x_scroll, 44);
    }

    /// Replaces the single-pixel test surface with a `width`-wide row of colour index 1.
    fn wide_frame_state(width: i32, pivot_x: i32) -> EngineState {
        let mut state = sprite_state(1);
        state.object_frames[1] = vec![ScriptFrame {
            pivot_x,
            pivot_y: 0,
            width,
            height: 1,
            spr_x: 0,
            spr_y: 0,
        }];
        state.render.surfaces[0] =
            retro_render::Surface::from_indexed(width as u16, 1, vec![1; width as usize]);
        state
    }

    #[test]
    fn draw_sprite_screen_xy_centers_and_ignores_scroll() {
        // A 256-wide frame with pivot -128 drawn at the 424-wide screen centre: 212 - 128 = 84.
        let mut state = wide_frame_state(256, -128);
        state.screen.x_scroll = 100;
        state.screen.y_scroll = 100;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 212;
        vm_state.operands[2] = 50;
        host.engine_op(Op::DrawSpriteScreenXY, &mut vm_state)
            .unwrap();
        let framebuffer = &host.state.render.framebuffer;
        assert_eq!(framebuffer.get(83, 50), 0, "pivot starts at x = 84");
        assert_eq!(framebuffer.get(84, 50), 0xF800);
        assert_eq!(framebuffer.get(212, 50), 0xF800);
        assert_eq!(framebuffer.get(339, 50), 0xF800, "last column is 84 + 255");
        assert_eq!(framebuffer.get(340, 50), 0);
    }

    #[test]
    fn draw_sprite_xy_subtracts_scroll_and_applies_pivot() {
        // `screen.xoffset = 44` with the object at world x = 256: 256 - 44 - 128 = 84.
        let mut state = wide_frame_state(256, -128);
        state.screen.x_scroll = 44;
        state.screen.y_scroll = 0;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 256 << 16;
        vm_state.operands[2] = 120 << 16;
        host.engine_op(Op::DrawSpriteXY, &mut vm_state).unwrap();
        let framebuffer = &host.state.render.framebuffer;
        assert_eq!(framebuffer.get(84, 120), 0xF800);
        assert_eq!(framebuffer.get(339, 120), 0xF800);
        assert_eq!(framebuffer.get(340, 120), 0);
    }

    #[test]
    fn draw_sprite_screen_xy_clips_negative_coordinates() {
        // A 4-wide frame at x = -2 keeps its last two columns at x = 0 and 1.
        let mut state = wide_frame_state(4, 0);
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = -2;
        vm_state.operands[2] = 0;
        host.engine_op(Op::DrawSpriteScreenXY, &mut vm_state)
            .unwrap();
        let framebuffer = &host.state.render.framebuffer;
        assert_eq!(framebuffer.get(0, 0), 0xF800);
        assert_eq!(framebuffer.get(1, 0), 0xF800);
        assert_eq!(framebuffer.get(2, 0), 0);
    }

    #[test]
    fn draw_sprite_fx_flip_mirrors_source_columns() {
        let mut state = sprite_state(1);
        state.object_frames[1] = vec![ScriptFrame {
            pivot_x: 0,
            pivot_y: 0,
            width: 4,
            height: 1,
            spr_x: 0,
            spr_y: 0,
        }];
        state.render.surfaces[0] = retro_render::Surface::from_indexed(4, 1, vec![1, 2, 3, 4]);
        state.render.palette.set_bank_entry(0, 2, 0, 255, 0);
        state.render.palette.set_bank_entry(0, 3, 0, 0, 255);
        state.render.palette.set_bank_entry(0, 4, 255, 255, 0);
        state.entities.get_mut(0).unwrap().direction = retro_render::FLIP_X;
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState::default();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 5; // FX_FLIP
        vm_state.operands[2] = 10 << 16;
        vm_state.operands[3] = 10 << 16;
        host.engine_op(Op::DrawSpriteFX, &mut vm_state).unwrap();
        let framebuffer = &host.state.render.framebuffer;
        // FLIP_X draws at `x - width - pivot` and samples the source backwards.
        assert_eq!(framebuffer.get(6, 10), 0xFFE0, "column 0 samples source 3");
        assert_eq!(framebuffer.get(7, 10), 0x001F);
        assert_eq!(framebuffer.get(8, 10), 0x07E0);
        assert_eq!(framebuffer.get(9, 10), 0xF800, "column 3 samples source 0");
        assert_eq!(framebuffer.get(5, 10), 0);
    }

    #[test]
    fn draw_text_advances_glyphs_by_x_advance() {
        let mut state = sprite_state(1);
        // Glyphs 'A' and 'B' are 2x1 red pixels advancing 3 px at scale 512 (1x).
        let mut font = vec![0u8; 65 * 20];
        for id in [65u32, 66] {
            font.extend_from_slice(&id.to_le_bytes());
            font.extend_from_slice(&0u16.to_le_bytes());
            font.extend_from_slice(&0u16.to_le_bytes());
            font.extend_from_slice(&2u16.to_le_bytes());
            font.extend_from_slice(&1u16.to_le_bytes());
            font.extend_from_slice(&[0, 0, 0, 0, 3, 0, 0, 0]);
        }
        let mut source = MemorySource::new();
        source.insert("Data/Game/Font.bin", font);
        source.insert("Data/Game/Text.bin", b"AB".to_vec());
        state.source = Arc::new(source);
        state.render.surfaces[0] = retro_render::Surface::from_indexed(4, 1, vec![1, 1, 1, 1]);
        state.object_frames[1] = vec![ScriptFrame {
            pivot_x: 0,
            pivot_y: 0,
            width: 2,
            height: 1,
            spr_x: 0,
            spr_y: 0,
        }];
        let mut host = EngineHost { state: &mut state };
        let mut vm_state = VmState {
            script_text: "Data/Game/Font.bin".to_owned(),
            ..VmState::default()
        };
        host.engine_op(Op::LoadFontFile, &mut vm_state).unwrap();
        vm_state.script_text = "Data/Game/Text.bin".to_owned();
        vm_state.operands[0] = 0;
        vm_state.operands[2] = 0;
        host.engine_op(Op::LoadTextFile, &mut vm_state).unwrap();
        vm_state.operands[0] = 0;
        vm_state.operands[1] = 0;
        vm_state.operands[2] = 0;
        vm_state.operands[3] = 512;
        vm_state.operands[4] = 4;
        vm_state.operands[5] = 0;
        vm_state.operands[6] = -1;
        host.engine_op(Op::DrawText, &mut vm_state).unwrap();
        let framebuffer = &host.state.render.framebuffer;
        assert_eq!(framebuffer.get(0, 0), 0xF800, "first glyph at x = 0");
        assert_eq!(framebuffer.get(1, 0), 0xF800);
        assert_eq!(framebuffer.get(2, 0), 0);
        assert_eq!(framebuffer.get(3, 0), 0xF800, "advance 3 puts B at x = 3");
        assert_eq!(framebuffer.get(4, 0), 0xF800);
    }
}
