//! Engine runtime: scene instantiation, the 60 Hz frame loop and state hashing.
//!
//! Startup and update ordering mirror `LoadStageFiles`/`ProcessStartupObjects`/`ProcessObjects`
//! in `RSDKv4/Scene.cpp` and `RSDKv4/Object.cpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195):
//!
//! * scene entities are placed in slots `32..` in file order,
//! * startup events run once per object type in object-list order against the temp slot,
//! * update events run per entity slot in ascending order,
//! * type groups are rebuilt after the update pass from the pre-update process flags,
//! * the camera follows `camera.target` once per frame (simplified, see below).
//!
//! The state hash is a canonical little-endian serialisation of all entity slots, the object
//! list, camera/screen/stage metadata, global VM state and the RNG state, fed through BLAKE3.
//!
//! # Camera divergence
//!
//! Upstream tracks the camera through `SetPlayerScreenPosition`/`SetPlayerScreenPositionCDStyle`
//! (look-ahead, y-locking and boundary easing, ~200 lines). M3 uses a deterministic
//! centre-and-clamp follow instead; `object.outOfBounds` and update ranges therefore differ from
//! the reference near screen edges.

use std::sync::Arc;

use retro_audio::AudioEngine;
use retro_format_v4::SceneEntity;
use retro_format_v4::scene::{
    ENTITY_ATTRIB_ALPHA, ENTITY_ATTRIB_ANIMATION, ENTITY_ATTRIB_ANIMATION_SPEED,
    ENTITY_ATTRIB_DIRECTION, ENTITY_ATTRIB_DRAW_ORDER, ENTITY_ATTRIB_FRAME,
    ENTITY_ATTRIB_INK_EFFECT, ENTITY_ATTRIB_PRIORITY, ENTITY_ATTRIB_ROTATION, ENTITY_ATTRIB_SCALE,
    ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES,
};
use retro_input::ScriptedInput;
use retro_io::DataSource;
use retro_platform::RawInput;
use retro_platform::Storage;
use retro_render::ChunkEntry;
use retro_render::layers::{LAYER_3DFLOOR, LAYER_3DSKY, LAYER_HSCROLL, LAYER_VSCROLL, LayerView};
use retro_scene::{
    DRAWLAYER_COUNT, ENTITY_COUNT, EntityStore, OBJECT_COUNT, SCENE_ENTITY_START, TEMPENTITY_START,
};
use retro_script::{ScriptEvent, ScriptFile, Vm, VmState};

use crate::EngineError;
use crate::audio::AudioState;
use crate::host::EngineHost;
use crate::input::{EngineInput, PRESS_BUTTONS, apply_players};
use crate::loader;
use crate::profile::EngineSettings;
use crate::rng::DEFAULT_SEED;
use crate::save::{SaveState, seed_memory_storage};
use crate::state::EngineState;

/// The compiled script file and its VM execution state.
pub struct ScriptRuntime {
    /// Merged global+stage script file.
    pub vm: Vm,
    /// Persistent VM registers and globals.
    pub vm_state: VmState,
}

/// A fully loaded, runnable scene.
pub struct Engine {
    /// Mutable state shared with the host.
    pub state: EngineState,
    /// Compiled scripts and VM state.
    pub scripts: ScriptRuntime,
    /// Per-frame input source (idle, scripted or raw platform state).
    pub input: EngineInput,
    /// Index of the `input.pressButton` global, when the GameConfig defines it.
    press_button_global: Option<usize>,
}

/// Summary of a completed run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// Final frame count.
    pub frames: u64,
    /// Final state hash.
    pub final_hash: String,
    /// `(frame, hash)` pairs when per-frame hashing was requested.
    pub frame_hashes: Vec<(u64, String)>,
    /// `(frame, audio hash)` for every executed frame.
    pub audio_hashes: Vec<(u64, String)>,
}

impl Engine {
    /// Loads and instantiates the requested scene with in-memory user data.
    ///
    /// The shipped `SData.bin`/`SGame.bin`/`Achievements.bin` files next to the game data are
    /// copied into the in-memory storage first, mirroring upstream's `gamePath` lookup, so a
    /// headless run can read a shipped save without writing to the asset tree.
    pub fn load(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: Option<&str>,
        seed: u32,
    ) -> Result<Self, EngineError> {
        let storage = Box::new(seed_memory_storage(source.as_ref()));
        Self::load_with(source, requested_scene, act, seed, storage)
    }

