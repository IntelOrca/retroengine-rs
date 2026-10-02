//! Engine-side input: turns a [`retro_input::InputSource`] or raw platform state into the VM's
//! per-frame `keyDown`/`keyPress` and touchscreen variables.
//!
//! The default is [`retro_input::NullInput`], so headless runs are deterministic and idle. A
//! windowed run uses [`InputMode::Platform`], where the CLI feeds one
//! [`retro_platform::RawInput`] poll per frame and [`retro_input::InputMappings`] (built from the
//! asset `Settings.ini`) maps scancodes/gamepads into player states. `--input FILE` overrides
//! either mode with a deterministic [`retro_input::ScriptedInput`] replay.
//!
//! [`apply_players`] copies player 0 into the legacy `keyDown`/`keyPress` fields the host reads,
//! aggregates every player's touches into the global touchscreen arrays (capped at the engine's
//! slot count), and leaves the `input.pressButton` global to the caller, which owns the VM state.

use retro_format_v4::Settings;
use retro_input::{
    ButtonState, InputMappings, InputSource, NullInput, PLAYER_COUNT, ScriptedInput, idle_states,
};
use retro_platform::RawInput;

use crate::state::EngineState;

/// Buttons that set the rev03 `input.pressButton` global (`CheckKeyPress`).
pub const PRESS_BUTTONS: ButtonState = ButtonState::A
    .union(ButtonState::B)
    .union(ButtonState::C)
    .union(ButtonState::X)
    .union(ButtonState::Y)
    .union(ButtonState::Z)
    .union(ButtonState::L)
    .union(ButtonState::R)
    .union(ButtonState::START)
    .union(ButtonState::SELECT);

/// How [`EngineInput`] produces per-frame player states.
enum InputMode {
    /// Deterministic idle input.
    Null(NullInput),
    /// Replay from a scripted input file.
    Scripted(ScriptedInput),
    /// Raw device state supplied by the platform each frame.
    Platform,
}

/// Per-frame input for all four players.
pub struct EngineInput {
    mode: InputMode,
    mappings: InputMappings,
    raw: RawInput,
    states: [retro_input::InputState; PLAYER_COUNT],
}

impl EngineInput {
    /// Builds an idle input source with `Settings.ini` mappings ready for platform mode.
    #[must_use]
    pub fn new(settings: &Settings) -> Self {
        Self {
            mode: InputMode::Null(NullInput::new()),
            mappings: InputMappings::from_settings(settings),
            raw: RawInput::default(),
            states: idle_states(),
        }
    }

    /// Selects the deterministic idle source.
    pub fn set_null(&mut self) {
        self.mode = InputMode::Null(NullInput::new());
        self.states = idle_states();
    }

    /// Selects a scripted replay, overriding any platform source.
    pub fn set_scripted(&mut self, input: ScriptedInput) {
        self.mode = InputMode::Scripted(input);
    }

    /// Selects raw platform polling through the `Settings.ini` mappings.
    pub fn set_platform(&mut self) {
        self.mode = InputMode::Platform;
    }

    /// Whether the CLI must feed [`EngineInput::set_raw`] before each frame.
    #[must_use]
    pub fn uses_platform_input(&self) -> bool {
        matches!(self.mode, InputMode::Platform)
    }

    /// Stores one raw platform poll for the next [`EngineInput::poll`].
    pub fn set_raw(&mut self, raw: RawInput) {
        self.raw = raw;
    }

    /// Produces the states for the current frame.
    pub fn poll(&mut self) -> [retro_input::InputState; PLAYER_COUNT] {
        self.states = match &mut self.mode {
            InputMode::Null(_) => idle_states(),
            InputMode::Scripted(scripted) => scripted.poll(),
            InputMode::Platform => {
                let mut states = std::array::from_fn(|index| {
                    self.mappings
                        .apply(index as u8, &self.raw.keys, &self.raw.gamepads[index])
                });
                // Touches are global upstream (`touchDown[8]`); attach the raw points to slot 0,
                // which `apply_players` aggregates into the engine's touchscreen arrays.
                let count = usize::from(self.raw.touch_count).min(retro_input::MAX_TOUCHES);
                let points: Vec<(i16, i16)> = self
                    .raw
                    .touches
                    .iter()
                    .take(count)
                    .filter(|touch| touch.down)
                    .map(|touch| (touch.x, touch.y))
                    .collect();
                if let Some(first) = states.first_mut() {
                    first.set_touches(&points);
                }
                states
            }
        };
        self.states
    }

    /// The states produced by the most recent [`EngineInput::poll`].
    #[must_use]
    pub fn states(&self) -> &[retro_input::InputState; PLAYER_COUNT] {
        &self.states
    }

    /// The `Settings.ini` keyboard mappings in use.
    #[must_use]
    pub fn mappings(&self) -> &InputMappings {
        &self.mappings
    }
}

impl Default for EngineInput {
    fn default() -> Self {
        Self::new(&Settings::default())
    }
}

