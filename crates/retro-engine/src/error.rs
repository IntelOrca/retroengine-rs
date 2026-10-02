//! Engine error type shared by the loader, runtime and CLI.

use retro_format::DataVersion;
use retro_format_v4::FormatError;
use retro_io::IoError;
use retro_script::{CompileError, ScriptError};
use std::path::PathBuf;

/// Errors raised while resolving, loading or running the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The assets directory does not exist.
    #[error("assets directory does not exist: {0}")]
    MissingAssets(PathBuf),
    /// The assets directory has no `Data/Game/GameConfig.bin`.
    #[error("not an unpacked RSDK asset folder, missing: {0}")]
    MissingGameConfig(PathBuf),
    /// A required file is missing from the data source.
    #[error("missing asset: {0}")]
    MissingAsset(String),
    /// A v4 format parser rejected a file.
    #[error("format error: {0}")]
    Format(#[from] FormatError),
    /// The data source failed.
    #[error("i/o error: {0}")]
    Io(#[from] IoError),
    /// A script source failed to compile.
    #[error("script compile error: {0}")]
    Compile(Box<CompileError>),
    /// The VM rejected a script execution.
    #[error("script error: {0}")]
    Script(#[from] ScriptError),
    /// A platform backend failed.
    #[error("platform error: {0}")]
    Platform(#[from] retro_platform::PlatformError),
    /// A scene name could not be resolved.
    #[error("unknown scene '{0}' (no matching stage folder or GameConfig scene)")]
    UnknownScene(String),
    /// The detected data family is not supported yet.
    #[error("unsupported data version: {0:?}")]
    UnsupportedVersion(DataVersion),
}

impl From<CompileError> for EngineError {
    fn from(error: CompileError) -> Self {
        Self::Compile(Box::new(error))
    }
}