    /// Loads and instantiates the requested scene with an explicit user-data storage.
    ///
    /// The backend is probed by [`SaveState::open`]; `ReadSaveRAM`/`WriteSaveRAM` then operate on
    /// it and a dirty save RAM is flushed on [`Engine::flush_save`].
    pub fn load_with(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: Option<&str>,
        seed: u32,
        save_storage: Box<dyn Storage>,
    ) -> Result<Self, EngineError> {
        Self::load_with_options(
            source,
            requested_scene,
            act,
            seed,
            save_storage,
            loader::LoadOptions::default(),
        )
    }

    /// Loads and instantiates a scene with explicit loader options.
    ///
    /// `options.origins` (the CLI's `--origins`) compiles the scripts with the Origins platform
    /// tag; the default is standalone. Everything else matches [`Engine::load_with`].
    pub fn load_with_options(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: Option<&str>,
        seed: u32,
        save_storage: Box<dyn Storage>,
        options: loader::LoadOptions,
    ) -> Result<Self, EngineError> {
        let world = loader::load_world_with(&source, requested_scene, act, options)?;
        let rng = crate::rng::GlibcRand::new(seed);
        let file: ScriptFile = world.scripts.file;
        let input = EngineInput::new(&world.raw_settings);
        let audio = AudioState::for_scene(
            Arc::clone(&source),
            &world.game_config,
            &world.stage_config,
            &world.raw_settings.audio,
        );
        let save = SaveState::open(save_storage)?;
        let mut state = EngineState::new(
            Arc::clone(&source),
            world.settings,
            world.game_config,
            world.stage_folder,
            world.act,
            world.scene,
            world.stage_config,
            world.collision,
            world.backgrounds,
            world.scripts.objects,
            rng,
        );
        state.audio = audio;
        state.save = save;
        let act_id = state.act.clone();
        state.stage.set_act_id(&act_id);
        // `[Window] DimLimit` is stored in seconds and converted to frames when settings load.
        state.render.dim_limit = state.settings.dim_limit_frames;
        state.apply_game_palette();
        state.apply_stage_palette();
        if let Some(tiles16) = &world.tiles16 {
            state.apply_tile_sheet(tiles16);
        }
        if let Some(tiles128) = &world.tiles128 {
            state
                .render
                .tiles
                .chunks
                .extend(tiles128.entries.iter().map(|entry| ChunkEntry {
                    gfx_data_pos: entry.gfx_data_pos(),
                    direction: entry.direction,
                    visual_plane: entry.visual_plane,
                }));
        }
        state.entities.reset_scene();
        place_scene_entities(&mut state.entities, &state.scene.entities);

        let global_variables = state
            .game_config
            .global_variables
            .iter()
            .map(|variable| variable.value)
            .collect();
        let mut array_position = [0i32; 9];
        array_position[8] = TEMPENTITY_START as i32;
        let vm_state = VmState {
            global_variables,
            array_position,
            ..VmState::default()
        };
        let press_button_global = state
            .game_config
            .global_variables
            .iter()
            .position(|variable| variable.name == "input.pressButton");
        let mut engine = Self {
            state,
            scripts: ScriptRuntime {
                vm: Vm::new(file),
                vm_state,
            },
            input,
            press_button_global,
        };
        engine.run_startup()?;
        Ok(engine)
    }

    /// Loads with the default seed.
    pub fn load_default(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: Option<&str>,
    ) -> Result<Self, EngineError> {
        Self::load(source, requested_scene, act, DEFAULT_SEED)
    }

