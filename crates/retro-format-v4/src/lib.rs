//! Parsers and writers for RSDK v4 legacy data formats.
//!
//! The byte layouts mirror the loaders of RSDKv4-Decompilation (`RSDKv4/RetroEngine.cpp`
//! `LoadGameConfig`, `RSDKv4/Scene.cpp` `LoadStageFiles`, `RSDKv4/Animation.cpp`,
//! `RSDKv4/Palette.cpp`, `RSDKv4/Userdata.cpp`) and the RSDKv5 legacy v4 loaders
//! (`RSDKv5/RSDK/Core/Legacy/v4/RetroEnginev4.cpp`,
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`,
//! `RSDKv5/RSDK/Graphics/Legacy/AnimationLegacy.cpp`), which are behaviourally identical for
//! these files. All multi-byte integers on disk are little-endian and all strings are
//! length-prefixed with a single `u8` (see [`Reader::read_string`]).
//!
//! Scope for the M1 WP1b/WP1c milestones:
//!
//! * [`gameconfig`] — `Data/Game/GameConfig.bin`
//! * [`stageconfig`] — `Data/Stages/<folder>/StageConfig.bin`
//! * [`scene`] — `Data/Stages/<folder>/ActN.bin`
//! * [`tiles`] — `Data/Stages/<folder>/128x128Tiles.bin` and `16x16Tiles.gif`
//! * [`collision`] — `Data/Stages/<folder>/CollisionMasks.bin`
//! * [`background`] — `Data/Stages/<folder>/Backgrounds.bin`
//! * [`animation`] — `Data/Animations/*.ani`
//! * [`palette`] — `Data/Palettes/*.act`
//! * [`settings`] — `Settings.ini`
//! * [`userdata`] — `SGame.bin`/`SData.bin`, `Achievements.bin` and `UData.bin`
//!
//! The crate performs no filesystem access of its own: every `load` helper reads through a
//! [`retro_io::DataSource`].

#![forbid(unsafe_code)]

pub mod animation;
pub mod background;
pub mod collision;
pub mod error;
pub mod gameconfig;
pub mod palette;
pub mod reader;
pub mod scene;
pub mod settings;
pub mod stageconfig;
pub mod tiles;
pub mod userdata;

pub use animation::{Animation, AnimationFile, AnimationFrame, Hitbox};
pub use background::{BackgroundLayer, Backgrounds, ParallaxEntry};
pub use collision::{CollisionMasks, CollisionPlane, CollisionTile};
pub use error::FormatError;
pub use gameconfig::{
    GameConfig, GlobalVariable, ObjectInfo, SceneCategory, SceneEntry, SoundEffect,
};
pub use palette::Palette;
pub use reader::Reader;
pub use scene::{Scene, SceneEntity};
pub use settings::{AudioSettings, GameSettings, GameType, KeyboardMap, Settings, VideoSettings};
pub use stageconfig::StageConfig;
pub use tiles::{Tile128, TileSheet16, TileSheet128};
pub use userdata::{
    Achievement, Achievements, LeaderboardEntry, SaveFile, SaveFileKind, SaveGame, SaveRam,
    UserData,
};
