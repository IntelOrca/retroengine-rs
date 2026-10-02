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

use retro_format_v4::SceneEntity;
use retro_format_v4::scene::{
    ENTITY_ATTRIB_ALPHA, ENTITY_ATTRIB_ANIMATION, ENTITY_ATTRIB_ANIMATION_SPEED,
    ENTITY_ATTRIB_DIRECTION, ENTITY_ATTRIB_DRAW_ORDER, ENTITY_ATTRIB_FRAME,
    ENTITY_ATTRIB_INK_EFFECT, ENTITY_ATTRIB_PRIORITY, ENTITY_ATTRIB_ROTATION, ENTITY_ATTRIB_SCALE,
    ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES,
};
use retro_io::DataSource;
use retro_scene::{
    DRAWLAYER_COUNT, ENTITY_COUNT, EntityStore, OBJECT_COUNT, SCENE_ENTITY_START, TEMPENTITY_START,
};
use retro_script::{ScriptEvent, ScriptFile, Vm, VmState};

use crate::EngineError;
use crate::host::EngineHost;
use crate::loader;
use crate::profile::EngineSettings;
use crate::rng::DEFAULT_SEED;
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
}

impl Engine {
    /// Loads and instantiates the requested scene.
    pub fn load(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: u32,
        seed: u32,
    ) -> Result<Self, EngineError> {
        let world = loader::load_world(&source, requested_scene, act)?;
        let rng = crate::rng::GameRng::new(seed);
        let file: ScriptFile = world.scripts.file;
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
        let mut engine = Self {
            state,
            scripts: ScriptRuntime {
                vm: Vm::new(file),
                vm_state,
            },
        };
        engine.run_startup()?;
        Ok(engine)
    }

    /// Loads with the default seed.
    pub fn load_default(
        source: Arc<dyn DataSource>,
        requested_scene: Option<&str>,
        act: u32,
    ) -> Result<Self, EngineError> {
        Self::load(source, requested_scene, act, DEFAULT_SEED)
    }

    /// Runs startup events for every registered object type, in object-list order.
    pub fn run_startup(&mut self) -> Result<(), EngineError> {
        self.state.object_entity_pos = TEMPENTITY_START;
        self.scripts.vm_state.array_position[8] = TEMPENTITY_START as i32;
        self.scripts.vm_state.foreach_stack.clear();
        self.scripts.vm_state.current_event = ScriptEvent::Setup;
        let scratch = self
            .state
            .entities
            .get(TEMPENTITY_START)
            .copied()
            .unwrap_or_default();
        let _ = scratch;
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
    pub fn run_frame(&mut self) -> Result<(), EngineError> {
        self.update_clock();
        self.process_objects()?;
        self.update_camera();
        self.state.frame += 1;
        Ok(())
    }

    /// Runs `frames` frames, optionally hashing every frame.
    pub fn run_frames(
        &mut self,
        frames: u64,
        hash_every_frame: bool,
    ) -> Result<RunOutcome, EngineError> {
        let mut frame_hashes = Vec::new();
        if hash_every_frame {
            frame_hashes.push((self.state.frame, self.state_hash()));
        }
        for _ in 0..frames {
            self.run_frame()?;
            if hash_every_frame {
                frame_hashes.push((self.state.frame, self.state_hash()));
            }
        }
        Ok(RunOutcome {
            frames: self.state.frame,
            final_hash: self.state_hash(),
            frame_hashes,
        })
    }

    fn script_exists(&self, code_pos: u32) -> bool {
        usize::try_from(code_pos)
            .ok()
            .and_then(|index| self.scripts.vm.file().code.get(index))
            .is_some_and(|word| *word > 0)
    }

    fn run_host_event(&mut self, code_pos: u32, jump_pos: u32) -> Result<(), EngineError> {
        let Engine { state, scripts } = self;
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
        put_u64(&mut hasher, self.state.rng.state());
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
            self.state.stage.cur_x_boundary1,
            self.state.stage.cur_x_boundary2,
            self.state.stage.cur_y_boundary1,
            self.state.stage.cur_y_boundary2,
            self.state.stage.water_level,
            self.state.stage.milliseconds,
            self.state.stage.seconds,
            self.state.stage.minutes,
            self.state.stage.frame_counter,
            self.state.music_track,
        ] {
            put_i32(&mut hasher, value);
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
