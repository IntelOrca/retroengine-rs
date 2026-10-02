//! Keyboard mapping from `Settings.ini` and conversion of raw device state to [`InputState`].
//!
//! # Upstream mapping
//!
//! `RSDKv4-Decompilation` indexes `SDL_GetKeyboardState` with `inputDevice[i].keyMappings`, where
//! the value read from `[Keyboard 1..4]` is compared against the SDL2 scancode space (see
//! `Input.cpp`'s `ProcessInput` and `Userdata.cpp`'s defaults). The `Settings.ini` files shipped
//! with the Origins asset set instead store Windows virtual-key codes (`VK_UP` = `0x26`,
//! `VK_RETURN` = `0x0D`, `'A'` = `0x41`, ...), so [`InputMappings::from_settings`] converts them
//! to SDL scancode numbers with [`vk_to_scancode`] before storing them. For decomp-generated
//! settings that already contain scancodes, use [`InputMappings::from_settings_scancodes`].
//!
//! [`InputMappings::apply`] indexes the caller's `keys` slice with those scancode numbers, so a
//! raw keyboard array can be handed straight from the platform layer without this crate knowing
//! anything about SDL types.

use std::cell::Cell;

use retro_format_v4::{KeyboardMap, Settings};

use crate::{
    Button, ButtonState, GamepadState, InputState, KEY_COUNT, PLAYER_COUNT, directions_from_axes,
};

/// Converts a Windows virtual-key code to an SDL scancode number (US layout).
///
/// Returns `None` for unmapped codes (including `0`, which the Origins settings use for unbound
/// keys) and negative values. Letters are layout-independent virtual-key codes mapped to their
/// physical SDL positions; OEM keys are mapped to the US layout.
#[must_use]
pub const fn vk_to_scancode(vk: i32) -> Option<u32> {
    Some(match vk {
        0x08 => 42,                             // Backspace
        0x09 => 43,                             // Tab
        0x0D => 40,                             // Return
        0x10 => 225,                            // Shift (generic)
        0x11 => 224,                            // Ctrl (generic)
        0x12 => 226,                            // Alt (generic)
        0x13 => 72,                             // Pause
        0x14 => 57,                             // Caps Lock
        0x1B => 41,                             // Escape
        0x20 => 44,                             // Space
        0x21 => 75,                             // Page Up
        0x22 => 78,                             // Page Down
        0x23 => 77,                             // End
        0x24 => 74,                             // Home
        0x25 => 80,                             // Left
        0x26 => 82,                             // Up
        0x27 => 79,                             // Right
        0x28 => 81,                             // Down
        0x2C => 70,                             // Print Screen
        0x2D => 73,                             // Insert
        0x2E => 76,                             // Delete
        0x30 => 39,                             // 0
        0x31..=0x39 => 29 + (vk - 0x30) as u32, // 1..9 on the top row
        0x41..=0x5A => 4 + (vk - 0x41) as u32,  // A..Z
        0x5B => 227,                            // Left GUI
        0x5C => 231,                            // Right GUI
        0x60 => 98,                             // Keypad 0
        0x61..=0x69 => 88 + (vk - 0x60) as u32, // Keypad 1..9
        0x6A => 85,                             // Keypad *
        0x6B => 87,                             // Keypad +
        0x6D => 86,                             // Keypad -
        0x6E => 99,                             // Keypad .
        0x6F => 84,                             // Keypad /
        0x70..=0x7B => 58 + (vk - 0x70) as u32, // F1..F12
        0x90 => 83,                             // Num Lock
        0x91 => 71,                             // Scroll Lock
        0xA0 => 225,                            // Left Shift
        0xA1 => 229,                            // Right Shift
        0xA2 => 224,                            // Left Ctrl
        0xA3 => 228,                            // Right Ctrl
        0xA4 => 226,                            // Left Alt
        0xA5 => 230,                            // Right Alt
        0xBA => 51,                             // ;
        0xBB => 46,                             // =
        0xBC => 54,                             // ,
        0xBD => 45,                             // -
        0xBE => 55,                             // .
        0xBF => 56,                             // /
        0xC0 => 53,                             // `
        0xDB => 47,                             // [
        0xDC => 49,                             // backslash
        0xDD => 48,                             // ]
        0xDE => 52,                             // '
        _ => return None,
    })
}