    /// Runs startup events for every registered object type, in object-list order.
    pub fn run_startup(&mut self) -> Result<(), EngineError> {
        self.state.object_entity_pos = TEMPENTITY_START;
        self.scripts.vm_state.array_position[8] = TEMPENTITY_START as i32;
        self.scripts.vm_state.foreach_stack.clear();
        self.scripts.vm_state.current_event = ScriptEvent::Setup;
        // Upstream copies slot 0's type into the second temp slot before the startup pass
        // ("Dunno what this is meant for, but it's here in the original code so...").
        if let Some(type_id) = self.state.entities.get(0).map(|entity| entity.type_id)
            && let Some(entity) = self.state.entities.get_mut(TEMPENTITY_START + 1)
        {
            entity.type_id = type_id;
        }
        for type_index in 0..OBJECT_COUNT {
            let type_id = type_index as u8;
            if let Some(entity) = self.state.entities.get_mut(TEMPENTITY_START) {
                entity.type_id = type_id;
            }
            let Some(entry) = self.state.objects.get(usize::from(type_id)) else {
                continue;
            };
            let pointer = entry.script.startup;
            if !self.script_exists(pointer.code_pos) {
                continue;
            }
            self.state.object_entity_pos = TEMPENTITY_START;
            self.scripts.vm_state.current_event = ScriptEvent::Setup;
            self.run_host_event(pointer.code_pos, pointer.jump_pos)?;
        }
        if let Some(entity) = self.state.entities.get_mut(TEMPENTITY_START) {
            entity.type_id = 0;
        }
        Ok(())
    }

    /// Runs one 60 Hz frame.
    ///
    /// Ordering matches `ProcessStage`'s `STAGEMODE_NORMAL`: fade decay, clock, object updates,
    /// camera follow, parallax auto-scroll, then `DrawStageGFX` (which runs `ObjectDraw` events
    /// through the draw lists, interleaved with the tile layers) and the fade rectangle.
    pub fn run_frame(&mut self) -> Result<(), EngineError> {
        self.poll_input();
        // `ProcessInput` runs before the frame: any press/hold resets the idle-dimming timer,
        // otherwise it advances towards `dim_limit` (`Input.cpp:377-382`). Presentation-only.
        let input_active = self.state.input.any_button()
            || self.state.input_press.any_button()
            || self
                .state
                .touch_down
                .iter()
                .filter(|down| **down != 0)
                .count()
                > 1;
        self.state.render.update_dim_timer(input_active, false);
        if self.state.render.fade_mode > 0 {
            self.state.render.fade_mode -= 1;
        }
        self.update_clock();
        self.process_objects()?;
        self.update_camera();
        self.process_parallax_auto_scroll();
        self.draw_stage_gfx()?;
        // `FlipScreen` updates the display-only dim state after the frame is composed.
        self.state.render.process_dimming();
        self.state.frame += 1;
        // Mix one engine tick (735 stereo frames) and, in windowed runs, submit it to the device.
        self.state.audio.tick();
        Ok(())
    }

    /// Polls the input source and copies the player states into the engine state.
    fn poll_input(&mut self) {
        let players = self.input.poll();
        apply_players(&mut self.state, &players);
        if let Some(index) = self.press_button_global {
            let pressed = players
                .first()
                .is_some_and(|player| player.pressed.intersects(PRESS_BUTTONS));
            if let Some(value) = self.scripts.vm_state.global_variables.get_mut(index) {
                *value = i32::from(pressed);
            }
        }
    }

    /// Selects the deterministic idle input source (the headless default).
    pub fn set_null_input(&mut self) {
        self.input.set_null();
    }

    /// Selects a deterministic scripted replay (`--input FILE`), overriding any platform source.
    pub fn set_scripted_input(&mut self, input: ScriptedInput) {
        self.input.set_scripted(input);
    }

    /// Selects raw platform polling through the `Settings.ini` mappings (windowed default).
    pub fn set_platform_input(&mut self) {
        self.input.set_platform();
    }

    /// Stores one raw platform poll for the next frame (windowed mode only).
    pub fn set_raw_input(&mut self, raw: RawInput) {
        self.input.set_raw(raw);
    }

    /// Attaches the windowed audio output device.
    pub fn set_audio_device(&mut self, device: AudioEngine) {
        self.state.audio.set_device(device);
    }

    /// Enables or disables audio output; mixing and hashing are unaffected.
    pub fn set_muted(&mut self, muted: bool) {
        self.state.audio.set_muted(muted);
    }

    /// The hash of the audio mixed for the most recent frame, as lower-case hex.
    #[must_use]
    pub fn audio_hash(&self) -> String {
        self.state.audio.last_hash_hex()
    }

