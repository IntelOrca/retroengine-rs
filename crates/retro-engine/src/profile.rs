//! Runtime engine profiles.
//!
//! The profile captures behaviour that differs between data families and game releases. It is
//! selected from the loaded data at run time; no `#[cfg]` switches are used, mirroring the
//! project decision log.

use retro_format_v4::{GameType, Settings};
use retro_script::{PlatformMode, V4Revision};

/// The data/behaviour family the engine is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProfile {
    /// RSDKv4 legacy data (Sonic 1 / Sonic 2 Origins assets).
    V4Legacy,
}

impl RuntimeProfile {
    /// Stable lower-case name (`"v4-legacy"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::V4Legacy => "v4-legacy",
        }
    }

    /// Script revision used to compile `Data/Scripts` sources for this profile.
    ///
    /// The shipped assets are Origins-era, so rev03 is the current default; standalone data
    /// would use one of rev00..rev02 and will be selected here once those assets are supported.
    #[must_use]
    pub const fn script_revision(self) -> V4Revision {
        V4Revision::Rev03
    }

    /// `#platform` mode derived from `[Game] gameType`.
    #[must_use]
    pub const fn platform_mode(game_type: GameType) -> PlatformMode {
        match game_type {
            GameType::Origins => PlatformMode::Origins,
            _ => PlatformMode::Standalone,
        }
    }
}

/// Behaviour switches resolved from `Settings.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSettings {
    /// The selected runtime profile.
    pub profile: RuntimeProfile,
    /// `#platform` mode for the script compiler.
    pub platform: PlatformMode,
    /// Script revision for the compiler and VM.
    pub revision: V4Revision,
    /// `[Game] txtScripts`: prefer `Data/Scripts` sources over `Bytecode/`.
    pub force_scripts: bool,
    /// `[Window] DimLimit` converted to frames (`seconds * refresh_rate`, or `-1` disabled),
    /// mirroring `Userdata.cpp:446-449`.
    pub dim_limit_frames: i32,
}

impl EngineSettings {
    /// Resolves the engine settings from parsed `Settings.ini`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        let refresh_rate = settings.video.refresh_rate.max(1);
        let dim_limit_frames = if settings.video.dim_limit >= 0 {
            settings.video.dim_limit.saturating_mul(refresh_rate)
        } else {
            -1
        };
        Self {
            profile: RuntimeProfile::V4Legacy,
            platform: RuntimeProfile::platform_mode(settings.game.game_type),
            revision: RuntimeProfile::V4Legacy.script_revision(),
            force_scripts: settings.game.txt_scripts,
            dim_limit_frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_game_type_selects_origins_platform() {
        assert_eq!(
            RuntimeProfile::platform_mode(GameType::Origins),
            PlatformMode::Origins
        );
        assert_eq!(
            RuntimeProfile::platform_mode(GameType::Standalone),
            PlatformMode::Standalone
        );
        assert_eq!(
            RuntimeProfile::platform_mode(GameType::Other(7)),
            PlatformMode::Standalone
        );
    }

    #[test]
    fn dim_limit_is_converted_to_frames() {
        let mut settings = Settings::default();
        settings.video.dim_limit = 300;
        settings.video.refresh_rate = 60;
        assert_eq!(
            EngineSettings::from_settings(&settings).dim_limit_frames,
            18000
        );
        settings.video.dim_limit = -1;
        assert_eq!(
            EngineSettings::from_settings(&settings).dim_limit_frames,
            -1
        );
        settings.video.dim_limit = 2;
        settings.video.refresh_rate = 0;
        assert_eq!(
            EngineSettings::from_settings(&settings).dim_limit_frames,
            2,
            "a zero refresh rate is treated as one frame per second"
        );
    }

    #[test]
    fn profile_names_are_stable() {
        assert_eq!(RuntimeProfile::V4Legacy.name(), "v4-legacy");
        assert_eq!(
            RuntimeProfile::V4Legacy.script_revision(),
            V4Revision::Rev03
        );
    }
}