/// Per-player keyboard mapping and edge state derived from `Settings.ini`.
///
/// Edge detection in [`InputMappings::apply`] is stateful: it remembers the last [`InputState`]
/// produced per slot and reports buttons that went down since then. Use [`Self::reset_edges`]
/// after regaining focus so held keys are not reported as freshly pressed.
#[derive(Clone, Debug)]
pub struct InputMappings {
    slots: [SlotMapping; PLAYER_COUNT],
    previous: [Cell<ButtonState>; PLAYER_COUNT],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SlotMapping {
    keys: [Option<u32>; Button::COUNT],
}

impl SlotMapping {
    fn from_keyboard_map(map: &KeyboardMap, convert: impl Fn(Option<i32>) -> Option<u32>) -> Self {
        let mut keys = [None; Button::COUNT];
        let mut set = |button: Button, value: Option<i32>| {
            if let Some(index) = keys.get_mut(button.index()) {
                *index = convert(value);
            }
        };
        set(Button::Up, map.up);
        set(Button::Down, map.down);
        set(Button::Left, map.left);
        set(Button::Right, map.right);
        set(Button::A, map.button_a);
        set(Button::B, map.button_b);
        set(Button::C, map.button_c);
        set(Button::X, map.button_x);
        set(Button::Y, map.button_y);
        set(Button::Z, map.button_z);
        set(Button::Start, map.start);
        set(Button::Select, map.select);
        Self { keys }
    }
}

impl Default for InputMappings {
    fn default() -> Self {
        Self::empty()
    }
}

impl InputMappings {
    /// An empty mapping with every key unbound.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            slots: [SlotMapping::default(); PLAYER_COUNT],
            previous: [const { Cell::new(ButtonState::NONE) }; PLAYER_COUNT],
        }
    }

    /// Builds mappings from `Settings.ini` virtual-key codes (the Origins asset convention).
    ///
    /// Missing `[Keyboard Map N]` sections and unbound keys (`0`) produce no mapping, as does any
    /// value [`vk_to_scancode`] does not know.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self::build(settings, |value| value.and_then(vk_to_scancode))
    }

    /// Builds mappings from settings values that are already SDL scancode numbers, as written by
    /// `RSDKv4-Decompilation`'s `WriteSettings`.
    ///
    /// Values outside `0..512` and `0` (unbound) produce no mapping.
    #[must_use]
    pub fn from_settings_scancodes(settings: &Settings) -> Self {
        Self::build(settings, |value| {
            let value = u32::try_from(value?).ok()?;
            if value == 0 || value as usize >= KEY_COUNT {
                None
            } else {
                Some(value)
            }
        })
    }

    fn build(settings: &Settings, convert: impl Fn(Option<i32>) -> Option<u32>) -> Self {
        let fallback = KeyboardMap::default();
        let slots = std::array::from_fn(|index| {
            let map = settings.keyboard_maps.get(index).unwrap_or(&fallback);
            SlotMapping::from_keyboard_map(map, &convert)
        });
        Self {
            slots,
            previous: [const { Cell::new(ButtonState::NONE) }; PLAYER_COUNT],
        }
    }

    /// The SDL scancode bound to `button` for `slot`, if any.
    #[must_use]
    pub fn scancode_for(&self, slot: u8, button: Button) -> Option<u32> {
        self.slots
            .get(usize::from(slot))?
            .keys
            .get(button.index())
            .copied()
            .flatten()
    }

    /// Forgets the previous held state used for edge detection.
    pub fn reset_edges(&self) {
        for previous in &self.previous {
            previous.set(ButtonState::NONE);
        }
    }

    /// Converts raw keyboard and gamepad state into one frame of [`InputState`].
    ///
    /// `keys` is indexed by SDL scancode number; out-of-range indices read as released.
    /// [`GamepadState::held`] is OR-ed in, as are stick directions derived via
    /// [`directions_from_axes`]. `pressed` is the difference from the previous call for `slot`,
    /// so call once per frame and in slot order. Slots outside `0..4` return a neutral state.
    #[must_use]
    pub fn apply(&self, slot: u8, keys: &[bool], gamepad: &GamepadState) -> InputState {
        let index = usize::from(slot);
        let Some(mapping) = self.slots.get(index) else {
            return InputState::new(slot);
        };
        let mut held = if gamepad.connected {
            gamepad
                .held
                .union(directions_from_axes(gamepad.axis_x, gamepad.axis_y))
        } else {
            ButtonState::NONE
        };
        for button in Button::ALL {
            let scancode = mapping.keys.get(button.index()).copied().flatten();
            if let Some(scancode) = scancode
                && keys.get(scancode as usize).copied().unwrap_or(false)
            {
                held.insert(button.flag());
            }
        }
        let Some(previous) = self.previous.get(index) else {
            return InputState::new(slot);
        };
        let pressed = held.difference(previous.get());
        previous.set(held);

        let (axis_x, axis_y) = if gamepad.connected {
            (gamepad.axis_x, gamepad.axis_y)
        } else {
            (0, 0)
        };
        InputState {
            slot,
            connected: gamepad.connected || !keys.is_empty(),
            held,
            pressed,
            axis_x,
            axis_y,
            touch_count: 0,
            touches: [crate::TouchPoint {
                down: false,
                x: 0,
                y: 0,
            }; crate::MAX_TOUCHES],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_format_v4::Settings;
    use std::str::FromStr;

    const S1_MAP: &str = "\
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

    fn s1_mappings() -> InputMappings {
        InputMappings::from_settings(&Settings::from_str(S1_MAP).unwrap())
    }

    #[test]
    fn maps_origins_virtual_keys_to_sdl_scancodes() {
        let mappings = s1_mappings();
        assert_eq!(mappings.scancode_for(0, Button::Up), Some(82));
        assert_eq!(mappings.scancode_for(0, Button::Down), Some(81));
        assert_eq!(mappings.scancode_for(0, Button::Left), Some(80));
        assert_eq!(mappings.scancode_for(0, Button::Right), Some(79));
        assert_eq!(mappings.scancode_for(0, Button::A), Some(4));
        assert_eq!(mappings.scancode_for(0, Button::B), Some(22));
        assert_eq!(mappings.scancode_for(0, Button::C), Some(7));
        assert_eq!(mappings.scancode_for(0, Button::X), Some(20));
        assert_eq!(mappings.scancode_for(0, Button::Y), Some(26));
        assert_eq!(mappings.scancode_for(0, Button::Z), Some(8));
        assert_eq!(mappings.scancode_for(0, Button::Start), Some(40));
        assert_eq!(mappings.scancode_for(0, Button::Select), Some(43));
        assert_eq!(mappings.scancode_for(0, Button::L), None);
        assert_eq!(mappings.scancode_for(0, Button::R), None);
        assert_eq!(mappings.scancode_for(1, Button::A), None);
        assert_eq!(mappings.scancode_for(9, Button::A), None);
    }

    #[test]
    fn vk_table_covers_common_codes() {
        assert_eq!(vk_to_scancode(0x26), Some(82));
        assert_eq!(vk_to_scancode(0x41), Some(4));
        assert_eq!(vk_to_scancode(0x5A), Some(29));
        assert_eq!(vk_to_scancode(0x30), Some(39));
        assert_eq!(vk_to_scancode(0x39), Some(38));
        assert_eq!(vk_to_scancode(0x70), Some(58));
        assert_eq!(vk_to_scancode(0x7B), Some(69));
        assert_eq!(vk_to_scancode(0x60), Some(98));
        assert_eq!(vk_to_scancode(0x61), Some(89));
        assert_eq!(vk_to_scancode(0x69), Some(97));
        assert_eq!(vk_to_scancode(0xDB), Some(47));
        assert_eq!(vk_to_scancode(0), None);
        assert_eq!(vk_to_scancode(-1), None);
        assert_eq!(vk_to_scancode(0xFF), None);
    }

    #[test]
    fn apply_derives_held_and_pressed_edges() {
        let mappings = s1_mappings();
        let mut keys = vec![false; KEY_COUNT];
        keys[4] = true; // A
        keys[82] = true; // Up
        let pad = GamepadState::default();

        let first = mappings.apply(0, &keys, &pad);
        assert!(first.connected);
        assert_eq!(first.held, ButtonState::A | ButtonState::UP);
        assert_eq!(first.pressed, ButtonState::A | ButtonState::UP);

        let second = mappings.apply(0, &keys, &pad);
        assert_eq!(second.held, ButtonState::A | ButtonState::UP);
        assert!(second.pressed.is_empty());

        keys[4] = false;
        let third = mappings.apply(0, &keys, &pad);
        assert_eq!(third.held, ButtonState::UP);
        assert!(third.pressed.is_empty());

        mappings.reset_edges();
        let fourth = mappings.apply(0, &keys, &pad);
        assert_eq!(fourth.pressed, ButtonState::UP);
    }

    #[test]
    fn apply_merges_gamepad_and_stick() {
        let mappings = InputMappings::empty();
        let pad = GamepadState {
            connected: true,
            held: ButtonState::A | ButtonState::START,
            axis_x: 20_000,
            axis_y: -20_000,
        };
        let state = mappings.apply(0, &[], &pad);
        assert!(state.held.contains(ButtonState::A));
        assert!(state.held.contains(ButtonState::START));
        assert!(state.held.contains(ButtonState::RIGHT));
        assert!(state.held.contains(ButtonState::UP));
        assert_eq!(state.axis_x, 20_000);
        assert_eq!(state.axis_y, -20_000);
        assert!(state.pressed.contains(ButtonState::RIGHT));

        mappings.reset_edges();
        let disconnected = GamepadState::default().with_axes(20_000, 0);
        let idle = mappings.apply(0, &[], &disconnected);
        assert!(idle.held.is_empty());
        assert_eq!(idle.axis_x, 0);
    }

    #[test]
    fn apply_out_of_range_slot_is_neutral() {
        let mappings = s1_mappings();
        let state = mappings.apply(7, &[true; 32], &GamepadState::default());
        assert_eq!(state.slot, 7);
        assert!(state.held.is_empty());
        assert!(state.pressed.is_empty());
        assert_eq!(mappings.scancode_for(7, Button::A), None);
    }

    #[test]
    fn scancode_settings_are_accepted_verbatim() {
        let settings = Settings::from_str("[Keyboard 1]\nUp=82\nA=4\nSelect=0\nDown=-3\n").unwrap();
        let mappings = InputMappings::from_settings_scancodes(&settings);
        assert_eq!(mappings.scancode_for(0, Button::Up), Some(82));
        assert_eq!(mappings.scancode_for(0, Button::A), Some(4));
        assert_eq!(mappings.scancode_for(0, Button::Select), None);
        assert_eq!(mappings.scancode_for(0, Button::Down), None);

        // The same values interpreted as virtual keys map differently: 82 is VK_R -> scancode R,
        // while 4 and -3 are not virtual-key codes at all.
        let vk = InputMappings::from_settings(&settings);
        assert_eq!(vk.scancode_for(0, Button::Up), Some(21));
        assert_eq!(vk.scancode_for(0, Button::A), None);
        assert_eq!(vk.scancode_for(0, Button::Down), None);
    }
}
