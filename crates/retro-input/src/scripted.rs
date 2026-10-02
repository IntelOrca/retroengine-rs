//! Deterministic scripted input for headless tests and replay capture.
//!
//! # File format
//!
//! The first non-empty, non-comment line must be the header `retro-input 1`. An optional
//! `seed <u32>` line may follow; it is stored for replay tooling and does not affect the input.
//! Every further line is one engine frame and must be numbered sequentially from `0`:
//!
//! ```text
//! retro-input 1
//! seed 12345
//! 0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -
//! 1 A|RIGHT 100 -30 10:20;30:40  - 0 0 -  - 0 0 -  - 0 0 -
//! ```
//!
//! Each frame carries four players, each with four whitespace-separated fields:
//!
//! 1. buttons: `-` for none, a `|`-separated list of `A,B,C,X,Y,Z,L,R,START,SELECT,UP,DOWN,
//!    LEFT,RIGHT`, or a decimal/`0x` mask of [`ButtonState`] bits,
//! 2. `axis_x`: decimal integer clamped to `i16`,
//! 3. `axis_y`: decimal integer clamped to `i16`,
//! 4. touches: `-` or `0` for none, otherwise `;`-separated `x:y` pairs (at most
//!    [`MAX_TOUCHES`]).
//!
//! `pressed` edges are derived while parsing: a button is pressed on the first frame it is held.
//! Polling past the end repeats the last frame, which keeps replays deterministic; an empty file
//! body polls as four neutral states.

use crate::error::InputError;
use crate::state::clamp_axis;
use crate::{Button, ButtonState, InputSource, InputState, MAX_TOUCHES, PLAYER_COUNT, idle_states};

/// Header token of the scripted input format.
pub const HEADER: &str = "retro-input";

/// Format version understood by this crate.
pub const VERSION: u32 = 1;

/// Input source replaying per-frame states parsed from a text file.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ScriptedInput {
    frames: Vec<[InputState; PLAYER_COUNT]>,
    cursor: usize,
    seed: Option<u32>,
}

impl ScriptedInput {
    /// Parses the scripted input text format.
    ///
    /// [`std::str::FromStr`] is implemented as well and delegates here.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Result<Self, InputError> {
        Self::parse(text)
    }

    fn parse(text: &str) -> Result<Self, InputError> {
        let mut frames: Vec<[InputState; PLAYER_COUNT]> = Vec::new();
        let mut previous = [ButtonState::NONE; PLAYER_COUNT];
        let mut seed = None;
        let mut seen_header = false;

        for (index, raw_line) in text.lines().enumerate() {
            let line = index + 1;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if !seen_header {
                let mut fields = trimmed.split_whitespace();
                if fields.next() != Some(HEADER) {
                    return Err(InputError::MissingHeader { line });
                }
                let version = fields.next().unwrap_or_default();
                if version != VERSION.to_string() {
                    return Err(InputError::UnsupportedVersion {
                        line,
                        version: version.to_owned(),
                    });
                }
                seen_header = true;
                continue;
            }
            if trimmed
                .split_whitespace()
                .next()
                .is_some_and(|token| token.eq_ignore_ascii_case("seed"))
            {
                if !frames.is_empty() {
                    return Err(InputError::SeedAfterFrames { line });
                }
                if seed.is_some() {
                    return Err(InputError::DuplicateSeed { line });
                }
                let value =
                    trimmed
                        .split_whitespace()
                        .nth(1)
                        .ok_or_else(|| InputError::InvalidSeed {
                            line,
                            value: String::new(),
                        })?;
                let parsed = value.parse::<u32>().map_err(|_| InputError::InvalidSeed {
                    line,
                    value: value.to_owned(),
                })?;
                seed = Some(parsed);
                continue;
            }
            let frame = parse_frame(trimmed, line, frames.len(), &mut previous)?;
            frames.push(frame);
        }

        if !seen_header {
            return Err(InputError::MissingHeader { line: 0 });
        }
        Ok(Self {
            frames,
            cursor: 0,
            seed,
        })
    }

    /// Parses scripted input from bytes, rejecting invalid UTF-8.
    pub fn load(bytes: &[u8]) -> Result<Self, InputError> {
        Self::from_str(std::str::from_utf8(bytes)?)
    }

    /// Number of frames in the file.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The optional replay seed from the header.
    #[must_use]
    pub const fn seed(&self) -> Option<u32> {
        self.seed
    }

    /// Number of frames already returned by [`InputSource::poll`], clamped to [`Self::frame_count`].
    #[must_use]
    pub const fn position(&self) -> usize {
        self.cursor
    }

    /// Borrows the states of `frame`, if present.
    #[must_use]
    pub fn frame(&self, frame: usize) -> Option<&[InputState; PLAYER_COUNT]> {
        self.frames.get(frame)
    }

    /// Rewinds the replay to the first frame.
    pub fn rewind(&mut self) {
        self.cursor = 0;
    }
}

