//! Engine button flags shared by keyboard, gamepad and scripted input.
//!
//! The bit layout mirrors upstream `InputButtons` in `Input.hpp`; [`ButtonState`] is a hand-rolled
//! bit set (no external crate) so it can be used in `const` contexts and copied freely.

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

/// A named engine button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    /// D-pad up / left stick up.
    Up,
    /// D-pad down / left stick down.
    Down,
    /// D-pad left / left stick left.
    Left,
    /// D-pad right / left stick right.
    Right,
    /// Primary action (keyboard `buttonA`, gamepad south face).
    A,
    /// Secondary action (keyboard `buttonB`, gamepad east face).
    B,
    /// Tertiary action (keyboard `buttonC`, gamepad north face).
    C,
    /// `buttonX` (gamepad west face).
    X,
    /// `buttonY` (gamepad left trigger by default).
    Y,
    /// `buttonZ` (gamepad right trigger by default).
    Z,
    /// Left shoulder button (`buttonL`).
    L,
    /// Right shoulder button (`buttonR`).
    R,
    /// Start.
    Start,
    /// Select (gamepad guide/back by default).
    Select,
}

impl Button {
    /// Total number of engine buttons.
    pub const COUNT: usize = 14;

    /// Every button in declaration order; the array index matches [`Button::index`].
    pub const ALL: [Self; Self::COUNT] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::A,
        Self::B,
        Self::C,
        Self::X,
        Self::Y,
        Self::Z,
        Self::L,
        Self::R,
        Self::Start,
        Self::Select,
    ];

    /// Bit index of this button in a [`ButtonState`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The [`ButtonState`] containing only this button.
    #[must_use]
    pub const fn flag(self) -> ButtonState {
        ButtonState(1u16 << self as u16)
    }

    /// Canonical upper-case name as used by the scripted input format.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::L => "L",
            Self::R => "R",
            Self::Start => "START",
            Self::Select => "SELECT",
        }
    }

    /// Case-insensitive lookup of a canonical name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|button| name.eq_ignore_ascii_case(button.name()))
    }
}

impl From<Button> for ButtonState {
    fn from(button: Button) -> Self {
        button.flag()
    }
}

/// A set of engine buttons.
///
/// Bits map one-to-one onto [`Button`] in declaration order. Unknown bits are ignored by
/// [`ButtonState::from_bits_truncate`] but retained by the bit operations, so callers should keep
/// values inside [`ButtonState::ALL`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ButtonState(u16);

impl ButtonState {
    /// No buttons.
    pub const NONE: Self = Self(0);
    /// D-pad up.
    pub const UP: Self = Self(1 << 0);
    /// D-pad down.
    pub const DOWN: Self = Self(1 << 1);
    /// D-pad left.
    pub const LEFT: Self = Self(1 << 2);
    /// D-pad right.
    pub const RIGHT: Self = Self(1 << 3);
    /// Primary action.
    pub const A: Self = Self(1 << 4);
    /// Secondary action.
    pub const B: Self = Self(1 << 5);
    /// Tertiary action.
    pub const C: Self = Self(1 << 6);
    /// Fourth face button.
    pub const X: Self = Self(1 << 7);
    /// Fifth face button.
    pub const Y: Self = Self(1 << 8);
    /// Sixth face button.
    pub const Z: Self = Self(1 << 9);
    /// Left shoulder.
    pub const L: Self = Self(1 << 10);
    /// Right shoulder.
    pub const R: Self = Self(1 << 11);
    /// Start.
    pub const START: Self = Self(1 << 12);
    /// Select.
    pub const SELECT: Self = Self(1 << 13);
    /// The four directional buttons.
    pub const DIRECTIONS: Self = Self(Self::UP.0 | Self::DOWN.0 | Self::LEFT.0 | Self::RIGHT.0);
    /// Every button bit.
    pub const ALL: Self = Self((1 << Button::COUNT) - 1);

    /// Raw bit pattern.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Builds a state from a raw bit pattern, dropping bits with no matching [`Button`].
    #[must_use]
    pub const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// Whether no buttons are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every button in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any button in `other` is set.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// The union of both states.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The buttons present in both states.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The buttons present in `self` but not in `other`.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Adds all buttons in `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Removes all buttons in `other`.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Toggles all buttons in `other`.
    pub const fn toggle(&mut self, other: Self) {
        self.0 ^= other.0;
    }

    /// Iterates over the set buttons.
    pub fn iter(self) -> impl Iterator<Item = Button> {
        Button::ALL
            .into_iter()
            .filter(move |button| self.contains(button.flag()))
    }
}

impl fmt::Debug for ButtonState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for button in Button::ALL {
            if self.contains(button.flag()) {
                list.entry(&button.name());
            }
        }
        list.finish()
    }
}

impl BitOr for ButtonState {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitOrAssign for ButtonState {
    fn bitor_assign(&mut self, other: Self) {
        self.insert(other);
    }
}

impl BitAnd for ButtonState {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        self.intersection(other)
    }
}

impl BitAndAssign for ButtonState {
    fn bitand_assign(&mut self, other: Self) {
        self.0 &= other.0;
    }
}

impl BitXor for ButtonState {
    type Output = Self;

    fn bitxor(self, other: Self) -> Self {
        Self(self.0 ^ other.0)
    }
}

impl BitXorAssign for ButtonState {
    fn bitxor_assign(&mut self, other: Self) {
        self.toggle(other);
    }
}

impl Not for ButtonState {
    type Output = Self;

    /// Complements the state inside [`ButtonState::ALL`].
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_match_declaration_order() {
        for (index, button) in Button::ALL.into_iter().enumerate() {
            assert_eq!(button.index(), index);
            assert_eq!(button.flag().bits(), 1u16 << index);
            assert_eq!(Button::from_name(button.name()), Some(button));
        }
        assert_eq!(ButtonState::ALL.bits(), (1 << Button::COUNT) - 1);
    }

    #[test]
    fn names_are_case_insensitive() {
        assert_eq!(Button::from_name("buttona"), None);
        assert_eq!(Button::from_name("start"), Some(Button::Start));
        assert_eq!(Button::from_name("Up"), Some(Button::Up));
        assert_eq!(Button::from_name("nope"), None);
    }

    #[test]
    fn set_operations() {
        let mut state = ButtonState::A | ButtonState::UP;
        assert!(state.contains(ButtonState::A));
        assert!(state.contains(ButtonState::A | ButtonState::UP));
        assert!(!state.contains(ButtonState::A | ButtonState::B));
        assert!(state.intersects(ButtonState::UP));
        assert!(!state.intersects(ButtonState::B));
        assert_eq!(state.difference(ButtonState::A), ButtonState::UP);
        assert_eq!((state & ButtonState::A), ButtonState::A);
        state.toggle(ButtonState::A);
        assert_eq!(state, ButtonState::UP);
        assert_eq!(!state, ButtonState::ALL.difference(ButtonState::UP));
        assert_eq!(ButtonState::from_bits_truncate(0xFFFF), ButtonState::ALL);
        assert_eq!(ButtonState::from_bits_truncate(0x2), ButtonState::DOWN);
        assert!(ButtonState::NONE.is_empty());
        assert_eq!(ButtonState::A.iter().collect::<Vec<_>>(), vec![Button::A]);
    }
}
