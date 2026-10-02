//! Parser for the RSDKv4 `Data/Game/GameConfig.bin` file.
//!
//! On-disk order (RSDKv4-Decompilation `RetroEngine::LoadGameConfig` in
//! `RSDKv4/RetroEngine.cpp`; identical in `RSDKv5/RSDK/Core/Legacy/v4/RetroEnginev4.cpp`):
//!
//! ```text
//! string   window title          (gameTitle)
//! string   window description    (gameSubtitle)
//! u8[3]    master palette, 0x60 entries
//! u8       object count
//! string[] object names          (object count)
//! string[] object script paths   (object count, paired with the names by index)
//! u8       global variable count
//! repeat: string name, i32 little-endian value
//! u8       global sound effect count
//! string[] sound effect names    (sfx count)
//! string[] sound effect paths    (sfx count, paired by index)
//! u8       player count
//! string[] player names
//! repeat 4 times (Presentation, Regular, Special, Bonus):
//!     u8       scene count
//!     repeat: string folder, string id, string name, u8 highlighted
//! ```
//!
//! The file stores no numeric engine version; the game release type (`gameType`) and other
//! engine settings live in `Settings.ini` (see [`crate::settings`]). The RSDKv5 legacy loader
//! names the two leading strings `gameTitle`/`gameSubtitle`, while RSDKv4 calls them
//! `gameWindowText`/`gameDescriptionText`.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// Number of master palette entries in a `GameConfig.bin`.
pub const PALETTE_COUNT: usize = 0x60;
/// Number of scene categories stored in a `GameConfig.bin`.
pub const CATEGORY_COUNT: usize = 4;
/// Category labels assigned by the engine, in file order.
pub const CATEGORY_NAMES: [&str; CATEGORY_COUNT] = ["Presentation", "Regular", "Special", "Bonus"];

/// An object type: the display name and the script path, stored as two parallel lists in the
/// file and paired up by index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectInfo {
    /// Object type name as shown in the editor (`Player Object`).
    pub name: String,
    /// Script path relative to `Data/Scripts` (`Players/PlayerObject.txt`).
    pub script_path: String,
}

/// A global script variable with its initial value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GlobalVariable {
    /// Variable name (`options.devMenuFlag`).
    pub name: String,
    /// Initial little-endian `i32` value.
    pub value: i32,
}

/// A sound effect definition (`name` + `path`), used by both the game and stage configs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SoundEffect {
    /// Script-visible sound effect name (`Jump`).
    pub name: String,
    /// Path relative to `Data/SoundFX` (`Global/Jump.wav`).
    pub path: String,
}

/// One entry of a scene category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneEntry {
    /// Folder under `Data/Stages` (`Zone01`).
    pub folder: String,
    /// Act/stage id inside the folder (`1`).
    pub id: String,
    /// Display name shown on the level select (`GREEN HILL ZONE 1`).
    pub name: String,
    /// Raw "highlighted" byte; the engine only tests it for truthiness.
    pub highlighted: u8,
}

/// A scene category with the fixed engine-assigned name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneCategory {
    /// Engine-assigned category name, one of [`CATEGORY_NAMES`].
    pub name: String,
    /// Scenes in file order.
    pub scenes: Vec<SceneEntry>,
}

/// Parsed `Data/Game/GameConfig.bin`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GameConfig {
    /// Window title (`Sonic 1`).
    pub title: String,
    /// Window description; may contain `\n` line separators.
    pub subtitle: String,
    /// Master palette, exactly [`PALETTE_COUNT`] RGB888 entries.
    pub palette: Vec<[u8; 3]>,
    /// Object type names paired with their script paths.
    pub objects: Vec<ObjectInfo>,
    /// Global variables with initial values.
    pub global_variables: Vec<GlobalVariable>,
    /// Global sound effects.
    pub sound_effects: Vec<SoundEffect>,
    /// Player names used by `PlayerName[]` script functions.
    pub players: Vec<String>,
    /// The four scene categories in file order.
    pub categories: Vec<SceneCategory>,
}