    /// Persists a dirty save RAM (called at exit; `WriteSaveRAM` writes immediately).
    pub fn flush_save(&mut self) -> bool {
        self.state.save.flush()
    }

    /// `ProcessParallaxAutoScroll`.
    fn process_parallax_auto_scroll(&mut self) {
        for table in [&mut self.state.h_parallax, &mut self.state.v_parallax] {
            for index in 0..table.entry_count.min(retro_render::PARALLAX_COUNT) {
                if let (Some(position), Some(speed)) = (
                    table.scroll_pos.get_mut(index),
                    table.scroll_speed.get(index).copied(),
                ) {
                    *position = position.wrapping_add(speed);
                }
            }
        }
    }

    /// `DrawStageGFX`: runs the draw lists in upstream order around the tile layers.
    fn draw_stage_gfx(&mut self) -> Result<(), EngineError> {
        let water_level = self.state.stage.water_level;
        let y_scroll = self.state.screen.y_scroll;
        let screen_height = self.state.render.framebuffer.height() as i32;
        self.state.render.water_draw_pos = (water_level - y_scroll).clamp(0, screen_height);
        let mid_point = self.state.stage.mid_point;
        if mid_point < 3 {
            self.draw_object_list(0)?;
            self.draw_tile_layer(0, mid_point);
            self.draw_object_list(1)?;
            self.draw_tile_layer(1, mid_point);
            self.draw_object_list(2)?;
            self.draw_object_list(3)?;
            self.draw_object_list(4)?;
            self.draw_tile_layer(2, mid_point);
        } else if mid_point < 6 {
            self.draw_object_list(0)?;
            self.draw_tile_layer(0, mid_point);
            self.draw_object_list(1)?;
            self.draw_tile_layer(1, mid_point);
            self.draw_object_list(2)?;
            self.draw_tile_layer(2, mid_point);
            self.draw_object_list(3)?;
            self.draw_object_list(4)?;
        }
        if mid_point < 6 {
            self.draw_tile_layer(3, mid_point);
            self.draw_object_list(5)?;
            // RETRO_REV03/Origins ordering also runs draw list 7.
            self.draw_object_list(7)?;
            self.draw_object_list(6)?;
        }
        self.state.render.draw_fade();
        Ok(())
    }

    /// `DrawObjectList`: runs `ObjectDraw` for every entity in `layer`'s draw list.
    fn draw_object_list(&mut self, layer: usize) -> Result<(), EngineError> {
        let Some(slot) = self.state.draw_lists.get_mut(layer) else {
            return Ok(());
        };
        let list = std::mem::take(slot);
        let result = self.run_draw_list(&list);
        if let Some(target) = self.state.draw_lists.get_mut(layer) {
            *target = list;
        }
        result
    }

    fn run_draw_list(&mut self, list: &[i32]) -> Result<(), EngineError> {
        for &slot in list {
            let Some(entity) = self
                .state
                .entities
                .get(usize::try_from(slot).unwrap_or(usize::MAX))
                .copied()
            else {
                continue;
            };
            if entity.type_id == 0 {
                continue;
            }
            self.state.object_entity_pos = usize::try_from(slot).unwrap_or(0);
            let Some(entry) = self.state.objects.get(usize::from(entity.type_id)) else {
                continue;
            };
            let draw = entry.script.draw;
            if self.script_exists(draw.code_pos) {
                self.scripts.vm_state.current_event = ScriptEvent::Draw;
                self.run_host_event(draw.code_pos, draw.jump_pos)?;
            }
        }
        Ok(())
    }

    /// Draws one of the four active tile layers, recording 3D layers as stubs.
    fn draw_tile_layer(&mut self, layer_id: usize, mid_point: i32) {
        let Some(active) = self.state.stage.active_layers.get(layer_id).copied() else {
            return;
        };
        let Ok(index) = usize::try_from(active) else {
            return;
        };
        let Some(layer) = self.state.layers.get_mut(index) else {
            return;
        };
        let view = LayerView {
            is_background: active != 0,
            above_mid_point: layer_id as i32 >= mid_point,
            x_scroll_offset: self.state.screen.x_scroll,
            y_scroll_offset: self.state.screen.y_scroll,
        };
        match layer.layer_type {
            LAYER_HSCROLL => retro_render::layers::draw_h_line_scroll_layer(
                &mut self.state.render,
                layer,
                &mut self.state.h_parallax,
                view,
            ),
            LAYER_VSCROLL => retro_render::layers::draw_v_line_scroll_layer(
                &mut self.state.render,
                layer,
                &mut self.state.v_parallax,
                view,
            ),
            LAYER_3DFLOOR | LAYER_3DSKY => {
                self.state.record_stub("Draw3DLayer");
            }
            _ => {}
        }
    }