/// Copies the per-player states into the legacy engine input fields.
///
/// Player 0 drives `keyDown`/`keyPress`; every player's active touches are appended to the global
/// touchscreen arrays in slot order, capped at the engine's array length.
pub fn apply_players(state: &mut EngineState, players: &[retro_input::InputState; PLAYER_COUNT]) {
    if let Some(player) = players.first() {
        state.input = held_state(player);
        state.input_press = pressed_state(player);
    }
    let capacity = state
        .touch_down
        .len()
        .min(state.touch_x.len())
        .min(state.touch_y.len());
    let mut count = 0usize;
    for player in players {
        for touch in player.touches.iter().take(usize::from(player.touch_count)) {
            if count >= capacity {
                break;
            }
            state.touch_down[count] = i32::from(touch.down);
            state.touch_x[count] = i32::from(touch.x);
            state.touch_y[count] = i32::from(touch.y);
            count += 1;
        }
    }
    for index in count..capacity {
        state.touch_down[index] = 0;
        state.touch_x[index] = 0;
        state.touch_y[index] = 0;
    }
}

fn held_state(player: &retro_input::InputState) -> crate::state::InputState {
    let held = player.held;
    crate::state::InputState {
        up: held.contains(ButtonState::UP),
        down: held.contains(ButtonState::DOWN),
        left: held.contains(ButtonState::LEFT),
        right: held.contains(ButtonState::RIGHT),
        button_a: held.contains(ButtonState::A),
        button_b: held.contains(ButtonState::B),
        button_c: held.contains(ButtonState::C),
        button_x: held.contains(ButtonState::X),
        button_y: held.contains(ButtonState::Y),
        button_z: held.contains(ButtonState::Z),
        button_l: held.contains(ButtonState::L),
        button_r: held.contains(ButtonState::R),
        start: held.contains(ButtonState::START),
        select: held.contains(ButtonState::SELECT),
    }
}