impl GameConfig {
    /// Canonical asset path passed to [`GameConfig::load`].
    pub const PATH: &str = "Data/Game/GameConfig.bin";

    /// Parses a `GameConfig.bin` from memory.
    ///
    /// Trailing bytes after the last scene entry are ignored, matching the streaming loader in
    /// the engine.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);

        let title = reader.read_string()?;
        let subtitle = reader.read_string()?;

        let mut palette = Vec::with_capacity(PALETTE_COUNT);
        for _ in 0..PALETTE_COUNT {
            palette.push(reader.read_array::<3>()?);
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

        let variable_count = reader.read_u8()? as usize;
        let mut global_variables = Vec::with_capacity(variable_count);
        for _ in 0..variable_count {
            global_variables.push(GlobalVariable {
                name: reader.read_string()?,
                value: reader.read_i32_le()?,
            });
        }

        let sfx_count = reader.read_u8()? as usize;
        let mut sound_effects = Vec::with_capacity(sfx_count);
        let mut sfx_names = Vec::with_capacity(sfx_count);
        for _ in 0..sfx_count {
            sfx_names.push(reader.read_string()?);
        }
        for name in sfx_names {
            sound_effects.push(SoundEffect {
                name,
                path: reader.read_string()?,
            });
        }

        let player_count = reader.read_u8()? as usize;
        let mut players = Vec::with_capacity(player_count);
        for _ in 0..player_count {
            players.push(reader.read_string()?);
        }

        let mut categories = Vec::with_capacity(CATEGORY_COUNT);
        for name in CATEGORY_NAMES {
            let scene_count = reader.read_u8()? as usize;
            let mut scenes = Vec::with_capacity(scene_count);
            for _ in 0..scene_count {
                scenes.push(SceneEntry {
                    folder: reader.read_string()?,
                    id: reader.read_string()?,
                    name: reader.read_string()?,
                    highlighted: reader.read_u8()?,
                });
            }
            categories.push(SceneCategory {
                name: name.to_owned(),
                scenes,
            });
        }

        Ok(Self {
            title,
            subtitle,
            palette,
            objects,
            global_variables,
            sound_effects,
            players,
            categories,
        })
    }