    /// Runs `frames` frames, optionally hashing every frame.
    pub fn run_frames(
        &mut self,
        frames: u64,
        hash_every_frame: bool,
    ) -> Result<RunOutcome, EngineError> {
        let mut frame_hashes = Vec::new();
        let mut audio_hashes = Vec::with_capacity(frames as usize);
        if hash_every_frame {
            frame_hashes.push((self.state.frame, self.state_hash()));
        }
        for _ in 0..frames {
            self.run_frame()?;
            audio_hashes.push((self.state.frame, self.audio_hash()));
            if hash_every_frame {
                frame_hashes.push((self.state.frame, self.state_hash()));
            }
        }
        Ok(RunOutcome {
            frames: self.state.frame,
            final_hash: self.state_hash(),
            frame_hashes,
            audio_hashes,
        })
    }

    fn script_exists(&self, code_pos: u32) -> bool {
        usize::try_from(code_pos)
            .ok()
            .and_then(|index| self.scripts.vm.file().code.get(index))
            .is_some_and(|word| *word > 0)
    }

    fn run_host_event(&mut self, code_pos: u32, jump_pos: u32) -> Result<(), EngineError> {
        let Engine { state, scripts, .. } = self;
        let mut host = EngineHost { state };
        scripts
            .vm
            .call_at(&mut host, code_pos, jump_pos, &mut scripts.vm_state)?;
        Ok(())
    }

    fn update_clock(&mut self) {
        if self.state.stage.time_enabled {
            self.state.stage.frame_counter += 1;
            if self.state.stage.frame_counter == 60 {
                self.state.stage.frame_counter = 0;
                self.state.stage.seconds += 1;
                if self.state.stage.seconds > 59 {
                    self.state.stage.seconds = 0;
                    self.state.stage.minutes += 1;
                    if self.state.stage.minutes > 59 {
                        self.state.stage.minutes = 0;
                    }
                }
            }
            self.state.stage.milliseconds = 100 * self.state.stage.frame_counter / 60;
        } else {
            self.state.stage.frame_counter = 60 * self.state.stage.milliseconds / 100;
        }
    }

    /// Ports the active-entity check from `ProcessObjects`.
    fn process_objects(&mut self) -> Result<(), EngineError> {
        for list in &mut self.state.draw_lists {
            list.clear();
        }
        let screen = self.state.screen;
        let borders = self.state.object_borders;
        for slot in 0..ENTITY_COUNT {
            self.state.process_flags[slot] = false;
            let Some(entity) = self.state.entities.get(slot).copied() else {
                continue;
            };
            let x = entity.xpos >> 16;
            let y = entity.ypos >> 16;
            let active = match entity.priority {
                0 => {
                    x > screen.x_scroll - borders[0]
                        && x < screen.x_scroll + borders[1]
                        && y > screen.y_scroll - 0x100
                        && y < screen.y_scroll + screen.ysize + 0x100
                }
                1 | 2 | 7 => true,
                3 => x > screen.x_scroll - borders[0] && x < screen.x_scroll + borders[1],
                4 => {
                    let active =
                        x > screen.x_scroll - borders[0] && x < screen.x_scroll + borders[1];
                    if !active && let Some(entity) = self.state.entities.get_mut(slot) {
                        entity.type_id = 0;
                    }
                    active
                }
                5 => false,
                6 => {
                    x > screen.x_scroll - borders[2]
                        && x < screen.x_scroll + borders[3]
                        && y > screen.y_scroll - 0x80
                        && y < screen.y_scroll + screen.ysize + 0x80
                }
                _ => false,
            };
            self.state.process_flags[slot] = active;
            if !active || entity.type_id == 0 {
                continue;
            }
            let Some(entry) = self.state.objects.get(usize::from(entity.type_id)) else {
                continue;
            };
            let update = entry.script.update;
            if self.script_exists(update.code_pos) {
                self.state.object_entity_pos = slot;
                self.scripts.vm_state.current_event = ScriptEvent::Main;
                self.run_host_event(update.code_pos, update.jump_pos)?;
            }
            let draw_order = usize::from(
                self.state
                    .entities
                    .get(slot)
                    .map(|entity| entity.draw_order)
                    .unwrap_or(0),
            );
            if draw_order < DRAWLAYER_COUNT
                && let Some(list) = self.state.draw_lists.get_mut(draw_order)
            {
                list.push(slot as i32);
            }
        }
        let object_count = self.state.objects.len().max(OBJECT_COUNT);
        let flags = std::mem::take(&mut self.state.process_flags);
        self.state
            .entities
            .build_type_groups(&flags, &mut self.state.type_groups, object_count);
        self.state.process_flags = flags;
        Ok(())
    }

