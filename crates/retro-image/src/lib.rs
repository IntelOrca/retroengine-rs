//! Decoding and conversion of RSDK image containers into engine pixel formats.
//!
//! The software renderer consumes palette indices directly, so the GIF decoder mirrors the
//! byte-stream parsing and LZW state machine of `ReadGifPictureData`/`ReadGifLine` in
//! RSDKv4-Decompilation (`RSDKv4/Sprite.cpp`) rather than a spec-compliant decoder.

#![forbid(unsafe_code)]

mod gif;

pub use gif::{GifImage, decode_gif};

/// Errors produced while decoding image containers.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// The container is malformed or uses unsupported features.
    #[error("invalid image: {0}")]
    Invalid(String),
    /// The container is structurally valid but uses an unsupported feature.
    #[error("unsupported image: {0}")]
    Unsupported(String),
    /// The input ended before a complete image could be decoded.
    #[error("image data is truncated")]
    Truncated,
}
