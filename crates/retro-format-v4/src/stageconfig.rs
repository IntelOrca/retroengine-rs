//! Parser for the RSDKv4 `Data/Stages/<folder>/StageConfig.bin` file.
//!
//! On-disk order (RSDKv4-Decompilation `LoadStageFiles` in `RSDKv4/Scene.cpp`; identical in
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`):
//!
//! ```text
//! u8       load global objects/scripts flag
//! u8[3]    stage palette, 0x20 entries (engine palette indices 0x60..0x7F)
//! u8       stage sound effect count
//! string[] sound effect names
//! string[] sound effect paths
//! u8       stage object count
//! string[] object names
//! string[] object script paths
//! ```
//!
//! The "load global objects" flag is exposed as [`StageConfig::load_global_objects`]; upstream
//! calls the same byte `loadGlobalScripts` (RSDKv4) / `loadGlobalScripts` (RSDKv5 legacy).

use serde::Serialize;

use crate::error::FormatError;
use crate::gameconfig::{ObjectInfo, SoundEffect};
use crate::reader::Reader;
use retro_io::DataSource;

/// Number of stage palette entries (palette indices `0x60..=0x7F`).
pub const STAGE_PALETTE_COUNT: usize = 0x20;
/// File name of the per-stage configuration inside a stage folder.
pub const STAGE_CONFIG_FILE: &str = "StageConfig.bin";

/// Parsed `StageConfig.bin` for one stage folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageConfig {
    /// When set, the engine also loads the global object scripts from `GameConfig.bin` for this
    /// scene.
    pub load_global_objects: bool,
    /// Stage palette, exactly [`STAGE_PALETTE_COUNT`] RGB888 entries.
    pub palette: Vec<[u8; 3]>,
    /// Stage-local sound effects.
    pub sound_effects: Vec<SoundEffect>,
    /// Stage objects with their script paths.
    pub objects: Vec<ObjectInfo>,
}

impl StageConfig {
    /// Parses a `StageConfig.bin` from memory.
    ///
    /// Trailing bytes after the last script path are ignored, matching the engine's streaming
    /// loader.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);

        let load_global_objects = reader.read_u8()? != 0;

        let mut palette = Vec::with_capacity(STAGE_PALETTE_COUNT);
        for _ in 0..STAGE_PALETTE_COUNT {
            palette.push(reader.read_array::<3>()?);
        }

        let sfx_count = reader.read_u8()? as usize;
        let mut sfx_names = Vec::with_capacity(sfx_count);
        for _ in 0..sfx_count {
            sfx_names.push(reader.read_string()?);
        }
        let mut sound_effects = Vec::with_capacity(sfx_count);
        for name in sfx_names {
            sound_effects.push(SoundEffect {
                name,
                path: reader.read_string()?,
            });
        }

        let object_count = reader.read_u8()? as usize;
        let mut object_names = Vec::with_capacity(object_count);
        for _ in 0..object_count {
            object_names.push(reader.read_string()?);
        }
        let mut objects = Vec::with_capacity(object_count);
        for name in object_names {
            objects.push(ObjectInfo {
                name,
                script_path: reader.read_string()?,
            });
        }