impl std::str::FromStr for ScriptedInput {
    type Err = InputError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl InputSource for ScriptedInput {
    fn poll(&mut self) -> [InputState; PLAYER_COUNT] {
        if self.frames.is_empty() {
            return idle_states();
        }
        let last = self.frames.len() - 1;
        let index = self.cursor.min(last);
        self.cursor = self.cursor.saturating_add(1).min(last);
        self.frames.get(index).copied().unwrap_or_else(idle_states)
    }
}

fn parse_frame(
    line_text: &str,
    line: usize,
    expected: usize,
    previous: &mut [ButtonState; PLAYER_COUNT],
) -> Result<[InputState; PLAYER_COUNT], InputError> {
    let fields: Vec<&str> = line_text.split_whitespace().collect();
    let Some((frame_token, players)) = fields.split_first() else {
        return Err(InputError::WrongFieldCount {
            line,
            expected: 1 + PLAYER_COUNT * 4,
            found: 0,
        });
    };
    let found = frame_token
        .parse::<usize>()
        .map_err(|_| InputError::UnexpectedFrame {
            line,
            expected,
            found: (*frame_token).to_owned(),
        })?;
    if found != expected {
        return Err(InputError::UnexpectedFrame {
            line,
            expected,
            found: found.to_string(),
        });
    }
    let (player_fields, remainder) = players.as_chunks::<4>();
    if player_fields.len() != PLAYER_COUNT || !remainder.is_empty() {
        return Err(InputError::WrongFieldCount {
            line,
            expected: 1 + PLAYER_COUNT * 4,
            found: fields.len(),
        });
    }

    let mut states = idle_states();
    for (slot, chunk) in player_fields.iter().enumerate() {
        let [buttons, axis_x, axis_y, touches] = chunk;
        let held = parse_buttons(buttons, line)?;
        let axis_x = parse_axis(axis_x, line)?;
        let axis_y = parse_axis(axis_y, line)?;
        let (touch_count, touch_points) = parse_touches(touches, line)?;
        let pressed = held.difference(previous[slot]);
        previous[slot] = held;
        states[slot] = InputState {
            slot: slot as u8,
            connected: true,
            held,
            pressed,
            axis_x,
            axis_y,
            touch_count,
            touches: touch_points,
        };
    }
    Ok(states)
}

fn parse_buttons(token: &str, line: usize) -> Result<ButtonState, InputError> {
    if token == "-" {
        return Ok(ButtonState::NONE);
    }
    let invalid = || InputError::InvalidButtons {
        line,
        value: token.to_owned(),
    };
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        let bits = u16::from_str_radix(hex, 16).map_err(|_| invalid())?;
        return Ok(ButtonState::from_bits_truncate(bits));
    }
    if token.bytes().all(|byte| byte.is_ascii_digit()) {
        let bits = token.parse::<u16>().map_err(|_| invalid())?;
        return Ok(ButtonState::from_bits_truncate(bits));
    }
    let mut state = ButtonState::NONE;
    for name in token.split('|') {
        let button = Button::from_name(name.trim()).ok_or_else(|| InputError::UnknownButton {
            line,
            name: name.trim().to_owned(),
        })?;
        state.insert(button.flag());
    }
    Ok(state)
}

fn parse_axis(token: &str, line: usize) -> Result<i16, InputError> {
    let value = token.parse::<i32>().map_err(|_| InputError::InvalidAxis {
        line,
        value: token.to_owned(),
    })?;
    Ok(clamp_axis(value))
}

