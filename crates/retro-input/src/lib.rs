//! Engine-level input mapping and processing of versioned platform input state.
//!
//! This crate is the v4 input model used by scripts and the engine host:
//!
//! * [`ButtonState`] / [`Button`] describe the 14 upstream `INPUT_*` buttons,
//! * [`InputState`] is one player's state for one frame: held and pressed edges, analog axes and
//!   touch points (`keyDown` / `keyPress` in the decompilation),
//! * [`InputMappings`] builds per-slot keyboard maps from `Settings.ini` and converts raw
//!   keyboard and gamepad state into [`InputState`],
//! * [`InputSource`] produces `[InputState; 4]` per poll, with [`NullInput`] and
//!   [`ScriptedInput`] providing deterministic sources for headless tests and replays.
//!
//! The crate performs no I/O of its own (bytes in, tests may read files), depends on no backend
//! and contains no `unsafe`.

#![forbid(unsafe_code)]

mod button;
mod error;
mod mappings;
mod scripted;
mod state;

pub use button::{Button, ButtonState};
pub use error::InputError;
pub use mappings::{InputMappings, vk_to_scancode};
pub use scripted::ScriptedInput;
pub use state::{
    GamepadState, InputState, MAX_TOUCHES, STICK_DEADZONE, TouchPoint, clamp_axis,
    directions_from_axes,
};

/// Player slots supported by the input model.
pub const PLAYER_COUNT: usize = 4;

/// Number of entries in the SDL scancode space used to index raw keyboard arrays.
///
/// This is `SDL_SCANCODE_COUNT`, kept as a plain integer so no SDL types leak into this crate.
pub const KEY_COUNT: usize = 512;

/// A pollable source of input states for all four player slots.
pub trait InputSource {
    /// Returns one state per player for the current frame.
    fn poll(&mut self) -> [InputState; PLAYER_COUNT];

    /// Returns the state for absolute engine tick `tick`.
    ///
    /// Sequential sources ignore the tick and behave like [`InputSource::poll`]. A scripted
    /// replay overrides this to index its file lines by tick, matching the C++ reference
    /// harness: line `N` belongs to record `N` (`InjectInput` indexes `inputMasks[frame]`).
    fn poll_at(&mut self, _tick: u64) -> [InputState; PLAYER_COUNT] {
        self.poll()
    }
}

/// Four neutral states in slot order.
#[must_use]
pub const fn idle_states() -> [InputState; PLAYER_COUNT] {
    [
        InputState::new(0),
        InputState::new(1),
        InputState::new(2),
        InputState::new(3),
    ]
}

/// A source that never reports input and never changes between polls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NullInput;

impl NullInput {
    /// Creates an idle input source.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl InputSource for NullInput {
    fn poll(&mut self) -> [InputState; PLAYER_COUNT] {
        idle_states()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_input_is_deterministic() {
        let mut input = NullInput::new();
        let first = input.poll();
        let second = input.poll();
        assert_eq!(first, second);
        assert_eq!(first, idle_states());
        for (slot, state) in first.iter().enumerate() {
            assert_eq!(state.slot, slot as u8);
            assert!(!state.connected);
            assert!(state.held.is_empty());
            assert!(state.pressed.is_empty());
            assert_eq!(state.touch_count, 0);
        }
    }
}