    /// Deterministic simplified camera follow.
    fn update_camera(&mut self) {
        let target = usize::try_from(self.state.camera.target).ok();
        if self.state.camera.enabled == 1
            && let Some(target) = target
            && let Some(entity) = self.state.entities.get(target).copied()
        {
            let half_x = self.state.screen.center_x();
            let half_y = self.state.screen.center_y();
            let target_x = entity.xpos >> 16;
            let target_y = (entity.ypos >> 16) + self.state.camera.adjust_y;
            let min_x = self.state.stage.cur_x_boundary1.wrapping_add(half_x);
            let max_x = self.state.stage.cur_x_boundary2.wrapping_sub(half_x);
            let min_y = self.state.stage.cur_y_boundary1.wrapping_add(half_y);
            let max_y = self.state.stage.cur_y_boundary2.wrapping_sub(half_y);
            self.state.camera.xpos = if min_x <= max_x {
                target_x.clamp(min_x, max_x)
            } else {
                target_x
            };
            self.state.camera.ypos = if min_y <= max_y {
                target_y.clamp(min_y, max_y)
            } else {
                target_y
            };
        }
        self.state.screen.x_scroll =
            self.state.camera.shake_x + self.state.camera.xpos - self.state.screen.center_x();
        self.state.screen.y_scroll =
            self.state.camera.shake_y + self.state.camera.ypos - self.state.screen.center_y();
    }