fn pressed_state(player: &retro_input::InputState) -> crate::state::InputState {
    let pressed = player.pressed;
    crate::state::InputState {
        up: pressed.contains(ButtonState::UP),
        down: pressed.contains(ButtonState::DOWN),
        left: pressed.contains(ButtonState::LEFT),
        right: pressed.contains(ButtonState::RIGHT),
        button_a: pressed.contains(ButtonState::A),
        button_b: pressed.contains(ButtonState::B),
        button_c: pressed.contains(ButtonState::C),
        button_x: pressed.contains(ButtonState::X),
        button_y: pressed.contains(ButtonState::Y),
        button_z: pressed.contains(ButtonState::Z),
        button_l: pressed.contains(ButtonState::L),
        button_r: pressed.contains(ButtonState::R),
        start: pressed.contains(ButtonState::START),
        select: pressed.contains(ButtonState::SELECT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EngineState;
    use retro_format_v4::Settings;
    use retro_input::{Button, InputState};
    use std::sync::Arc;

    /// The real `Settings.ini` convention: Origins ships Windows virtual-key codes.
    const SETTINGS_BYTES: &[u8] = b"\
[Game]\n\
gameType=1\n\
[Keyboard Map 1]\n\
up=0x26\n\
down=0x28\n\
left=0x25\n\
right=0x27\n\
buttonA=0x41\n\
buttonB=0x53\n\
buttonC=0x44\n\
buttonX=0x51\n\
buttonY=0x57\n\
buttonZ=0x45\n\
start=0xd\n\
select=0x9\n";

    fn settings() -> Settings {
        Settings::from_bytes(SETTINGS_BYTES).expect("settings parse")
    }

    fn raw_with(scancode: u32) -> RawInput {
        let mut keys = vec![false; retro_input::KEY_COUNT];
        keys[scancode as usize] = true;
        RawInput {
            keys,
            ..RawInput::default()
        }
    }

    #[test]
    fn default_input_is_idle_and_deterministic() {
        let mut input = EngineInput::new(&settings());
        assert_eq!(input.poll(), idle_states());
        assert_eq!(input.poll(), idle_states());
        assert!(!input.uses_platform_input());
    }

    #[test]
    fn platform_input_maps_real_settings_bytes_to_scancodes() {
        let mut input = EngineInput::new(&settings());
        assert_eq!(
            input.mappings().scancode_for(0, Button::Right),
            Some(79),
            "VK_RIGHT -> SDL_SCANCODE_RIGHT"
        );
        input.set_platform();
        assert!(input.uses_platform_input());

        input.set_raw(raw_with(79));
        let states = input.poll();
        assert!(states[0].held.contains(ButtonState::RIGHT));
        assert!(states[0].pressed.contains(ButtonState::RIGHT));
        assert!(!states[0].held.contains(ButtonState::LEFT));

        let states = input.poll();
        assert!(states[0].held.contains(ButtonState::RIGHT));
        assert!(
            states[0].pressed.is_empty(),
            "held is not re-reported as pressed"
        );

        input.set_raw(RawInput::default());
        let states = input.poll();
        assert!(states[0].held.is_empty());
    }

    #[test]
    fn scripted_input_overrides_platform_input() {
        let script = "retro-input 1\n0 A|RIGHT 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n";
        let mut input = EngineInput::new(&settings());
        input.set_platform();
        input.set_scripted(ScriptedInput::from_str(script).unwrap());
        assert!(!input.uses_platform_input());
        input.set_raw(raw_with(79));
        let states = input.poll();
        assert!(states[0].held.contains(ButtonState::A | ButtonState::RIGHT));
    }

    fn test_state() -> EngineState {
        use crate::profile::EngineSettings;
        use crate::rng::GlibcRand;
        use retro_format_v4::gameconfig::PALETTE_COUNT;
        use retro_format_v4::stageconfig::STAGE_PALETTE_COUNT;
        use retro_format_v4::{GameConfig, Scene, StageConfig};
        use retro_io::MemorySource;
        use retro_scene::ObjectRegistry;

        let scene = Scene {
            title: "Test".to_owned(),
            active_layers: [9, 9, 9, 9],
            mid_point: 3,
            width: 1,
            height: 1,
            layout: vec![0],
            entities: Vec::new(),
        };
        EngineState::new(
            Arc::new(MemorySource::new()),
            EngineSettings {
                profile: crate::RuntimeProfile::V4Legacy,
                platform: retro_script::PlatformMode::Origins,
                revision: retro_script::V4Revision::Rev03,
                force_scripts: false,
                dim_limit_frames: 18000,
            },
            GameConfig {
                title: "Test".to_owned(),
                subtitle: String::new(),
                palette: vec![[0, 0, 0]; PALETTE_COUNT],
                objects: Vec::new(),
                global_variables: Vec::new(),
                sound_effects: Vec::new(),
                players: Vec::new(),
                categories: Vec::new(),
            },
            "Zone01".to_owned(),
            "1".to_owned(),
            scene,
            StageConfig {
                load_global_objects: false,
                palette: vec![[0, 0, 0]; STAGE_PALETTE_COUNT],
                sound_effects: Vec::new(),
                objects: Vec::new(),
            },
            None,
            None,
            ObjectRegistry::new(),
            GlibcRand::new(1),
        )
    }

    #[test]
    fn apply_players_copies_buttons_and_touches() {
        let mut state = test_state();
        let mut player = InputState::new(0);
        player.held = ButtonState::RIGHT | ButtonState::A;
        player.pressed = ButtonState::A;
        player.set_touches(&[(10, 20), (30, 40)]);
        let mut second = InputState::new(1);
        second.set_touches(&[(50, 60)]);
        let players = [player, second, InputState::new(2), InputState::new(3)];

        apply_players(&mut state, &players);
        assert!(state.input.right);
        assert!(state.input.button_a);
        assert!(!state.input_press.right);
        assert!(state.input_press.button_a);
        assert_eq!(state.touch_down, vec![1, 1, 1, 0, 0, 0, 0, 0]);
        assert_eq!(state.touch_x, vec![10, 30, 50, 0, 0, 0, 0, 0]);
        assert_eq!(state.touch_y, vec![20, 40, 60, 0, 0, 0, 0, 0]);

        // Idle players clear the remaining slots.
        apply_players(&mut state, &idle_states());
        assert_eq!(state.touch_down, vec![0; crate::state::TOUCH_COUNT]);
        assert!(!state.input.right);
    }

    #[test]
    fn touches_fill_the_eight_engine_slots_and_are_capped() {
        let mut state = test_state();
        let mut player = InputState::new(0);
        player.set_touches(&[(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]);
        let mut second = InputState::new(1);
        second.set_touches(&[(7, 7), (8, 8)]);
        let mut third = InputState::new(2);
        third.set_touches(&[(9, 9)]);
        let players = [player, second, third, InputState::new(3)];
        apply_players(&mut state, &players);
        assert_eq!(state.touch_down, vec![1; crate::state::TOUCH_COUNT]);
        assert_eq!(state.touch_x, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(state.touch_y, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn platform_input_attaches_raw_touches_to_slot_zero() {
        let mut input = EngineInput::new(&settings());
        input.set_platform();
        let mut raw = RawInput {
            keys: vec![false; retro_input::KEY_COUNT],
            ..RawInput::default()
        };
        raw.touches[0] = retro_input::TouchPoint {
            down: true,
            x: 120,
            y: 80,
        };
        raw.touches[1] = retro_input::TouchPoint {
            down: true,
            x: 300,
            y: 200,
        };
        raw.touch_count = 2;
        input.set_raw(raw);

        let states = input.poll();
        assert_eq!(states[0].touch_count, 2);
        assert_eq!(
            states[0].touch(0),
            Some(retro_input::TouchPoint {
                down: true,
                x: 120,
                y: 80
            })
        );
        assert_eq!(
            states[0].touch(1),
            Some(retro_input::TouchPoint {
                down: true,
                x: 300,
                y: 200
            })
        );
        assert_eq!(states[1].touch_count, 0, "touches are global, not per slot");

        let mut state = test_state();
        apply_players(&mut state, &states);
        assert_eq!(state.touch_down[..2], [1, 1]);
        assert_eq!(state.touch_x[..2], [120, 300]);
        assert_eq!(state.touch_y[..2], [80, 200]);
    }
}
