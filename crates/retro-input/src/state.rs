//! Per-player input state produced once per engine frame.

use crate::ButtonState;

/// Maximum number of simultaneous touch points, matching upstream `touchDown[8]`.
pub const MAX_TOUCHES: usize = 8;

/// Left-stick deadzone in raw SDL axis units, equivalent to the upstream `LSTICK_DEADZONE` of 0.3.
pub const STICK_DEADZONE: i16 = 9830;

/// One active touch point.
///
/// Coordinates are screen-space for scripted input and normalized-to-`i16` for the SDL backend;
/// each source documents its own convention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TouchPoint {
    /// Whether the finger is currently down.
    pub down: bool,
    /// Horizontal position.
    pub x: i16,
    /// Vertical position.
    pub y: i16,
}

/// Raw gamepad state for one player slot, free of any backend types.
///
/// Axes follow the SDL convention: `-32768` is up/left and `32767` is down/right.
/// [`GamepadState::held`] only contains physical buttons; stick directions are derived by
/// [`crate::InputMappings::apply`] via [`directions_from_axes`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadState {
    /// Whether a gamepad is present for this slot.
    pub connected: bool,
    /// Buttons held this frame.
    pub held: ButtonState,
    /// Left stick horizontal axis.
    pub axis_x: i16,
    /// Left stick vertical axis (positive down).
    pub axis_y: i16,
}

impl GamepadState {
    /// An empty, disconnected gamepad state.
    #[must_use]
    pub const fn disconnected() -> Self {
        Self {
            connected: false,
            held: ButtonState::NONE,
            axis_x: 0,
            axis_y: 0,
        }
    }

    /// Returns a copy with the left stick axes replaced.
    #[must_use]
    pub const fn with_axes(mut self, axis_x: i16, axis_y: i16) -> Self {
        self.axis_x = axis_x;
        self.axis_y = axis_y;
        self
    }

    /// Direction buttons implied by the left stick, applying [`STICK_DEADZONE`].
    #[must_use]
    pub const fn directions(&self) -> ButtonState {
        directions_from_axes(self.axis_x, self.axis_y)
    }
}

/// Derives D-pad-equivalent bits from a left stick position, applying [`STICK_DEADZONE`].
#[must_use]
pub const fn directions_from_axes(axis_x: i16, axis_y: i16) -> ButtonState {
    let mut state = ButtonState::NONE;
    if axis_y <= -STICK_DEADZONE {
        state = state.union(ButtonState::UP);
    }
    if axis_y >= STICK_DEADZONE {
        state = state.union(ButtonState::DOWN);
    }
    if axis_x <= -STICK_DEADZONE {
        state = state.union(ButtonState::LEFT);
    }
    if axis_x >= STICK_DEADZONE {
        state = state.union(ButtonState::RIGHT);
    }
    state
}

/// Clamps an arbitrary axis reading into the `i16` range used by [`InputState`].
#[must_use]
pub const fn clamp_axis(value: i32) -> i16 {
    if value < i16::MIN as i32 {
        i16::MIN
    } else if value > i16::MAX as i32 {
        i16::MAX
    } else {
        value as i16
    }
}

/// Complete input for one player in one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputState {
    /// Player slot, `0..crate::PLAYER_COUNT`.
    pub slot: u8,
    /// Whether a keyboard or gamepad is available for this slot.
    pub connected: bool,
    /// Buttons held down this frame.
    pub held: ButtonState,
    /// Buttons that changed from up to down this frame (upstream `keyPress`).
    pub pressed: ButtonState,
    /// Horizontal analog axis, `-32768..=32767`, positive right.
    pub axis_x: i16,
    /// Vertical analog axis, `-32768..=32767`, positive down.
    pub axis_y: i16,
    /// Number of active points in [`InputState::touches`].
    pub touch_count: u8,
    /// Active touch points.
    pub touches: [TouchPoint; MAX_TOUCHES],
}

