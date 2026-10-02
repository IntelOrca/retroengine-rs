//! Asset loading: settings, configs, scenes and the linked script groups.
//!
//! Mirrors `LoadStageFiles` in `RSDKv4/Scene.cpp` (RSDKModding/RSDKv4-Decompilation @ a7f5195):
//!
//! * `StageConfig.bin` decides whether the global object scripts are loaded before the stage
//!   objects.
//! * Global objects occupy type ids `1..=global_count`; stage objects follow, so type ids in
//!   the act file are absolute.
//! * Global and stage sources are compiled with one shared [`Compiler`], then split into the
//!   shipped group layout with [`Compiler::finish_group`]: the stage file stores only its own
//!   code but keeps the global functions and absolute positions.
//!
//! # `_Bytecode/` deviation (M7 triage)
//!
//! The settled project decision is that the reference engine ignores `_Bytecode/` (the underscore
//! prefix makes it a non-loaded directory), so the shipped S1/S2 folders are loaded by compiling
//! `Data/Scripts/**/*.txt`. This loader therefore always takes the compile path and reports but
//! does not honour `[Game] txtScripts`. For M7 parity this is the *same* path the reference takes
//! for these assets; if a future data set ships a real `Bytecode/` directory, loading it (the
//! `retro_script::load_bytecode` entry point already exists) will need to be added here and
//! validated against the compiler output.

use std::sync::Arc;

use retro_format::{DataVersion, detect};
use retro_format_v4::{
    Achievements, Backgrounds, CollisionMasks, GameConfig, Scene, StageConfig, TileSheet16,
    TileSheet128,
};
use retro_io::DataSource;
use retro_scene::strip_spaces;
use retro_scene::{ObjectRegistry, SceneCollision, StageLayout};
use retro_script::{CompileOptions, Compiler, SceneNames, ScriptFile, SymbolTables};

use crate::EngineError;
use crate::profile::EngineSettings;

/// Compiles the global and stage script groups and merges them into one runtime file.
pub struct LoadedScripts {
    /// Merged code/jump table/functions.
    pub file: ScriptFile,
    /// Object list with per-type event entry points.
    pub objects: ObjectRegistry,
    /// The single global group file, when globals were loaded.
    pub global: Option<ScriptFile>,
}

/// Everything needed to build the runtime engine state for one scene.
pub struct LoadedWorld {
    /// Resolved settings.
    pub settings: EngineSettings,
    /// Parsed `GameConfig.bin`.
    pub game_config: GameConfig,
    /// Stage folder (`Zone01`).
    pub stage_folder: String,
    /// Act id (`1`).
    pub act: String,
    /// Parsed act file.
    pub scene: Scene,
    /// Parsed stage config.
    pub stage_config: StageConfig,
    /// Collision context (`None` when the stage has no collision files).
    pub collision: Option<SceneCollision>,
    /// Background layers/parallax (`None` when absent).
    pub backgrounds: Option<Backgrounds>,
    /// Decoded `16x16Tiles.gif` (`None` when absent or malformed).
    pub tiles16: Option<TileSheet16>,
    /// Decoded `128x128Tiles.bin` (`None` when absent or malformed).
    pub tiles128: Option<TileSheet128>,
    /// Linked scripts.
    pub scripts: LoadedScripts,
}

/// Resolves `--scene`/`--act` to a stage folder and act id using `GameConfig`.
///
/// Resolution order, all case-insensitive:
/// 1. an exact stage folder match (`Zone01`),
/// 2. a GameConfig scene name match (`GREEN HILL ZONE 1`),
/// 3. without `--scene`, the first scene of the presentation category (normal boot flow up to
///    the title screen).
///
/// An explicit `act` always wins, including the literal `1` and stage ids such as `B`; when it
/// is absent the GameConfig entry's own id is used.
pub fn resolve_scene(
    game_config: &GameConfig,
    requested: Option<&str>,
    act: Option<&str>,
) -> Result<(String, String), EngineError> {
    let resolve_act = |entry_id: &str| {
        act.map(str::to_owned)
            .unwrap_or_else(|| entry_id.to_owned())
    };
    let Some(requested) = requested else {
        let entry = game_config
            .categories
            .first()
            .and_then(|category| category.scenes.first());
        return match entry {
            Some(entry) => Ok((entry.folder.clone(), resolve_act(&entry.id))),
            None => Err(EngineError::UnknownScene("<default>".to_owned())),
        };
    };
    let lowered = requested.to_ascii_lowercase();
    if let Some(entry) = game_config
        .categories
        .iter()
        .flat_map(|category| &category.scenes)
        .find(|entry| entry.folder.to_ascii_lowercase() == lowered)
    {
        return Ok((entry.folder.clone(), resolve_act(&entry.id)));
    }
    let normalized: String = requested
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(entry) = game_config
        .categories
        .iter()
        .flat_map(|category| &category.scenes)
        .find(|entry| {
            entry
                .name
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .to_ascii_lowercase()
                == normalized
        })
    {
        return Ok((entry.folder.clone(), resolve_act(&entry.id)));
    }
    Err(EngineError::UnknownScene(requested.to_owned()))
}

