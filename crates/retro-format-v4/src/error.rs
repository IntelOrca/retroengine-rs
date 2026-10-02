//! Errors returned by the RSDKv4 format parsers.

use retro_io::IoError;

/// Errors produced while reading an RSDKv4 data file.
///
/// Parsers never panic on malformed input; every failure is reported as one of these variants.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The input ended before a complete record could be read.
    #[error("unexpected end of input")]
    Truncated,
    /// The bytes are structurally readable but violate the format's constraints.
    #[error("invalid data: {0}")]
    Invalid(String),
    /// The data is well formed but uses a variant this parser does not support.
    #[error("unsupported data: {0}")]
    Unsupported(String),
    /// The underlying [`retro_io::DataSource`] failed.
    #[error(transparent)]
    Io(#[from] IoError),
}

impl FormatError {
    /// Convenience constructor for [`FormatError::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Convenience constructor for [`FormatError::Unsupported`].
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }
}