    /// Hashes the canonical engine state.
    ///
    /// Tile layer data is hashed for the populated `xsize * ysize` region of each layer; the
    /// zero-initialised remainder of the engine's `0x100`-wide buffers is not serialised.
    #[must_use]
    pub fn state_hash(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        let mut scratch = Vec::with_capacity(512);
        put_u64(&mut hasher, self.state.frame);
        for word in self.state.rng.state() {
            hasher.update(&word.to_le_bytes());
        }
        let (front, rear) = self.state.rng.pointers();
        put_u64(&mut hasher, front as u64);
        put_u64(&mut hasher, rear as u64);
        put_i32(&mut hasher, self.state.object_entity_pos as i32);
        for slot in 0..self.state.entities.len() {
            let entity = self.state.entities.get_or_blank(slot);
            scratch.clear();
            write_entity(&mut scratch, entity);
            hasher.update(&scratch);
        }
        for (index, entry) in self.state.objects.iter_enumerated() {
            put_i32(&mut hasher, index as i32);
            put_bytes(&mut hasher, entry.name.as_bytes());
            put_i32(&mut hasher, entry.animation_file.map_or(-1, |id| id as i32));
            put_i32(&mut hasher, entry.sprite_sheet_id);
            put_i32(&mut hasher, entry.script.update.code_pos as i32);
            put_i32(&mut hasher, entry.script.update.jump_pos as i32);
            put_i32(&mut hasher, entry.script.draw.code_pos as i32);
            put_i32(&mut hasher, entry.script.draw.jump_pos as i32);
            put_i32(&mut hasher, entry.script.startup.code_pos as i32);
            put_i32(&mut hasher, entry.script.startup.jump_pos as i32);
        }
        let camera = &self.state.camera;
        for value in [
            camera.enabled,
            camera.target,
            camera.style,
            camera.xpos,
            camera.ypos,
            camera.adjust_y,
            camera.shake_x,
            camera.shake_y,
        ] {
            put_i32(&mut hasher, value);
        }
        put_bytes(&mut hasher, self.state.scene.title.as_bytes());
        for value in [
            i32::from(self.state.scene.mid_point),
            self.state.screen.x_scroll,
            self.state.screen.y_scroll,
            self.state.stage.state,
            self.state.stage.active_list,
            self.state.stage.list_pos,
            i32::from(self.state.stage.time_enabled),
            self.state.stage.act_num,
            i32::from(self.state.stage.pause_enabled),
            self.state.stage.list_size,
            self.state.stage.new_x_boundary1,
            self.state.stage.new_x_boundary2,
            self.state.stage.new_y_boundary1,
            self.state.stage.new_y_boundary2,
            self.state.stage.cur_x_boundary1,
            self.state.stage.cur_x_boundary2,
            self.state.stage.cur_y_boundary1,
            self.state.stage.cur_y_boundary2,
            self.state.stage.water_level,
            self.state.stage.mid_point,
            self.state.stage.player_list_pos,
            self.state.stage.debug_mode,
            self.state.stage.milliseconds,
            self.state.stage.seconds,
            self.state.stage.minutes,
            self.state.stage.frame_counter,
            self.state.music_track,
            self.state.menu1_selection,
            self.state.menu2_selection,
            i32::from(self.state.load_stage_requested),
        ] {
            put_i32(&mut hasher, value);
        }
        for (down, (x, y)) in self
            .state
            .touch_down
            .iter()
            .zip(self.state.touch_x.iter().zip(self.state.touch_y.iter()))
        {
            put_i32(&mut hasher, *down);
            put_i32(&mut hasher, *x);
            put_i32(&mut hasher, *y);
        }
        for layer in &self.state.stage.active_layers {
            put_i32(&mut hasher, *layer);
        }
        for value in self.scripts.vm_state.global_variables.iter() {
            put_i32(&mut hasher, *value);
        }
        for value in self.scripts.vm_state.temp {
            put_i32(&mut hasher, value);
        }
        put_i32(&mut hasher, self.scripts.vm_state.check_result);
        for value in self.scripts.vm_state.array_position {
            put_i32(&mut hasher, value);
        }
        for value in &self.scripts.vm_state.foreach_stack {
            put_i32(&mut hasher, *value);
        }
        for value in self.state.object_borders {
            put_i32(&mut hasher, value);
        }
        put_i32(&mut hasher, self.state.screen.xsize);
        put_i32(&mut hasher, self.state.screen.ysize);
        for layer in &self.state.layers {
            for value in [
                layer.xsize,
                layer.ysize,
                layer.layer_type,
                layer.angle,
                layer.xpos,
                layer.ypos,
                layer.zpos,
                layer.parallax_factor,
                layer.scroll_speed,
                layer.scroll_pos,
                layer.deformation_offset,
                layer.deformation_offset_w,
            ] {
                put_i32(&mut hasher, value);
            }
            scratch.clear();
            let width = layer.xsize.max(0);
            let height = layer.ysize.max(0);
            for y in 0..height {
                for x in 0..width {
                    scratch.extend_from_slice(&layer.entry(x, y).to_le_bytes());
                }
            }
            put_u64(&mut hasher, scratch.len() as u64);
            hasher.update(&scratch);
        }
        for table in [&self.state.h_parallax, &self.state.v_parallax] {
            for value in &table.parallax_factor {
                put_i32(&mut hasher, *value);
            }
            for value in &table.scroll_speed {
                put_i32(&mut hasher, *value);
            }
            for value in &table.scroll_pos {
                put_i32(&mut hasher, *value);
            }
        }
        for list in &self.state.draw_lists {
            put_i32(&mut hasher, list.len() as i32);
            for value in list {
                put_i32(&mut hasher, *value);
            }
        }
        // The software framebuffer is part of the canonical state: every backing pixel
        // (`pitch * height`, including the padding columns) is hashed as little-endian u16.
        self.state.render.framebuffer.hash_into(&mut hasher);
        hasher.finalize().to_hex().to_string()
    }

    /// Returns the stub/unknown op histogram as `name -> count`.
    #[must_use]
    pub fn stub_histogram(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.state.stub_histogram
    }

