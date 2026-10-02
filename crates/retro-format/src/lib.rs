//! Version-agnostic facade over the supported RSDK data formats.
//!
//! The engine needs to know which format family an asset folder is before it can load anything
//! else. Today only the v4 legacy layout (`Data/Game/GameConfig.bin` + `Settings.ini`) is
//! supported; [`detect`] performs that detection at run time (never with `#[cfg]`) and returns
//! the parsed settings and game config, leaving later format families room to be added without
//! touching the engine entry point.
//!
//! ```
//! use retro_format::{DataVersion, detect};
//! use retro_io::MemorySource;
//!
//! let mut source = MemorySource::new();
//! source.insert("Settings.ini", "[Game]\ngameType=1\n");
//! // A GameConfig.bin is required as well; see the crate tests for a fixture.
//! assert_eq!(DataVersion::V4Legacy.name(), "v4-legacy");
//! ```
#![forbid(unsafe_code)]

use retro_format_v4::{FormatError, GameConfig, GameType, Settings};
use retro_io::DataSource;

/// Identifies the on-disk data family of an asset folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataVersion {
    /// RSDKv4 legacy data (`Data/Game/GameConfig.bin`, `_Bytecode` or `Data/Scripts`).
    V4Legacy,
}

impl DataVersion {
    /// Stable lower-case name (`"v4-legacy"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::V4Legacy => "v4-legacy",
        }
    }
}

/// An asset folder whose format family has been identified.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedGame {
    /// The detected data family.
    pub version: DataVersion,
    /// Parsed `Settings.ini`.
    pub settings: Settings,
    /// Parsed `Data/Game/GameConfig.bin`.
    pub game_config: GameConfig,
}

impl DetectedGame {
    /// The engine game release type (`[Game] gameType`).
    #[must_use]
    pub fn game_type(&self) -> GameType {
        self.settings.game.game_type
    }

    /// Whether the engine should ignore `Bytecode/` and compile `Data/Scripts` sources
    /// (`[Game] txtScripts`).
    #[must_use]
    pub fn force_scripts(&self) -> bool {
        self.settings.game.txt_scripts
    }
}

/// Detects and loads the game metadata of the asset folder backed by `source`.
///
/// The detection is deliberately shallow: presence of `Data/Game/GameConfig.bin` identifies a
/// v4 legacy tree, and parsing it validates that assumption.
pub fn detect(source: &dyn DataSource) -> Result<DetectedGame, FormatError> {
    let game_config = GameConfig::load(source)?;
    let settings = Settings::load(source)?;
    Ok(DetectedGame {
        version: DataVersion::V4Legacy,
        settings,
        game_config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_format_v4::gameconfig::PALETTE_COUNT;
    use retro_io::MemorySource;

    fn game_config_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        for text in ["Test", ""] {
            bytes.push(text.len() as u8);
            bytes.extend_from_slice(text.as_bytes());
        }
        for _ in 0..PALETTE_COUNT {
            bytes.extend_from_slice(&[0, 0, 0]);
        }
        bytes.push(0); // objects
        bytes.push(0); // variables
        bytes.push(0); // sfx
        bytes.push(0); // players
        bytes.extend(std::iter::repeat_n(0u8, 4)); // categories
        bytes
    }

    fn source() -> MemorySource {
        let mut source = MemorySource::new();
        source.insert("Data/Game/GameConfig.bin", game_config_bytes());
        source.insert("Settings.ini", "[Game]\ngameType=1\ntxtScripts=n\n");
        source
    }

    #[test]
    fn detects_v4_legacy_tree() {
        let detected = detect(&source()).unwrap();
        assert_eq!(detected.version, DataVersion::V4Legacy);
        assert_eq!(detected.version.name(), "v4-legacy");
        assert_eq!(detected.game_type(), GameType::Origins);
        assert!(!detected.force_scripts());
        assert_eq!(detected.game_config.title, "Test");
    }

    #[test]
    fn txt_scripts_flag_is_exposed() {
        let mut source = source();
        source.insert("Settings.ini", "[Game]\ntxtScripts=y\n");
        let detected = detect(&source).unwrap();
        assert!(detected.force_scripts());
        // Default gameType is Origins when the key is missing.
        assert_eq!(detected.game_type(), GameType::Origins);
    }

    #[test]
    fn missing_game_config_is_an_io_error() {
        let mut source = MemorySource::new();
        source.insert("Settings.ini", "[Game]\n");
        assert!(detect(&source).is_err());
    }

    #[test]
    fn missing_settings_is_an_io_error() {
        let mut source = MemorySource::new();
        source.insert("Data/Game/GameConfig.bin", game_config_bytes());
        assert!(detect(&source).is_err());
    }

    #[test]
    fn standalone_game_type_is_preserved() {
        let mut source = source();
        source.insert("Settings.ini", "[Game]\ngameType=0\n");
        let detected = detect(&source).unwrap();
        assert_eq!(detected.game_type(), GameType::Standalone);
    }

    #[test]
    fn other_game_type_is_preserved() {
        let mut source = source();
        source.insert("Settings.ini", "[Game]\ngameType=7\n");
        let detected = detect(&source).unwrap();
        assert_eq!(detected.game_type(), GameType::Other(7));
    }
}
