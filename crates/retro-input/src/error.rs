//! Errors produced while parsing scripted input files.

use std::fmt;

/// A malformed scripted-input file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputError {
    /// The file does not start with `retro-input <version>`.
    MissingHeader {
        /// One-based line number, or `0` for an empty file.
        line: usize,
    },
    /// The header names a format version this crate does not understand.
    UnsupportedVersion {
        /// One-based line number.
        line: usize,
        /// The version token that was found.
        version: String,
    },
    /// The optional `seed` line has no value or a non-numeric value.
    InvalidSeed {
        /// One-based line number.
        line: usize,
        /// The value token that was found.
        value: String,
    },
    /// More than one `seed` line was supplied.
    DuplicateSeed {
        /// One-based line number.
        line: usize,
    },
    /// A `seed` line appeared after the first frame.
    SeedAfterFrames {
        /// One-based line number.
        line: usize,
    },
    /// Frame lines must be numbered `0, 1, 2, ...` in order.
    UnexpectedFrame {
        /// One-based line number.
        line: usize,
        /// The frame index expected on this line.
        expected: usize,
        /// The frame token that was found.
        found: String,
    },
    /// A frame line does not contain exactly `1 + 4 * 4` whitespace-separated fields.
    WrongFieldCount {
        /// One-based line number.
        line: usize,
        /// Number of fields expected.
        expected: usize,
        /// Number of fields found.
        found: usize,
    },
    /// A button name in the `|`-separated list is not a known [`crate::Button`].
    UnknownButton {
        /// One-based line number.
        line: usize,
        /// The offending name.
        name: String,
    },
    /// An `L`/`R` button was requested. The rev03 legacy `ControllerState` has no L/R fields,
    /// so neither the reference harness nor the v4 host can inject them.
    UnsupportedButton {
        /// One-based line number.
        line: usize,
        /// The offending name or mask.
        name: String,
    },
    /// The button field is neither a name list, `-`, nor a numeric mask.
    InvalidButtons {
        /// One-based line number.
        line: usize,
        /// The offending token.
        value: String,
    },
    /// The axis field is not a decimal integer.
    InvalidAxis {
        /// One-based line number.
        line: usize,
        /// The offending token.
        value: String,
    },
    /// The touch field is not `-`, `0`, or a `;`-separated list of `x:y` pairs.
    InvalidTouch {
        /// One-based line number.
        line: usize,
        /// The offending token.
        value: String,
    },
    /// More than [`crate::MAX_TOUCHES`] touch points were listed.
    TooManyTouches {
        /// One-based line number.
        line: usize,
        /// Number of touch points listed.
        count: usize,
    },
    /// The input was not valid UTF-8.
    InvalidUtf8 {
        /// The underlying decoding error.
        message: String,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHeader { line } => {
                write!(f, "line {line}: expected `retro-input 1` header")
            }
            Self::UnsupportedVersion { line, version } => {
                write!(f, "line {line}: unsupported retro-input version {version}")
            }
            Self::InvalidSeed { line, value } => {
                write!(f, "line {line}: invalid seed value `{value}`")
            }
            Self::DuplicateSeed { line } => write!(f, "line {line}: duplicate seed line"),
            Self::SeedAfterFrames { line } => {
                write!(f, "line {line}: seed must appear before the first frame")
            }
            Self::UnexpectedFrame {
                line,
                expected,
                found,
            } => write!(f, "line {line}: expected frame {expected}, found `{found}`"),
            Self::WrongFieldCount {
                line,
                expected,
                found,
            } => write!(
                f,
                "line {line}: expected {expected} fields per frame, found {found}"
            ),
            Self::UnknownButton { line, name } => {
                write!(f, "line {line}: unknown button `{name}`")
            }
            Self::UnsupportedButton { line, name } => {
                write!(
                    f,
                    "line {line}: button `{name}` cannot be replayed (rev03 controllers have no L/R fields)"
                )
            }
            Self::InvalidButtons { line, value } => {
                write!(f, "line {line}: invalid button field `{value}`")
            }
            Self::InvalidAxis { line, value } => {
                write!(f, "line {line}: invalid axis value `{value}`")
            }
            Self::InvalidTouch { line, value } => {
                write!(f, "line {line}: invalid touch field `{value}`")
            }
            Self::TooManyTouches { line, count } => write!(
                f,
                "line {line}: {count} touch points exceed the maximum of {}",
                crate::MAX_TOUCHES
            ),
            Self::InvalidUtf8 { message } => {
                write!(f, "scripted input is not valid UTF-8: {message}")
            }
        }
    }
}

impl std::error::Error for InputError {}

impl From<std::str::Utf8Error> for InputError {
    fn from(error: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8 {
            message: error.to_string(),
        }
    }
}