    /// Returns the full engine-op histogram as `name -> count`.
    #[must_use]
    pub fn op_histogram(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.state.op_histogram
    }

    /// The loaded stage folder and act.
    #[must_use]
    pub fn stage_info(&self) -> (&str, &str) {
        (&self.state.stage_folder, &self.state.act)
    }

    /// The resolved engine settings.
    #[must_use]
    pub fn settings(&self) -> &EngineSettings {
        &self.state.settings
    }

    /// The game config title.
    #[must_use]
    pub fn game_title(&self) -> &str {
        &self.state.game_config.title
    }

    /// The currently rendered framebuffer.
    #[must_use]
    pub fn framebuffer(&self) -> &retro_render::Framebuffer {
        &self.state.render.framebuffer
    }
}

fn place_scene_entities(store: &mut EntityStore, entities: &[SceneEntity]) {
    for (index, source) in entities.iter().enumerate() {
        let Some(slot) = SCENE_ENTITY_START.checked_add(index) else {
            break;
        };
        let Some(entity) = store.get_mut(slot) else {
            break;
        };
        entity.type_id = source.type_id;
        entity.property_value = source.property_value;
        entity.xpos = source.x;
        entity.ypos = source.y;
        if source.has(ENTITY_ATTRIB_STATE) {
            entity.state = source.state.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_DIRECTION) {
            entity.direction = source.direction.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_SCALE) {
            entity.scale = source.scale.unwrap_or(512);
        }
        if source.has(ENTITY_ATTRIB_ROTATION) {
            entity.rotation = source.rotation.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_DRAW_ORDER) {
            entity.draw_order = source.draw_order.unwrap_or(3);
        }
        if source.has(ENTITY_ATTRIB_PRIORITY) {
            entity.priority = source.priority.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_ALPHA) {
            entity.alpha = i32::from(source.alpha.unwrap_or(0));
        }
        if source.has(ENTITY_ATTRIB_ANIMATION) {
            entity.animation = source.animation.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_ANIMATION_SPEED) {
            entity.animation_speed = source.animation_speed.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_FRAME) {
            entity.frame = source.frame.unwrap_or(0);
        }
        if source.has(ENTITY_ATTRIB_INK_EFFECT) {
            entity.ink_effect = source.ink_effect.unwrap_or(0);
        }
        for (value_index, attribute) in ENTITY_ATTRIB_VALUES.into_iter().enumerate() {
            if source.has(attribute)
                && let Some(value) = source.value(value_index)
            {
                entity.values[value_index] = value;
            }
        }
    }
}

fn write_entity(buffer: &mut Vec<u8>, entity: &retro_scene::Entity) {
    for value in [
        entity.xpos,
        entity.ypos,
        entity.xvel,
        entity.yvel,
        entity.speed,
        entity.state,
        entity.angle,
        entity.scale,
        entity.rotation,
        entity.alpha,
        entity.animation_timer,
        entity.animation_speed,
        entity.look_pos_x,
        entity.look_pos_y,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in entity.values {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    buffer.extend_from_slice(&i32::from(entity.group_id).to_le_bytes());
    for value in [
        entity.type_id,
        entity.property_value,
        entity.priority,
        entity.draw_order,
        entity.direction,
        entity.ink_effect,
        entity.animation,
        entity.prev_animation,
        entity.frame,
        entity.collision_mode,
        entity.collision_plane,
        entity.control_mode as u8,
        entity.control_lock,
        entity.pushing,
        entity.visible,
        entity.tile_collisions,
        entity.object_interactions,
        entity.gravity,
        entity.left,
        entity.right,
        entity.up,
        entity.down,
        entity.jump_press,
        entity.jump_hold,
        entity.scroll_tracking,
    ] {
        buffer.push(value);
    }
    buffer.extend_from_slice(&entity.floor_sensors);
}

fn put_i32(hasher: &mut blake3::Hasher, value: i32) {
    hasher.update(&value.to_le_bytes());
}

fn put_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn put_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    put_u64(hasher, bytes.len() as u64);
    hasher.update(bytes);
}

/// Seed used when the caller does not override it.
pub const DEFAULT_RNG_SEED: u32 = DEFAULT_SEED;