        Ok(Self {
            load_global_objects,
            palette,
            sound_effects,
            objects,
        })
    }

    /// Reads `<stage_dir>/StageConfig.bin` through `src` and parses it.
    ///
    /// `stage_dir` is a path relative to the source root, e.g. `Data/Stages/Zone01`. A trailing
    /// slash is accepted.
    pub fn load(stage_dir: &str, src: &dyn DataSource) -> Result<Self, FormatError> {
        let directory = stage_dir.trim_end_matches('/');
        let path = if directory.is_empty() {
            STAGE_CONFIG_FILE.to_owned()
        } else {
            format!("{directory}/{STAGE_CONFIG_FILE}")
        };
        Self::from_bytes(&src.read(&path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    struct Fixture {
        load_global_objects: bool,
        palette: [[u8; 3]; STAGE_PALETTE_COUNT],
        sfx_names: Vec<String>,
        sfx_paths: Vec<String>,
        object_names: Vec<String>,
        script_paths: Vec<String>,
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    impl Fixture {
        fn minimal() -> Self {
            Self {
                load_global_objects: false,
                palette: [[0; 3]; STAGE_PALETTE_COUNT],
                sfx_names: Vec::new(),
                sfx_paths: Vec::new(),
                object_names: Vec::new(),
                script_paths: Vec::new(),
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.push(u8::from(self.load_global_objects));
            for entry in &self.palette {
                bytes.extend_from_slice(entry);
            }
            assert_eq!(self.sfx_names.len(), self.sfx_paths.len());
            bytes.push(self.sfx_names.len() as u8);
            for name in &self.sfx_names {
                push_string(&mut bytes, name);
            }
            for path in &self.sfx_paths {
                push_string(&mut bytes, path);
            }
            assert_eq!(self.object_names.len(), self.script_paths.len());
            bytes.push(self.object_names.len() as u8);
            for name in &self.object_names {
                push_string(&mut bytes, name);
            }
            for path in &self.script_paths {
                push_string(&mut bytes, path);
            }
            bytes
        }
    }

    fn push_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
    }

    #[test]
    fn parses_minimal_config() {
        let config = StageConfig::from_bytes(&Fixture::minimal().build()).unwrap();
        assert!(!config.load_global_objects);
        assert_eq!(config.palette.len(), STAGE_PALETTE_COUNT);
        assert!(config.sound_effects.is_empty());
        assert!(config.objects.is_empty());
    }

    #[test]
    fn parses_all_sections() {
        let mut fixture = Fixture::minimal();
        fixture.load_global_objects = true;
        fixture.palette[0] = [10, 20, 30];
        fixture.palette[STAGE_PALETTE_COUNT - 1] = [40, 50, 60];
        fixture.sfx_names = strings(&["Boss Hit"]);
        fixture.sfx_paths = strings(&["Stage/BossHit.wav"]);
        fixture.object_names = strings(&["Setup", "H Platform"]);
        fixture.script_paths = strings(&["GHZ/GHZSetup.txt", "EHZ/HPlatform.txt"]);

        let config = StageConfig::from_bytes(&fixture.build()).unwrap();
        assert!(config.load_global_objects);
        assert_eq!(config.palette[0], [10, 20, 30]);
        assert_eq!(config.palette[STAGE_PALETTE_COUNT - 1], [40, 50, 60]);
        assert_eq!(config.sound_effects[0].name, "Boss Hit");
        assert_eq!(config.sound_effects[0].path, "Stage/BossHit.wav");
        assert_eq!(config.objects[1].name, "H Platform");
        assert_eq!(config.objects[1].script_path, "EHZ/HPlatform.txt");
    }

    #[test]
    fn parses_maximum_counts() {
        let mut fixture = Fixture::minimal();
        let names: Vec<String> = (0..255).map(|index| format!("Object{index}")).collect();
        fixture.object_names = names.clone();
        let paths: Vec<String> = (0..255).map(|index| format!("Path{index}.txt")).collect();
        fixture.script_paths = paths.clone();
        fixture.sfx_names = names;
        fixture.sfx_paths = paths;

        let config = StageConfig::from_bytes(&fixture.build()).unwrap();
        assert_eq!(config.objects.len(), 255);
        assert_eq!(config.objects[254].script_path, "Path254.txt");
        assert_eq!(config.sound_effects.len(), 255);
    }

    #[test]
    fn load_global_flag_is_nonzero() {
        let mut bytes = Fixture::minimal().build();
        bytes[0] = 0x80;
        assert!(StageConfig::from_bytes(&bytes).unwrap().load_global_objects);
    }

    #[test]
    fn truncated_inputs_error_cleanly() {
        let mut fixture = Fixture::minimal();
        fixture.load_global_objects = true;
        fixture.sfx_names = strings(&["Sfx"]);
        fixture.sfx_paths = strings(&["Sfx.wav"]);
        fixture.object_names = strings(&["Object"]);
        fixture.script_paths = strings(&["Object.txt"]);
        let bytes = fixture.build();

        for cut in 0..bytes.len() {
            assert!(
                StageConfig::from_bytes(&bytes[..cut]).is_err(),
                "prefix of {cut} bytes unexpectedly parsed"
            );
        }
        assert!(StageConfig::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn rejects_invalid_utf8_strings() {
        let mut bytes = Vec::new();
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; STAGE_PALETTE_COUNT * 3]);
        bytes.push(0);
        bytes.push(1);
        bytes.extend_from_slice(&[1, 0x80]);
        assert!(matches!(
            StageConfig::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn loads_from_stage_directory() {
        let mut source = MemorySource::new();
        source.insert(
            "Data/Stages/Zone01/StageConfig.bin",
            Fixture::minimal().build(),
        );
        let config = StageConfig::load("Data/Stages/Zone01", &source).unwrap();
        assert!(config.objects.is_empty());
        let config = StageConfig::load("Data/Stages/Zone01/", &source).unwrap();
        assert!(config.objects.is_empty());
        assert!(matches!(
            StageConfig::load("Data/Stages/Missing", &source),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0xDEAD_BEEFu32;
        for length in 0..512usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = StageConfig::from_bytes(&bytes);
        }
    }
}