    /// Reads [`GameConfig::PATH`] through `src` and parses it.
    pub fn load(src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(Self::PATH)?)
    }

    /// Total number of scenes across all categories.
    pub fn scene_count(&self) -> usize {
        self.categories
            .iter()
            .map(|category| category.scenes.len())
            .sum()
    }

    /// Looks up a global variable by name.
    pub fn global_variable(&self, name: &str) -> Option<i32> {
        self.global_variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    struct Fixture {
        title: String,
        subtitle: String,
        palette: [[u8; 3]; PALETTE_COUNT],
        object_names: Vec<String>,
        script_paths: Vec<String>,
        variables: Vec<(String, i32)>,
        sfx_names: Vec<String>,
        sfx_paths: Vec<String>,
        players: Vec<String>,
        categories: Vec<Vec<(String, String, String, u8)>>,
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    impl Fixture {
        fn minimal() -> Self {
            Self {
                title: "Sonic 1".to_owned(),
                subtitle: "line one\nline two".to_owned(),
                palette: [[0; 3]; PALETTE_COUNT],
                object_names: Vec::new(),
                script_paths: Vec::new(),
                variables: Vec::new(),
                sfx_names: Vec::new(),
                sfx_paths: Vec::new(),
                players: Vec::new(),
                categories: vec![Vec::new(); CATEGORY_COUNT],
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            push_string(&mut bytes, &self.title);
            push_string(&mut bytes, &self.subtitle);
            for entry in &self.palette {
                bytes.extend_from_slice(entry);
            }
            assert_eq!(self.object_names.len(), self.script_paths.len());
            bytes.push(self.object_names.len() as u8);
            for name in &self.object_names {
                push_string(&mut bytes, name);
            }
            for path in &self.script_paths {
                push_string(&mut bytes, path);
            }
            bytes.push(self.variables.len() as u8);
            for (name, value) in &self.variables {
                push_string(&mut bytes, name);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            assert_eq!(self.sfx_names.len(), self.sfx_paths.len());
            bytes.push(self.sfx_names.len() as u8);
            for name in &self.sfx_names {
                push_string(&mut bytes, name);
            }
            for path in &self.sfx_paths {
                push_string(&mut bytes, path);
            }
            bytes.push(self.players.len() as u8);
            for player in &self.players {
                push_string(&mut bytes, player);
            }
            for category in &self.categories {
                bytes.push(category.len() as u8);
                for (folder, id, name, highlighted) in category {
                    push_string(&mut bytes, folder);
                    push_string(&mut bytes, id);
                    push_string(&mut bytes, name);
                    bytes.push(*highlighted);
                }
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
        let fixture = Fixture::minimal();
        let config = GameConfig::from_bytes(&fixture.build()).unwrap();
        assert_eq!(config.title, "Sonic 1");
        assert_eq!(config.subtitle, "line one\nline two");
        assert_eq!(config.palette.len(), PALETTE_COUNT);
        assert!(config.objects.is_empty());
        assert!(config.global_variables.is_empty());
        assert!(config.sound_effects.is_empty());
        assert!(config.players.is_empty());
        assert_eq!(config.categories.len(), CATEGORY_COUNT);
        assert!(
            config
                .categories
                .iter()
                .all(|category| category.scenes.is_empty())
        );
        assert_eq!(config.scene_count(), 0);
        assert_eq!(config.categories[0].name, "Presentation");
        assert_eq!(config.categories[3].name, "Bonus");
    }

    #[test]
    fn pairs_names_and_scripts_by_index() {
        let mut fixture = Fixture::minimal();
        fixture.object_names = strings(&["First", "Second"]);
        fixture.script_paths = strings(&["Objects/First.txt", "Objects/Second.txt"]);
        let config = GameConfig::from_bytes(&fixture.build()).unwrap();
        assert_eq!(
            config.objects,
            [
                ObjectInfo {
                    name: "First".to_owned(),
                    script_path: "Objects/First.txt".to_owned(),
                },
                ObjectInfo {
                    name: "Second".to_owned(),
                    script_path: "Objects/Second.txt".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn parses_all_sections() {
        let mut fixture = Fixture::minimal();
        fixture.palette[1] = [1, 2, 3];
        fixture.palette[PALETTE_COUNT - 1] = [253, 254, 255];
        fixture.object_names = strings(&["Player"]);
        fixture.script_paths = strings(&["Players/Player.txt"]);
        fixture.variables = vec![
            ("options.hi".to_owned(), -1234),
            ("engine.mode".to_owned(), 0x0102_0304),
        ];
        fixture.sfx_names = strings(&["Jump", "Ring"]);
        fixture.sfx_paths = strings(&["Global/Jump.wav", "Global/Ring.wav"]);
        fixture.players = strings(&["SONIC", "TAILS"]);
        fixture.categories[0] = vec![("Title", "1", "TITLE", 1)]
            .into_iter()
            .map(|(folder, id, name, highlighted)| {
                (
                    folder.to_owned(),
                    id.to_owned(),
                    name.to_owned(),
                    highlighted,
                )
            })
            .collect();
        fixture.categories[1] = vec![("Zone01", "1", "GHZ 1", 1), ("Zone01", "2", "2", 0)]
            .into_iter()
            .map(|(folder, id, name, highlighted)| {
                (
                    folder.to_owned(),
                    id.to_owned(),
                    name.to_owned(),
                    highlighted,
                )
            })
            .collect();

        let config = GameConfig::from_bytes(&fixture.build()).unwrap();
        assert_eq!(config.palette[1], [1, 2, 3]);
        assert_eq!(config.palette[PALETTE_COUNT - 1], [253, 254, 255]);
        assert_eq!(config.global_variables[0].value, -1234);
        assert_eq!(config.global_variables[1].value, 0x0102_0304);
        assert_eq!(config.sound_effects[1].name, "Ring");
        assert_eq!(config.sound_effects[1].path, "Global/Ring.wav");
        assert_eq!(config.players, ["SONIC", "TAILS"]);
        assert_eq!(config.categories[1].scenes[1].id, "2");
        assert_eq!(config.categories[1].scenes[1].highlighted, 0);
        assert_eq!(config.scene_count(), 3);
        assert_eq!(config.global_variable("options.hi"), Some(-1234));
        assert_eq!(config.global_variable("missing"), None);
    }

    #[test]
    fn parses_maximum_counts() {
        let mut fixture = Fixture::minimal();
        let names: Vec<String> = (0..255).map(|index| format!("Object{index}")).collect();
        fixture.object_names = names.clone();
        let paths: Vec<String> = (0..255).map(|index| format!("Path{index}.txt")).collect();
        fixture.script_paths = paths.clone();
        fixture.variables = (0..255).map(|index| (format!("var{index}"), -1)).collect();
        fixture.sfx_names = names.clone();
        fixture.sfx_paths = paths.clone();
        fixture.players = names;
        fixture.categories = (0..CATEGORY_COUNT)
            .map(|_| {
                (0..255)
                    .map(|index| {
                        (
                            "Folder".to_owned(),
                            "1".to_owned(),
                            format!("Scene{index}"),
                            0,
                        )
                    })
                    .collect()
            })
            .collect();

        let config = GameConfig::from_bytes(&fixture.build()).unwrap();
        assert_eq!(config.objects.len(), 255);
        assert_eq!(config.global_variables.len(), 255);
        assert_eq!(config.sound_effects.len(), 255);
        assert_eq!(config.players.len(), 255);
        assert_eq!(config.scene_count(), 255 * CATEGORY_COUNT);
    }

    #[test]
    fn truncated_inputs_error_cleanly() {
        let mut fixture = Fixture::minimal();
        fixture.object_names = strings(&["Object"]);
        fixture.script_paths = strings(&["Object.txt"]);
        fixture.variables = vec![("var".to_owned(), 7)];
        fixture.sfx_names = strings(&["Sfx"]);
        fixture.sfx_paths = strings(&["Sfx.wav"]);
        fixture.players = strings(&["SONIC"]);
        fixture.categories[2] = vec![("Special", "1", "SPECIAL", 1)]
            .into_iter()
            .map(|(folder, id, name, highlighted)| {
                (
                    folder.to_owned(),
                    id.to_owned(),
                    name.to_owned(),
                    highlighted,
                )
            })
            .collect();
        let bytes = fixture.build();

        for cut in 0..bytes.len() {
            let result = GameConfig::from_bytes(&bytes[..cut]);
            assert!(
                result.is_err(),
                "prefix of {cut} bytes unexpectedly parsed: {result:?}"
            );
        }
        assert!(GameConfig::from_bytes(&bytes).is_ok());
    }

    #[test]
    fn rejects_invalid_utf8_strings() {
        let mut bytes = Vec::new();
        bytes.push(2);
        bytes.extend_from_slice(&[0xC3, 0x28]);
        bytes.extend_from_slice(&Fixture::minimal().build());
        assert!(matches!(
            GameConfig::from_bytes(&bytes),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn accepts_trailing_bytes() {
        let mut bytes = Fixture::minimal().build();
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let config = GameConfig::from_bytes(&bytes).unwrap();
        assert_eq!(config.title, "Sonic 1");
    }

    #[test]
    fn never_panics_on_mutated_input() {
        let mut bytes = Fixture::minimal().build();
        bytes[0] = 255;
        for index in 0..bytes.len() {
            let _ = GameConfig::from_bytes(&bytes[..index]);
        }
        let mut state = 0x1234_5678u32;
        for _ in 0..512 {
            let mut mutated = bytes.clone();
            for byte in &mut mutated {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte ^= (state >> 24) as u8;
            }
            let _ = GameConfig::from_bytes(&mutated);
        }
    }

    #[test]
    fn loads_through_data_source_case_insensitively() {
        let mut source = MemorySource::new();
        source.insert("Data/Game/GameConfig.bin", Fixture::minimal().build());
        let config = GameConfig::load(&source).unwrap();
        assert_eq!(config.title, "Sonic 1");
    }
}