fn parse_touches(
    token: &str,
    line: usize,
) -> Result<(u8, [crate::TouchPoint; MAX_TOUCHES]), InputError> {
    let mut points = [crate::TouchPoint {
        down: false,
        x: 0,
        y: 0,
    }; MAX_TOUCHES];
    if token == "-" || token == "0" {
        return Ok((0, points));
    }
    let mut count = 0usize;
    for pair in token.split(';') {
        let invalid = || InputError::InvalidTouch {
            line,
            value: token.to_owned(),
        };
        let (x, y) = pair.split_once(':').ok_or_else(invalid)?;
        if count >= MAX_TOUCHES {
            return Err(InputError::TooManyTouches {
                line,
                count: count + 1,
            });
        }
        let x = x.trim().parse::<i32>().map_err(|_| invalid())?;
        let y = y.trim().parse::<i32>().map_err(|_| invalid())?;
        points[count] = crate::TouchPoint {
            down: true,
            x: clamp_axis(x),
            y: clamp_axis(y),
        };
        count += 1;
    }
    Ok((count as u8, points))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One idle player's four fields.
    const IDLE_PLAYER: &str = "- 0 0 -";

    /// Formats a frame line for four players, each given as `buttons axis_x axis_y touches`.
    fn frame_line(frame: usize, players: [&str; PLAYER_COUNT]) -> String {
        format!("{frame} {}", players.join(" "))
    }

    /// Formats a frame line with player 1 active and the rest idle.
    fn solo(frame: usize, player0: &str) -> String {
        frame_line(frame, [player0, IDLE_PLAYER, IDLE_PLAYER, IDLE_PLAYER])
    }

    const SAMPLE: &str = "\
# replay of a test run
retro-input 1
seed 7
0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -
1 A|RIGHT 100 -30 10:20;30:40  - 0 0 -  - 0 0 -  - 0 0 -
2 0x0004 100000 -100000 0  - 0 0 -  - 0 0 -  - 0 0 -
";

    #[test]
    fn parses_header_seed_and_frames() {
        let mut input = ScriptedInput::from_str(SAMPLE).unwrap();
        assert_eq!(input.frame_count(), 3);
        assert_eq!(input.seed(), Some(7));

        let frame0 = input.poll();
        assert_eq!(frame0[0].slot, 0);
        assert!(frame0[0].held.is_empty());
        assert!(frame0[0].pressed.is_empty());

        let frame1 = input.poll();
        assert!(frame1[0].held.contains(ButtonState::A | ButtonState::RIGHT));
        assert!(
            frame1[0]
                .pressed
                .contains(ButtonState::A | ButtonState::RIGHT)
        );
        assert_eq!(frame1[0].axis_x, 100);
        assert_eq!(frame1[0].axis_y, -30);
        assert_eq!(frame1[0].touch_count, 2);
        assert_eq!(
            frame1[0].touch(0),
            Some(crate::TouchPoint {
                down: true,
                x: 10,
                y: 20
            })
        );
        assert_eq!(
            frame1[0].touch(1),
            Some(crate::TouchPoint {
                down: true,
                x: 30,
                y: 40
            })
        );
        assert!(frame1[1].connected);
        assert!(frame1[1].held.is_empty());

        let frame2 = input.poll();
        assert!(frame2[0].held.contains(ButtonState::LEFT));
        assert!(!frame2[0].held.contains(ButtonState::A));
        assert!(frame2[0].pressed.contains(ButtonState::LEFT));
        assert_eq!(frame2[0].axis_x, i16::MAX);
        assert_eq!(frame2[0].axis_y, i16::MIN);
        assert_eq!(frame2[0].touch_count, 0);
    }

    #[test]
    fn pressed_edges_track_hold_across_frames() {
        let script = format!(
            "retro-input 1\n{}\n{}\n{}\n{}\n",
            solo(0, "A 0 0 -"),
            solo(1, "A 0 0 -"),
            solo(2, "A|B 0 0 -"),
            solo(3, "B 0 0 -"),
        );
        let mut input = ScriptedInput::from_str(&script).unwrap();
        assert!(input.poll()[0].pressed.contains(ButtonState::A));
        assert!(input.poll()[0].pressed.is_empty());
        let third = input.poll()[0];
        assert!(third.pressed.contains(ButtonState::B));
        assert!(!third.pressed.contains(ButtonState::A));
        let fourth = input.poll()[0];
        assert!(fourth.held.contains(ButtonState::B));
        assert!(!fourth.held.contains(ButtonState::A));
        assert!(fourth.pressed.is_empty());
    }

    #[test]
    fn polling_past_the_end_repeats_the_last_frame() {
        let script = format!(
            "retro-input 1\n{}\n{}\n",
            solo(0, "A 0 0 -"),
            solo(1, "- 1 2 -"),
        );
        let mut input = ScriptedInput::from_str(&script).unwrap();
        let _ = input.poll();
        let second = input.poll();
        assert_eq!(input.poll(), second);
        assert_eq!(input.poll(), second);
        assert_eq!(input.position(), 1);
        input.rewind();
        assert_eq!(input.position(), 0);
        assert!(input.poll()[0].held.contains(ButtonState::A));
    }

    #[test]
    fn identical_scripts_poll_identically() {
        let mut parsed = ScriptedInput::from_str(SAMPLE).unwrap();
        let mut loaded = ScriptedInput::load(SAMPLE.as_bytes()).unwrap();
        for frame in 0..8 {
            assert_eq!(parsed.poll(), loaded.poll(), "frame {frame}");
        }
    }

    #[test]
    fn header_only_stream_polls_idle() {
        let mut input = ScriptedInput::from_str("retro-input 1\n").unwrap();
        assert_eq!(input.frame_count(), 0);
        let states = input.poll();
        for (slot, state) in states.iter().enumerate() {
            assert_eq!(state.slot, slot as u8);
            assert!(state.held.is_empty());
        }
    }

    #[test]
    fn load_rejects_invalid_utf8() {
        assert!(matches!(
            ScriptedInput::load(&[b'r', b'e', b't', b'r', b'o', 0xFF]),
            Err(InputError::InvalidUtf8 { .. })
        ));
        assert!(ScriptedInput::load(SAMPLE.as_bytes()).is_ok());
    }

    #[test]
    fn malformed_files_produce_errors() {
        let cases: Vec<(String, InputError)> = vec![
            (String::new(), InputError::MissingHeader { line: 0 }),
            (
                "retro-input 2\n".to_owned(),
                InputError::UnsupportedVersion {
                    line: 1,
                    version: "2".to_owned(),
                },
            ),
            (
                "retro-input\n".to_owned(),
                InputError::UnsupportedVersion {
                    line: 1,
                    version: String::new(),
                },
            ),
            (
                "retro-input 1\nseed\n".to_owned(),
                InputError::InvalidSeed {
                    line: 2,
                    value: String::new(),
                },
            ),
            (
                "retro-input 1\nseed abc\n".to_owned(),
                InputError::InvalidSeed {
                    line: 2,
                    value: "abc".to_owned(),
                },
            ),
            (
                "retro-input 1\nseed 1\nseed 2\n".to_owned(),
                InputError::DuplicateSeed { line: 3 },
            ),
            (
                format!("retro-input 1\n{}\nseed 1\n", solo(0, IDLE_PLAYER)),
                InputError::SeedAfterFrames { line: 3 },
            ),
            (
                format!("retro-input 1\n{}\n", solo(1, IDLE_PLAYER)),
                InputError::UnexpectedFrame {
                    line: 2,
                    expected: 0,
                    found: "1".to_owned(),
                },
            ),
            (
                "retro-input 1\nnope - 0 0 -\n".to_owned(),
                InputError::UnexpectedFrame {
                    line: 2,
                    expected: 0,
                    found: "nope".to_owned(),
                },
            ),
            (
                "retro-input 1\n0 - 0 0 -\n".to_owned(),
                InputError::WrongFieldCount {
                    line: 2,
                    expected: 17,
                    found: 5,
                },
            ),
            (
                format!("retro-input 1\n{}\n", solo(0, "NOPE 0 0 -")),
                InputError::UnknownButton {
                    line: 2,
                    name: "NOPE".to_owned(),
                },
            ),
            (
                format!("retro-input 1\n{}\n", solo(0, "0xzz 0 0 -")),
                InputError::InvalidButtons {
                    line: 2,
                    value: "0xzz".to_owned(),
                },
            ),
            (
                format!("retro-input 1\n{}\n", solo(0, "A 1e5 0 -")),
                InputError::InvalidAxis {
                    line: 2,
                    value: "1e5".to_owned(),
                },
            ),
            (
                format!("retro-input 1\n{}\n", solo(0, "A 0 0 1:2:3")),
                InputError::InvalidTouch {
                    line: 2,
                    value: "1:2:3".to_owned(),
                },
            ),
            (
                format!(
                    "retro-input 1\n{}\n",
                    solo(0, "A 0 0 1:2;3:4;5:6;7:8;9:10;11:12;13:14;15:16;17:18")
                ),
                InputError::TooManyTouches { line: 2, count: 9 },
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(
                ScriptedInput::from_str(&text),
                Err(expected.clone()),
                "input: {text:?}"
            );
        }
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let script = format!(
            "; a comment\nretro-input 1\n\n# another\n{}\n\n{}\n",
            solo(0, "A 0 0 -"),
            solo(1, "- 0 0 1:2"),
        );
        let mut input = ScriptedInput::from_str(&script).unwrap();
        assert_eq!(input.frame_count(), 2);
        assert!(input.poll()[0].held.contains(ButtonState::A));
        assert_eq!(input.poll()[0].touch_count, 1);
    }
}