impl InputState {
    /// A neutral state for `slot` with no buttons, axes or touches.
    #[must_use]
    pub const fn new(slot: u8) -> Self {
        Self {
            slot,
            connected: false,
            held: ButtonState::NONE,
            pressed: ButtonState::NONE,
            axis_x: 0,
            axis_y: 0,
            touch_count: 0,
            touches: [TouchPoint {
                down: false,
                x: 0,
                y: 0,
            }; MAX_TOUCHES],
        }
    }

    /// Whether `button` is held this frame.
    #[must_use]
    pub const fn is_held(&self, button: crate::Button) -> bool {
        self.held.contains(button.flag())
    }

    /// Whether `button` went down this frame.
    #[must_use]
    pub const fn is_pressed(&self, button: crate::Button) -> bool {
        self.pressed.contains(button.flag())
    }

    /// Returns the `index`th active touch point, if any.
    #[must_use]
    pub fn touch(&self, index: usize) -> Option<TouchPoint> {
        if index < usize::from(self.touch_count) && index < MAX_TOUCHES {
            self.touches.get(index).copied()
        } else {
            None
        }
    }

    /// Replaces the touch points, keeping at most [`MAX_TOUCHES`] and marking them down.
    pub fn set_touches(&mut self, points: &[(i16, i16)]) {
        let mut touches = [TouchPoint {
            down: false,
            x: 0,
            y: 0,
        }; MAX_TOUCHES];
        let count = points.len().min(MAX_TOUCHES);
        for (index, (x, y)) in points.iter().take(MAX_TOUCHES).enumerate() {
            touches[index] = TouchPoint {
                down: true,
                x: *x,
                y: *y,
            };
        }
        self.touches = touches;
        self.touch_count = count as u8;
    }

    /// Returns a copy with the given touch points attached.
    #[must_use]
    pub fn with_touches(mut self, points: &[(i16, i16)]) -> Self {
        self.set_touches(points);
        self
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Button;

    #[test]
    fn stick_directions_respect_deadzone() {
        assert_eq!(directions_from_axes(0, 0), ButtonState::NONE);
        assert_eq!(
            directions_from_axes(0, -(STICK_DEADZONE + 1)),
            ButtonState::UP
        );
        assert_eq!(
            directions_from_axes(0, STICK_DEADZONE + 1),
            ButtonState::DOWN
        );
        assert_eq!(
            directions_from_axes(-(STICK_DEADZONE + 1), 0),
            ButtonState::LEFT
        );
        assert_eq!(
            directions_from_axes(STICK_DEADZONE + 1, 0),
            ButtonState::RIGHT
        );
        assert_eq!(
            directions_from_axes(i16::MIN, i16::MAX),
            ButtonState::DOWN | ButtonState::LEFT
        );
        assert_eq!(directions_from_axes(1, -1), ButtonState::NONE);
    }

    #[test]
    fn axis_clamping_saturates() {
        assert_eq!(clamp_axis(0), 0);
        assert_eq!(clamp_axis(100_000), i16::MAX);
        assert_eq!(clamp_axis(-100_000), i16::MIN);
        assert_eq!(clamp_axis(i32::MAX), i16::MAX);
    }

    #[test]
    fn touch_points_are_bounded() {
        let mut state = InputState::new(1);
        state.set_touches(&[(1, 2), (3, 4)]);
        assert_eq!(state.touch_count, 2);
        assert_eq!(
            state.touch(0),
            Some(TouchPoint {
                down: true,
                x: 1,
                y: 2
            })
        );
        assert_eq!(state.touch(2), None);
    }

    #[test]
    fn buttons_are_queryable() {
        let mut state = InputState::new(0);
        state.held = ButtonState::A | ButtonState::LEFT;
        state.pressed = ButtonState::A;
        assert!(state.is_held(Button::A));
        assert!(state.is_held(Button::Left));
        assert!(!state.is_held(Button::Right));
        assert!(state.is_pressed(Button::A));
        assert!(!state.is_pressed(Button::Left));
    }
}