fn read_required(source: &dyn DataSource, path: &str) -> Result<Vec<u8>, EngineError> {
    source
        .read(path)
        .map_err(|_| EngineError::MissingAsset(path.to_owned()))
}

fn read_script(source: &dyn DataSource, script_path: &str) -> Result<String, EngineError> {
    let path = format!("Data/Scripts/{script_path}");
    let bytes = read_required(source, &path)?;
    String::from_utf8(bytes)
        .map_err(|error| EngineError::MissingAsset(format!("{path} is not valid UTF-8: {error}")))
}

fn symbol_tables(
    game_config: &GameConfig,
    stage_config: &StageConfig,
    load_globals: bool,
) -> SymbolTables {
    let global_object_types: Vec<String> = std::iter::once("BlankObject".to_owned())
        .chain(
            game_config
                .objects
                .iter()
                .map(|object| strip_spaces(&object.name)),
        )
        .collect();
    let global_sfx_names: Vec<String> = game_config
        .sound_effects
        .iter()
        .map(|sfx| strip_spaces(&sfx.name))
        .collect();
    let mut object_types = if load_globals {
        global_object_types
    } else {
        vec!["BlankObject".to_owned()]
    };
    object_types.extend(
        stage_config
            .objects
            .iter()
            .map(|object| strip_spaces(&object.name)),
    );
    let mut sfx_names = global_sfx_names;
    sfx_names.extend(
        stage_config
            .sound_effects
            .iter()
            .map(|sfx| strip_spaces(&sfx.name)),
    );
    let mut categories = game_config.categories.iter();
    let mut next = || {
        categories
            .next()
            .map(|category| {
                category
                    .scenes
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
    SymbolTables {
        global_variables: game_config
            .global_variables
            .iter()
            .map(|variable| variable.name.clone())
            .collect(),
        object_types,
        sfx_names,
        players: game_config.players.clone(),
        achievements: Vec::new(),
        scenes: SceneNames {
            presentation: next(),
            regular: next(),
            special: next(),
            bonus: next(),
        },
    }
}

/// Compiles the global and stage script groups exactly like `LoadStageFiles` links them.
pub fn load_scripts(
    source: &dyn DataSource,
    game_config: &GameConfig,
    stage_config: &StageConfig,
    settings: &EngineSettings,
) -> Result<LoadedScripts, EngineError> {
    let options = CompileOptions {
        platform: settings.platform,
        revision: settings.revision,
        base_code_pos: 0,
        base_jump_pos: 0,
        include_global_scripts: stage_config.load_global_objects,
        symbols: symbol_tables(game_config, stage_config, stage_config.load_global_objects),
        strict: false,
    };

    let mut objects = ObjectRegistry::new();

    if stage_config.load_global_objects {
        let mut global_compiler = Compiler::new(&options);
        for object in &game_config.objects {
            let text = read_script(source, &object.script_path)?;
            global_compiler.compile_file(Some(&object.script_path), &text)?;
        }
        let global = global_compiler.clone().finish();

        let mut stage_compiler = global_compiler;
        stage_compiler.set_symbols(symbol_tables(game_config, stage_config, true));
        let stage_mark = stage_compiler.mark();
        for object in &stage_config.objects {
            let text = read_script(source, &object.script_path)?;
            stage_compiler.compile_file(Some(&object.script_path), &text)?;
        }
        let stage = stage_compiler.finish_group(stage_mark);

        for (index, object) in game_config.objects.iter().enumerate() {
            let script = global
                .object_scripts
                .get(index)
                .copied()
                .unwrap_or_default();
            objects.push(&object.name, stage.values.clone(), script);
        }
        for (index, object) in stage_config.objects.iter().enumerate() {
            let script = stage.object_scripts.get(index).copied().unwrap_or_default();
            objects.push(&object.name, stage.values.clone(), script);
        }

        let mut file = ScriptFile {
            functions: stage.functions.clone(),
            global_variables: stage.global_variables.clone(),
            values: stage.values.clone(),
            ..ScriptFile::default()
        };
        file.code.extend_from_slice(&global.code);
        file.code.extend_from_slice(&stage.code);
        file.jump_table.extend_from_slice(&global.jump_table);
        file.jump_table.extend_from_slice(&stage.jump_table);
        Ok(LoadedScripts {
            file,
            objects,
            global: Some(global),
        })
    } else {
        let mut compiler = Compiler::new(&options);
        let mark = compiler.mark();
        for object in &stage_config.objects {
            let text = read_script(source, &object.script_path)?;
            compiler.compile_file(Some(&object.script_path), &text)?;
        }
        let stage = compiler.finish_group(mark);
        for (index, object) in stage_config.objects.iter().enumerate() {
            let script = stage.object_scripts.get(index).copied().unwrap_or_default();
            objects.push(&object.name, stage.values.clone(), script);
        }
        Ok(LoadedScripts {
            file: stage,
            objects,
            global: None,
        })
    }
}

/// Loads settings, configs, scene data and scripts for the requested scene.
pub fn load_world(
    source: &Arc<dyn DataSource>,
    requested_scene: Option<&str>,
    act: Option<&str>,
) -> Result<LoadedWorld, EngineError> {
    let detected = detect(source.as_ref())?;
    if detected.version != DataVersion::V4Legacy {
        return Err(EngineError::UnsupportedVersion(detected.version));
    }
    let settings = EngineSettings::from_settings(&detected.settings);
    let game_config = detected.game_config;

    let (folder, act) = resolve_scene(&game_config, requested_scene, act)?;
    let stage_dir = format!("Data/Stages/{folder}");
    let stage_config = StageConfig::load(&stage_dir, source.as_ref())?;
    let scene = Scene::load(&stage_dir, &act, source.as_ref())?;

    let tiles128 = TileSheet128::load(&stage_dir, source.as_ref()).ok();
    let collision = match (
        tiles128.clone(),
        CollisionMasks::load(&stage_dir, source.as_ref()),
    ) {
        (Some(tiles), Ok(masks)) => {
            let layout = StageLayout::from_scene(&scene);
            Some(SceneCollision::new(layout, tiles, masks))
        }
        _ => None,
    };
    let tiles16 = TileSheet16::load(&stage_dir, source.as_ref()).ok();
    let backgrounds = if source.exists(&format!("{stage_dir}/Backgrounds.bin")) {
        Backgrounds::load(&stage_dir, source.as_ref()).ok()
    } else {
        None
    };
    // Achievements are referenced by some scripts; load them when present.
    let _achievements = Achievements::load(source.as_ref()).ok();

    let scripts = load_scripts(source.as_ref(), &game_config, &stage_config, &settings)?;
    Ok(LoadedWorld {
        settings,
        game_config,
        stage_folder: folder,
        act,
        scene,
        stage_config,
        collision,
        backgrounds,
        tiles16,
        tiles128,
        scripts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_format_v4::gameconfig::PALETTE_COUNT;
    use retro_format_v4::{ObjectInfo, SceneCategory, SceneEntry, SoundEffect, gameconfig};

    fn config() -> GameConfig {
        GameConfig {
            title: "Test".to_owned(),
            subtitle: String::new(),
            palette: vec![[0, 0, 0]; PALETTE_COUNT],
            objects: Vec::new(),
            global_variables: Vec::new(),
            sound_effects: Vec::new(),
            players: Vec::new(),
            categories: gameconfig::CATEGORY_NAMES
                .iter()
                .map(|name| SceneCategory {
                    name: (*name).to_owned(),
                    scenes: Vec::new(),
                })
                .collect(),
        }
    }

    fn push_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
    }

    fn game_config_bytes_with_scene(folder: &str, id: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_string(&mut bytes, "Test");
        push_string(&mut bytes, "");
        for _ in 0..PALETTE_COUNT {
            bytes.extend_from_slice(&[0, 0, 0]);
        }
        bytes.push(0); // objects
        bytes.push(0); // variables
        bytes.push(0); // sfx
        bytes.push(0); // players
        // Presentation has the scene, the other categories are empty.
        bytes.push(1);
        push_string(&mut bytes, folder);
        push_string(&mut bytes, id);
        push_string(&mut bytes, "TEST");
        bytes.push(1);
        bytes.extend(std::iter::repeat_n(0u8, 3));
        bytes
    }

    fn stage_config_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(0); // load_global_objects
        for _ in 0..retro_format_v4::stageconfig::STAGE_PALETTE_COUNT {
            bytes.extend_from_slice(&[0, 0, 0]);
        }
        bytes.push(0); // sfx
        bytes.push(0); // objects
        bytes
    }

    fn scene_bytes(title: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_string(&mut bytes, title);
        bytes.extend_from_slice(&[9; 4]);
        bytes.push(3);
        bytes.push(1);
        bytes.push(0);
        bytes.push(1);
        bytes.push(0);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    #[test]
    fn scene_resolution_prefers_folder_then_name() {
        let mut config = config();
        config.categories[0].scenes.push(SceneEntry {
            folder: "Title".to_owned(),
            id: "1".to_owned(),
            name: "TITLE SCREEN".to_owned(),
            highlighted: 1,
        });
        config.categories[1].scenes.push(SceneEntry {
            folder: "Zone01".to_owned(),
            id: "2".to_owned(),
            name: "GREEN HILL ZONE 1".to_owned(),
            highlighted: 1,
        });
        assert_eq!(
            resolve_scene(&config, None, None).unwrap(),
            ("Title".to_owned(), "1".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("zone01"), None).unwrap(),
            ("Zone01".to_owned(), "2".to_owned()),
            "folder match uses the GameConfig id when act is absent"
        );
        assert_eq!(
            resolve_scene(&config, Some("green hill zone 1"), None).unwrap(),
            ("Zone01".to_owned(), "2".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("Zone01"), Some("3")).unwrap(),
            ("Zone01".to_owned(), "3".to_owned()),
            "explicit act overrides the config id"
        );
        assert_eq!(
            resolve_scene(&config, Some("Zone01"), Some("1")).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "explicit --act 1 must beat a config id of 2"
        );
        assert_eq!(
            resolve_scene(&config, Some("Zone01"), Some("B")).unwrap(),
            ("Zone01".to_owned(), "B".to_owned()),
            "literal act ids are preserved"
        );
        assert!(matches!(
            resolve_scene(&config, Some("Missing"), None),
            Err(EngineError::UnknownScene(_))
        ));
    }

    #[test]
    fn group_linking_offsets_stage_positions_by_global_code() {
        use retro_io::MemorySource;

        let global_source = "event ObjectUpdate\nobject.value0 = 1\nend event\n";
        let stage_source = "event ObjectUpdate\nobject.value0 = 2\nend event\n";
        let mut source = MemorySource::new();
        source.insert("Data/Scripts/G.txt", global_source);
        source.insert("Data/Scripts/S.txt", stage_source);

        let mut config = config();
        config.objects.push(ObjectInfo {
            name: "Global Object".to_owned(),
            script_path: "G.txt".to_owned(),
        });
        let mut stage = StageConfig {
            load_global_objects: true,
            palette: vec![[0, 0, 0]; retro_format_v4::stageconfig::STAGE_PALETTE_COUNT],
            sound_effects: Vec::new(),
            objects: vec![ObjectInfo {
                name: "Stage Object".to_owned(),
                script_path: "S.txt".to_owned(),
            }],
        };
        let settings = EngineSettings {
            profile: crate::RuntimeProfile::V4Legacy,
            platform: retro_script::PlatformMode::Origins,
            revision: retro_script::V4Revision::Rev03,
            force_scripts: false,
        };

        let linked = load_scripts(&source, &config, &stage, &settings).unwrap();
        let global = linked.global.as_ref().expect("global group");
        let global_len = global.code.len();
        assert!(global_len > 0);
        assert_eq!(linked.objects.get(1).unwrap().name, "GlobalObject");
        assert_eq!(linked.objects.get(2).unwrap().name, "StageObject");

        // The stage's object script positions are absolute in the merged (global + stage)
        // space, so the stage code starts right after the global code.
        let stage_ptr = linked.objects.get(2).unwrap().script.update.code_pos as usize;
        assert!(
            stage_ptr >= global_len,
            "stage position {stage_ptr} must follow global code {global_len}"
        );
        assert!(linked.file.code.len() > global_len);
        assert!(stage_ptr < linked.file.code.len());
        assert!(linked.file.code.get(stage_ptr).copied().unwrap_or(0) > 0);
        assert!(linked.file.code.get(global_len).copied().unwrap_or(0) > 0);

        // Without globals the stage group starts at position 0.
        stage.load_global_objects = false;
        let standalone = load_scripts(&source, &config, &stage, &settings).unwrap();
        assert!(standalone.global.is_none());
        assert_eq!(standalone.objects.len(), 2);
        assert_eq!(standalone.objects.get(1).unwrap().script.update.code_pos, 0);
    }

    #[test]
    fn explicit_act_one_loads_act_one_despite_config_id_two() {
        use retro_io::MemorySource;

        let mut source = MemorySource::new();
        source.insert("Settings.ini", "[Game]\ngameType=1\n");
        source.insert(
            "Data/Game/GameConfig.bin",
            game_config_bytes_with_scene("Zone01", "2"),
        );
        source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
        source.insert("Data/Stages/Zone01/Act1.bin", scene_bytes("FIRST"));
        source.insert("Data/Stages/Zone01/Act2.bin", scene_bytes("SECOND"));
        source.insert("Data/Stages/Zone01/ActB.bin", scene_bytes("BONUS"));
        let source: Arc<dyn retro_io::DataSource> = Arc::new(source);

        // No explicit act: the GameConfig entry id (2) selects Act2.
        let world = load_world(&source, Some("Zone01"), None).unwrap();
        assert_eq!(world.act, "2");
        assert_eq!(world.scene.title, "SECOND");

        // Explicit `--act 1` must select Act1 even though the entry id is "2".
        let world = load_world(&source, Some("Zone01"), Some("1")).unwrap();
        assert_eq!(world.act, "1");
        assert_eq!(world.scene.title, "FIRST");

        // The literal stage id `B` is accepted as-is.
        let world = load_world(&source, Some("Zone01"), Some("B")).unwrap();
        assert_eq!(world.act, "B");
        assert_eq!(world.scene.title, "BONUS");
    }

    #[test]
    fn symbol_tables_include_globals_only_when_loaded() {
        let mut config = config();
        config.objects.push(ObjectInfo {
            name: "Player Object".to_owned(),
            script_path: "Players/PlayerObject.txt".to_owned(),
        });
        config.sound_effects.push(SoundEffect {
            name: "Ring".to_owned(),
            path: "Global/Ring.wav".to_owned(),
        });
        config.global_variables.push(gameconfig::GlobalVariable {
            name: "lives".to_owned(),
            value: 3,
        });
        let stage = StageConfig {
            load_global_objects: true,
            palette: vec![[0, 0, 0]; retro_format_v4::stageconfig::STAGE_PALETTE_COUNT],
            sound_effects: vec![SoundEffect {
                name: "Spring".to_owned(),
                path: "Global/Spring.wav".to_owned(),
            }],
            objects: vec![ObjectInfo {
                name: "Bridge".to_owned(),
                script_path: "GHZ/Bridge.txt".to_owned(),
            }],
        };
        let with_globals = symbol_tables(&config, &stage, true);
        assert_eq!(
            with_globals.object_types,
            vec!["BlankObject", "PlayerObject", "Bridge"]
        );
        assert_eq!(with_globals.sfx_names, vec!["Ring", "Spring"]);
        assert_eq!(with_globals.global_variables, vec!["lives"]);

        let without = symbol_tables(&config, &stage, false);
        assert_eq!(without.object_types, vec!["BlankObject", "Bridge"]);
    }
}
