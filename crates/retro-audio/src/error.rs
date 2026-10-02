//! Errors produced while decoding or mixing audio.

/// Errors produced while loading, decoding or mixing audio.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// The data is structurally valid but uses an unsupported feature or format.
    #[error("unsupported audio: {0}")]
    Unsupported(String),
    /// The data is malformed.
    #[error("invalid audio: {0}")]
    Invalid(String),
    /// The input ended before a complete asset could be decoded.
    #[error("audio data is truncated")]
    Truncated,
    /// The decoder rejected the bitstream.
    #[error("audio decode failed: {0}")]
    Decode(String),
}
