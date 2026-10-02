//! Parsers and writers for RSDK v4 legacy data formats.
//!
//! The byte layouts mirror the loaders of RSDKv4-Decompilation (`RSDKv4/RetroEngine.cpp`
//! `LoadGameConfig`, `RSDKv4/Scene.cpp` `LoadStageFiles`, `RSDKv4/Userdata.cpp`) and the
//! RSDKv5 legacy v4 loaders (`RSDKv5/RSDK/Core/Legacy/v4/RetroEnginev4.cpp`,
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`), which are behaviourally identical for
//! these files. All multi-byte integers on disk are little-endian and all strings are
//! length-prefixed with a single `u8` (see [`Reader::read_string`]).
//!
//! Scope for the M1 WP1b milestone:
//!
//! * [`gameconfig`] — `Data/Game/GameConfig.bin`
//! * [`stageconfig`] — `Data/Stages/<folder>/StageConfig.bin`
//! * [`settings`] — `Settings.ini`
//! * [`userdata`] — `SGame.bin`/`SData.bin`, `Achievements.bin` and `UData.bin`
//!
//! Scene/tile/animation formats are intentionally not implemented here (WP1c).
//!
//! The crate performs no filesystem access of its own: every `load` helper reads through a
//! [`retro_io::DataSource`].

#![forbid(unsafe_code)]

pub mod error;
pub mod gameconfig;
pub mod reader;
pub mod settings;
pub mod stageconfig;
pub mod userdata;

pub use error::FormatError;
pub use gameconfig::{
    GameConfig, GlobalVariable, ObjectInfo, SceneCategory, SceneEntry, SoundEffect,
};
pub use reader::Reader;
pub use settings::{AudioSettings, GameSettings, GameType, KeyboardMap, Settings, VideoSettings};
pub use stageconfig::StageConfig;
pub use userdata::{
    Achievement, Achievements, LeaderboardEntry, SaveFile, SaveFileKind, SaveGame, SaveRam,
    UserData,
};
