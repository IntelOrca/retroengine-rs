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
    Achievements, Backgrounds, CollisionMasks, GameConfig, Scene, Settings, StageConfig,
    TileSheet16, TileSheet128,
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
    /// Parsed `Settings.ini`, kept for the input mappings and audio volumes.
    pub raw_settings: Settings,
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

/// Stage assets for one act, independent of settings/`GameConfig` detection.
///
/// This is the `LoadStageFiles` body after `stageList[activeStageList][stageListPosition]` has
/// been resolved: deferred `LoadStage` requests reuse it with a folder/act pair taken directly
/// from the stage list entry.
pub struct SceneAssets {
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

/// Loads the stage config, act, tiles, collision, backgrounds and scripts for `folder`/`act`.
///
/// The folder and act are used verbatim (the GameConfig entry's own values), matching upstream's
/// `stageList[activeStageList][stageListPosition].folder`/`.id`.
pub fn load_scene_assets(
    source: &Arc<dyn DataSource>,
    game_config: &GameConfig,
    settings: &EngineSettings,
    folder: &str,
    act: &str,
) -> Result<SceneAssets, EngineError> {
    let stage_dir = format!("Data/Stages/{folder}");
    if !source.exists(&Scene::path(&stage_dir, act)) {
        let available = available_acts(source.as_ref(), &stage_dir);
        return Err(EngineError::MissingAct {
            folder: folder.to_owned(),
            act: act.to_owned(),
            available: if available.is_empty() {
                "<none>".to_owned()
            } else {
                available.join(", ")
            },
        });
    }
    let stage_config = StageConfig::load(&stage_dir, source.as_ref())?;
    let scene = Scene::load(&stage_dir, act, source.as_ref())?;

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

    let scripts = load_scripts(source.as_ref(), game_config, &stage_config, settings)?;
    Ok(SceneAssets {
        scene,
        stage_config,
        collision,
        backgrounds,
        tiles16,
        tiles128,
        scripts,
    })
}

/// A GameConfig scene paired with its category (file order) and global 1-based `--list` index.
#[derive(Clone, Copy, Debug)]
struct IndexedScene<'a> {
    index: usize,
    category: usize,
    entry: &'a retro_format_v4::SceneEntry,
}

/// Lower-cases and drops everything that is not an ASCII letter or digit.
fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Splits trailing ASCII digits off a normalized name (`greenhillzone1` -> `greenhillzone`+`1`).
fn split_trailing_digits(value: &str) -> (&str, Option<&str>) {
    let split = value
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_digit())
        .map_or(0, |(index, character)| index + character.len_utf8());
    if split == value.len()
        || split == 0 && value.chars().all(|character| character.is_ascii_digit())
    {
        (&value[..split], None)
    } else {
        (&value[..split], Some(&value[split..]))
    }
}

/// Word-initial acronym of a scene name (`GREEN HILL ZONE 1` -> `ghz1`).
fn acronym(value: &str) -> String {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .filter_map(|word| word.chars().next())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Length of the longest common prefix of `a` and `b`.
fn common_prefix(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

/// Normalizes an act id: numeric ids lose leading zeros, short ids (`b`) become upper case.
fn normalize_act(value: &str) -> Result<String, EngineError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(EngineError::InvalidAct(value.to_owned()));
    }
    if trimmed.chars().all(|character| character.is_ascii_digit()) {
        let number = trimmed
            .parse::<u32>()
            .map_err(|_| EngineError::InvalidAct(value.to_owned()))?;
        Ok(number.to_string())
    } else {
        Ok(trimmed.to_ascii_uppercase())
    }
}

/// The canonical "first zone" shorthand that also resolves on trees without a Green Hill.
///
/// `--scene GHZ` matches `GREEN HILL ZONE 1` by name on Sonic 1. Sonic 2's first regular zone
/// is Emerald Hill, so when no name/acronym match exists the shorthand falls back to the first
/// Regular-category scene, keeping `retroengine <S1|S2> --scene GHZ` working for both.
const FIRST_ZONE_ALIASES: &[&str] = &["ghz"];

/// Resolves `--scene`/`--act` to a stage folder and act id using `GameConfig`.
///
/// `--scene` accepts, case-insensitively and ignoring spaces/punctuation:
/// 1. a numeric scene index from `--list` (`7`, 1-based, category-major),
/// 2. a stage folder (`Zone01`, `Zone-01`), where duplicate folders use `--act` to pick an
///    entry id and otherwise resolve to the highlighted entry with the lowest id,
/// 3. an exact scene name (`GREEN HILL ZONE 1`),
/// 4. a name without the act number (`GREEN HILL ZONE`, `MarbleZone2` selects act 2),
/// 5. a name prefix (`GreenHill`),
/// 6. a word-initial acronym with or without the act number (`GHZ`, `GHZ1`).
///
/// `GHZ` additionally resolves to the first Regular-category scene when the loaded game has no
/// Green Hill (Sonic 2's Emerald Hill). Without `--scene` the first scene of the first category
/// is used (normal boot flow up to the title screen).
///
/// An explicit `--act` always wins, including the literal `1` and stage ids such as `B`; a
/// trailing act number in `--scene` is used next; otherwise the GameConfig entry's own id is
/// used.
pub fn resolve_scene(
    game_config: &GameConfig,
    requested: Option<&str>,
    act: Option<&str>,
) -> Result<(String, String), EngineError> {
    let mut entries = Vec::new();
    for (category, data) in game_config.categories.iter().enumerate() {
        for entry in &data.scenes {
            entries.push(IndexedScene {
                index: entries.len() + 1,
                category,
                entry,
            });
        }
    }
    let resolve = |scene: IndexedScene<'_>, act_override: Option<&str>| {
        let act = match (act, act_override) {
            (Some(value), _) | (None, Some(value)) => normalize_act(value)?,
            (None, None) => normalize_act(&scene.entry.id)?,
        };
        Ok((scene.entry.folder.clone(), act))
    };

    let Some(requested) = requested else {
        return match entries.first() {
            Some(scene) => resolve(*scene, None),
            None => Err(unknown_scene(
                "<default>",
                "the GameConfig has no scenes",
                &entries,
            )),
        };
    };
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(unknown_scene(requested, "the value is empty", &entries));
    }

    // 1. Numeric `--list` index.
    if trimmed.chars().all(|character| character.is_ascii_digit()) {
        return match trimmed
            .parse::<usize>()
            .ok()
            .filter(|index| *index >= 1 && *index <= entries.len())
        {
            Some(index) => resolve(entries[index - 1], None),
            None => Err(unknown_scene(
                requested,
                &format!("index is out of range (1..={})", entries.len()),
                &entries,
            )),
        };
    }

    let request_key = normalize_name(trimmed);

    // A key with no letters or only digits (`+7`, `!!!`) is a mistyped `--list` index, not a
    // scene name; reject it instead of matching a digit-named entry by accident.
    if request_key.is_empty()
        || request_key
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(unknown_scene(
            requested,
            "not a scene name (numbers select a `--list` index and must not carry punctuation)",
            &entries,
        ));
    }

    let (request_base, request_act) = split_trailing_digits(&request_key);

    // 2. Exact stage folder, ignoring case, spaces and punctuation. Duplicate folders
    //    (per-act entries or presentation variants such as `LSelect`) are disambiguated by
    //    `--act`; otherwise the best-ranked entry wins: highlighted entries first, then the
    //    lowest numeric id, then short ids alphabetically, then file order.
    let folders: Vec<IndexedScene<'_>> = entries
        .iter()
        .filter(|scene| normalize_name(&scene.entry.folder) == request_key)
        .copied()
        .collect();
    if !folders.is_empty() {
        if let Some(chosen) = act.and_then(|value| {
            folders
                .iter()
                .find(|scene| scene.entry.id.eq_ignore_ascii_case(value.trim()))
        }) {
            return resolve(*chosen, None);
        }
        let chosen = folders
            .iter()
            .min_by_key(|scene| folder_rank(scene))
            .unwrap_or(&folders[0]);
        return resolve(*chosen, None);
    }

    // 3. Exact scene name.
    let exact: Vec<IndexedScene<'_>> = entries
        .iter()
        .filter(|scene| normalize_name(&scene.entry.name) == request_key)
        .copied()
        .collect();
    match exact.as_slice() {
        [scene] => return resolve(*scene, None),
        [] => {}
        _ => return Err(unknown_scene(requested, &ambiguous(&exact), &entries)),
    }

    // 4. Scene name without its act number, optionally taking the act number from the request.
    let actless: Vec<IndexedScene<'_>> = entries
        .iter()
        .filter(|scene| {
            !request_base.is_empty()
                && split_trailing_digits(&normalize_name(&scene.entry.name)).0 == request_base
        })
        .copied()
        .collect();
    match actless.as_slice() {
        [scene] => return resolve(*scene, request_act),
        [] => {}
        _ => return Err(unknown_scene(requested, &ambiguous(&actless), &entries)),
    }

    // 5. Name prefix (`GreenHill`).
    if request_base.len() >= 3 {
        let prefixed: Vec<IndexedScene<'_>> = entries
            .iter()
            .filter(|scene| {
                split_trailing_digits(&normalize_name(&scene.entry.name))
                    .0
                    .starts_with(request_base)
            })
            .copied()
            .collect();
        match prefixed.as_slice() {
            [scene] => return resolve(*scene, request_act),
            [] => {}
            _ => return Err(unknown_scene(requested, &ambiguous(&prefixed), &entries)),
        }
    }

    // 6. Word-initial acronym (`GHZ`, `GHZ1`).
    let acronyms: Vec<IndexedScene<'_>> = entries
        .iter()
        .filter(|scene| {
            let name_key = normalize_name(&scene.entry.name);
            let name_acronym = acronym(&scene.entry.name);
            let name_acronym_base = split_trailing_digits(&name_acronym).0;
            request_key == name_acronym
                || request_key == name_acronym_base
                || request_base == name_acronym_base
                || name_key == request_key
        })
        .copied()
        .collect();
    match acronyms.as_slice() {
        [scene] => return resolve(*scene, request_act),
        [] => {}
        _ => return Err(unknown_scene(requested, &ambiguous(&acronyms), &entries)),
    }

    // 7. First-zone shorthand (`GHZ` on a tree whose first zone is Emerald Hill).
    if FIRST_ZONE_ALIASES.contains(&request_base) {
        let first_regular = entries.iter().find(|scene| scene.category == 1);
        if let Some(scene) = first_regular {
            return resolve(*scene, request_act);
        }
    }

    Err(unknown_scene(
        requested,
        &candidate_hint(&entries, request_base),
        &entries,
    ))
}

/// Actionable detail string for an unknown scene.
fn unknown_scene(requested: &str, detail: &str, entries: &[IndexedScene<'_>]) -> EngineError {
    let scenes = entries.len();
    EngineError::UnknownScene {
        requested: requested.to_owned(),
        details: format!(
            "{detail} (this GameConfig has {scenes} scene(s); run `--list` to see them)"
        ),
    }
}

/// Formats an ambiguity list (`#7 Zone01/GREEN HILL ZONE 1 (id 1), ...`).
fn ambiguous(matches: &[IndexedScene<'_>]) -> String {
    let labels: Vec<String> = matches.iter().map(scene_label).collect();
    format!("ambiguous, matches {}", labels.join(", "))
}

/// Formats one scene as `#7 Zone01/GREEN HILL ZONE 1 (id 1)`.
fn scene_label(scene: &IndexedScene<'_>) -> String {
    format!(
        "#{} {}/{} (id {})",
        scene.index, scene.entry.folder, scene.entry.name, scene.entry.id
    )
}

/// "did you mean ..." hint for a failed resolution.
fn candidate_hint(entries: &[IndexedScene<'_>], request_base: &str) -> String {
    let mut scored: Vec<(usize, IndexedScene<'_>)> = entries
        .iter()
        .filter_map(|scene| {
            let name = normalize_name(&scene.entry.name);
            let name_key = split_trailing_digits(&name).0;
            let name_acronym = acronym(&scene.entry.name);
            let score = common_prefix(request_base, name_key)
                .max(common_prefix(request_base, &name_acronym));
            (score >= 2).then_some((score, *scene))
        })
        .collect();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then(left.index.cmp(&right.index))
    });
    let mut candidates: Vec<IndexedScene<'_>> =
        scored.into_iter().map(|(_, scene)| scene).collect();
    if candidates.is_empty() {
        candidates = entries
            .iter()
            .filter(|scene| scene.entry.highlighted != 0)
            .copied()
            .collect();
    }
    if candidates.is_empty() {
        candidates = entries.to_vec();
    }
    candidates.truncate(5);
    let labels: Vec<String> = candidates.iter().map(scene_label).collect();
    format!("did you mean {}?", labels.join(", "))
}

/// Returns the `Act<id>.bin` ids directly under `stage_dir`, case-insensitively.
///
/// The ids keep their on-disk spelling, numeric ids sort first in numeric order, and a missing
/// or unreadable directory yields an empty list.
#[must_use]
pub fn available_acts(source: &dyn DataSource, stage_dir: &str) -> Vec<String> {
    let mut acts = Vec::new();
    if let Ok(files) = source.enumerate(stage_dir) {
        for file in files {
            let Some(name) = file.rsplit('/').next() else {
                continue;
            };
            if name.len() < 8 {
                continue;
            }
            let (Some(prefix), Some(suffix)) = (name.get(..3), name.get(name.len() - 4..)) else {
                continue;
            };
            if !prefix.eq_ignore_ascii_case("Act") || !suffix.eq_ignore_ascii_case(".bin") {
                continue;
            }
            let Some(id) = name.get(3..name.len() - 4) else {
                continue;
            };
            if id
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
            {
                acts.push(id.to_owned());
            }
        }
    }
    acts.sort_by_key(|act| act_sort_key(act));
    acts.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    acts
}

/// Sort key that orders numeric acts before short ids.
fn act_sort_key(act: &str) -> (bool, u64, String) {
    match act.parse::<u64>() {
        Ok(number) => (false, number, String::new()),
        Err(_) => (true, 0, act.to_ascii_uppercase()),
    }
}

/// Ranking key used to pick one entry when several GameConfig entries share a stage folder:
/// highlighted entries first, then numeric ids ascending, then short ids alphabetically, then
/// file order.
fn folder_rank(scene: &IndexedScene<'_>) -> (bool, bool, u64, String, usize) {
    let (non_numeric, number, text) = act_sort_key(&scene.entry.id);
    (
        scene.entry.highlighted == 0,
        non_numeric,
        number,
        text,
        scene.index,
    )
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

/// Options that affect how one world is loaded and compiled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadOptions {
    /// Compile scripts with the Origins (`USE_ORIGINS`) platform tag instead of the default
    /// standalone (`USE_STANDALONE`) platform (the CLI's `--origins`).
    pub origins: bool,
}

/// Locates a stage folder/act pair in the engine stage lists, returning
/// `(engine_list, list_pos, list_size)`.
///
/// `engine_list` is the `stage.activeList` value (`STAGELIST_*` order, not the GameConfig
/// file order); `list_size` is the number of entries in that list. `None` when no entry matches.
#[must_use]
pub fn engine_list_position(
    game_config: &GameConfig,
    folder: &str,
    act: &str,
) -> Option<(i32, i32, i32)> {
    for engine_index in 0..4usize {
        let Some(category) = game_config.category_for_engine_index(engine_index) else {
            continue;
        };
        for (position, entry) in category.scenes.iter().enumerate() {
            if entry.folder == folder && entry.id == act {
                return Some((
                    engine_index as i32,
                    position as i32,
                    category.scenes.len() as i32,
                ));
            }
        }
    }
    None
}

/// Resolves a stage list request the way `LoadStage` consumes it: `stage.activeList` selects the
/// engine stage list and `stage.listPos` the entry inside it.
///
/// Returns the entry's folder and id or [`EngineError::InvalidStageList`].
pub fn stage_list_entry(
    game_config: &GameConfig,
    active_list: i32,
    list_pos: i32,
) -> Result<(&retro_format_v4::SceneEntry, i32), EngineError> {
    let invalid = || EngineError::InvalidStageList {
        list: active_list,
        pos: list_pos,
    };
    let category = usize::try_from(active_list)
        .ok()
        .and_then(|index| game_config.category_for_engine_index(index))
        .ok_or_else(invalid)?;
    let position = usize::try_from(list_pos).map_err(|_| invalid())?;
    let entry = category.scenes.get(position).ok_or_else(invalid)?;
    Ok((entry, category.scenes.len() as i32))
}

/// Loads settings, configs, scene data and scripts for the requested scene.
pub fn load_world(
    source: &Arc<dyn DataSource>,
    requested_scene: Option<&str>,
    act: Option<&str>,
) -> Result<LoadedWorld, EngineError> {
    load_world_with(source, requested_scene, act, LoadOptions::default())
}

/// Loads settings, configs, scene data and scripts with explicit [`LoadOptions`].
pub fn load_world_with(
    source: &Arc<dyn DataSource>,
    requested_scene: Option<&str>,
    act: Option<&str>,
    options: LoadOptions,
) -> Result<LoadedWorld, EngineError> {
    if !source.exists(Settings::PATH) {
        return Err(EngineError::MissingAsset(
            "Settings.ini (keyboard controls and video/audio settings)".to_owned(),
        ));
    }
    let detected = detect(source.as_ref())?;
    if detected.version != DataVersion::V4Legacy {
        return Err(EngineError::UnsupportedVersion(detected.version));
    }
    let settings = EngineSettings::from_settings_with(&detected.settings, options.origins);
    let game_config = detected.game_config;

    let (folder, act) = resolve_scene(&game_config, requested_scene, act)?;
    let assets = load_scene_assets(source, &game_config, &settings, &folder, &act)?;
    Ok(LoadedWorld {
        settings,
        raw_settings: detected.settings,
        game_config,
        stage_folder: folder,
        act,
        scene: assets.scene,
        stage_config: assets.stage_config,
        collision: assets.collision,
        backgrounds: assets.backgrounds,
        tiles16: assets.tiles16,
        tiles128: assets.tiles128,
        scripts: assets.scripts,
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

    fn scene_entry(folder: &str, id: &str, name: &str) -> SceneEntry {
        SceneEntry {
            folder: folder.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            highlighted: 1,
        }
    }

    fn sonic1_like_config() -> GameConfig {
        let mut config = config();
        config.categories[0]
            .scenes
            .push(scene_entry("Title", "1", "TITLE SCREEN"));
        config.categories[1].scenes = vec![
            scene_entry("Zone01", "1", "GREEN HILL ZONE 1"),
            scene_entry("Zone01", "2", "2"),
            scene_entry("Zone01", "3", "3"),
            scene_entry("Zone02", "1", "MARBLE ZONE 1"),
            scene_entry("Zone02", "2", "2"),
        ];
        config.categories[2].scenes = vec![
            scene_entry("Special", "1", "SPECIAL STAGE 1"),
            scene_entry("Special", "2", "SPECIAL STAGE 2"),
        ];
        config
    }

    fn sonic2_like_config() -> GameConfig {
        let mut config = config();
        config.categories[0]
            .scenes
            .push(scene_entry("Title", "1", "TITLE SCREEN"));
        config.categories[1].scenes = vec![
            scene_entry("Zone01", "1", "EMERALD HILL ZONE 1"),
            scene_entry("Zone01", "2", "2"),
            scene_entry("Zone02", "1", "CHEMICAL PLANT ZONE 1"),
        ];
        config
    }

    #[test]
    fn scene_resolution_prefers_folder_then_name() {
        let config = sonic1_like_config();
        assert_eq!(
            resolve_scene(&config, None, None).unwrap(),
            ("Title".to_owned(), "1".to_owned()),
            "the default is the first scene of the first category"
        );
        assert_eq!(
            resolve_scene(&config, Some("zone01"), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "folder match uses the GameConfig id when act is absent"
        );
        assert_eq!(
            resolve_scene(&config, Some("green hill zone 1"), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned())
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
    }

    #[test]
    fn scene_resolution_accepts_short_names_and_punctuation() {
        let config = sonic1_like_config();
        for request in [
            "GHZ",
            "ghz",
            "GhZ1",
            "GHZ1",
            "GreenHill",
            "green hill",
            "GREEN HILL ZONE",
            "GREEN-HILL-ZONE-1!",
            "  Green Hill Zone 1  ",
        ] {
            assert_eq!(
                resolve_scene(&config, Some(request), None).unwrap(),
                ("Zone01".to_owned(), "1".to_owned()),
                "--scene {request:?}"
            );
        }
        assert_eq!(
            resolve_scene(&config, Some("GreenHill2"), None).unwrap(),
            ("Zone01".to_owned(), "2".to_owned()),
            "a trailing act number selects the act"
        );
        assert_eq!(
            resolve_scene(&config, Some("GREEN HILL ZONE 3"), None).unwrap(),
            ("Zone01".to_owned(), "3".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("marble"), None).unwrap(),
            ("Zone02".to_owned(), "1".to_owned()),
            "a name prefix is enough"
        );
        assert_eq!(
            resolve_scene(&config, Some("MZ"), None).unwrap(),
            ("Zone02".to_owned(), "1".to_owned()),
            "an acronym is enough"
        );
        assert_eq!(
            resolve_scene(&config, Some("GHZ"), Some("2")).unwrap(),
            ("Zone01".to_owned(), "2".to_owned()),
            "an explicit --act beats the request's own act number"
        );
    }

    #[test]
    fn scene_resolution_accepts_list_indexes() {
        let config = sonic1_like_config();
        assert_eq!(
            resolve_scene(&config, Some("1"), None).unwrap(),
            ("Title".to_owned(), "1".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some(" 2 "), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("6"), None).unwrap(),
            ("Zone02".to_owned(), "2".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("6"), Some("1")).unwrap(),
            ("Zone02".to_owned(), "1".to_owned())
        );
        let error = resolve_scene(&config, Some("9"), None).unwrap_err();
        let EngineError::UnknownScene { requested, details } = error else {
            panic!("expected UnknownScene, got {error:?}");
        };
        assert_eq!(requested, "9");
        assert!(details.contains("1..=8"), "{details}");
        assert!(details.contains("--list"), "{details}");
        assert_eq!(
            details.matches("--list").count(),
            1,
            "the --list hint must not be duplicated: {details}"
        );
    }

    #[test]
    fn folder_resolution_ignores_spaces_and_punctuation() {
        let config = sonic1_like_config();
        for request in ["Zone 01", "Zone-01", "zone_01", "zOnE.01", " ZONE 01 "] {
            assert_eq!(
                resolve_scene(&config, Some(request), None).unwrap(),
                ("Zone01".to_owned(), "1".to_owned()),
                "--scene {request:?}"
            );
        }
    }

    #[test]
    fn duplicate_folders_use_the_act_then_the_best_ranked_entry() {
        let mut config = config();
        config.categories[0].scenes = vec![
            SceneEntry {
                folder: "LSelect".to_owned(),
                id: "1".to_owned(),
                name: "LEVEL SELECT".to_owned(),
                highlighted: 1,
            },
            SceneEntry {
                folder: "LSelect".to_owned(),
                id: "2".to_owned(),
                name: "2P VS".to_owned(),
                highlighted: 0,
            },
            SceneEntry {
                folder: "LSelect".to_owned(),
                id: "3".to_owned(),
                name: "STAGE MENU".to_owned(),
                highlighted: 1,
            },
        ];
        assert_eq!(
            resolve_scene(&config, Some("lselect"), None).unwrap(),
            ("LSelect".to_owned(), "1".to_owned()),
            "among highlighted entries the lowest id wins"
        );
        assert_eq!(
            resolve_scene(&config, Some("L Select"), None).unwrap(),
            ("LSelect".to_owned(), "1".to_owned()),
            "spaces are ignored"
        );
        assert_eq!(
            resolve_scene(&config, Some("LSelect"), Some("2")).unwrap(),
            ("LSelect".to_owned(), "2".to_owned()),
            "an explicit --act picks the entry with that id"
        );
        assert_eq!(
            resolve_scene(&config, Some("LSelect"), Some("3")).unwrap(),
            ("LSelect".to_owned(), "3".to_owned())
        );
        // Highlighting beats a lower id, and with no highlighted entry the lowest id wins
        // (Sonic 2 lists LSelect's `2P VS` (id 2) before `LEVEL SELECT` (id 1)).
        let mut reordered = config.clone();
        reordered.categories[0].scenes = vec![
            SceneEntry {
                folder: "LSelect".to_owned(),
                id: "2".to_owned(),
                name: "2P VS".to_owned(),
                highlighted: 0,
            },
            SceneEntry {
                folder: "LSelect".to_owned(),
                id: "1".to_owned(),
                name: "LEVEL SELECT".to_owned(),
                highlighted: 1,
            },
        ];
        assert_eq!(
            resolve_scene(&reordered, Some("LSelect"), None).unwrap(),
            ("LSelect".to_owned(), "1".to_owned()),
            "a highlighted higher id beats an unhighlighted lower id"
        );
        for scene in &mut reordered.categories[0].scenes {
            scene.highlighted = 0;
        }
        assert_eq!(
            resolve_scene(&reordered, Some("LSelect"), None).unwrap(),
            ("LSelect".to_owned(), "1".to_owned()),
            "with no highlighted entry the lowest numeric id wins"
        );
    }

    #[test]
    fn digit_only_requests_never_match_scene_names() {
        let sonic1 = sonic1_like_config();
        assert_eq!(
            resolve_scene(&sonic1, Some("2"), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "plain digits still select the --list index"
        );
        for request in ["+2", "-2", "2!", "#3", "!!!"] {
            let error = resolve_scene(&sonic1, Some(request), None).unwrap_err();
            let EngineError::UnknownScene { details, .. } = error else {
                panic!("expected UnknownScene for {request:?}, got {error:?}");
            };
            assert!(details.contains("--list"), "{request:?}: {details}");
        }

        // Even a scene literally named `7` must not be booted by `+7`.
        let mut named = config();
        named.categories[0].scenes = vec![SceneEntry {
            folder: "Level".to_owned(),
            id: "7".to_owned(),
            name: "7".to_owned(),
            highlighted: 1,
        }];
        assert!(matches!(
            resolve_scene(&named, Some("+7"), None),
            Err(EngineError::UnknownScene { .. })
        ));
    }

    #[test]
    fn scene_resolution_ghz_falls_back_to_the_first_regular_zone() {
        let config = sonic2_like_config();
        assert_eq!(
            resolve_scene(&config, Some("GHZ"), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "GHZ is the canonical first-zone shorthand on trees without Green Hill"
        );
        assert_eq!(
            resolve_scene(&config, Some("ghz2"), None).unwrap(),
            ("Zone01".to_owned(), "2".to_owned())
        );
        assert_eq!(
            resolve_scene(&config, Some("EHZ"), None).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "the native acronym still wins"
        );
        assert_eq!(
            resolve_scene(&config, Some("ChemicalPlant"), None).unwrap(),
            ("Zone02".to_owned(), "1".to_owned())
        );
    }

    #[test]
    fn scene_resolution_errors_list_close_candidates() {
        let config = sonic1_like_config();
        let error = resolve_scene(&config, Some("GREN HILL"), None).unwrap_err();
        let EngineError::UnknownScene { requested, details } = error else {
            panic!("expected UnknownScene, got {error:?}");
        };
        assert_eq!(requested, "GREN HILL");
        assert!(details.contains("GREEN HILL ZONE 1"), "{details}");
        assert!(details.contains("--list"), "{details}");

        let error = resolve_scene(&config, Some("SPECIAL STAGE"), None).unwrap_err();
        let EngineError::UnknownScene { details, .. } = error else {
            panic!("expected UnknownScene, got {error:?}");
        };
        assert!(details.contains("ambiguous"), "{details}");

        let error = resolve_scene(&config, Some("M"), None).unwrap_err();
        let EngineError::UnknownScene { details, .. } = error else {
            panic!("expected UnknownScene, got {error:?}");
        };
        assert!(details.contains("did you mean"), "{details}");
    }

    #[test]
    fn acts_are_normalized_to_their_canonical_form() {
        let config = sonic1_like_config();
        for act in ["b", "B", " B "] {
            assert_eq!(
                resolve_scene(&config, Some("Zone01"), Some(act)).unwrap(),
                ("Zone01".to_owned(), "B".to_owned()),
                "--act {act:?}"
            );
        }
        assert_eq!(
            resolve_scene(&config, Some("Zone01"), Some("01")).unwrap(),
            ("Zone01".to_owned(), "1".to_owned()),
            "leading zeros are stripped"
        );
        for act in ["", "..", "1/2", "1 2"] {
            assert!(
                matches!(
                    resolve_scene(&config, Some("Zone01"), Some(act)),
                    Err(EngineError::InvalidAct(_))
                ),
                "--act {act:?} must be rejected"
            );
        }
    }

    #[test]
    fn available_acts_scans_case_insensitively_and_sorts_naturally() {
        use retro_io::MemorySource;

        let mut source = MemorySource::new();
        for file in [
            "Data/Stages/Zone01/Act1.bin",
            "Data/Stages/Zone01/act2.BIN",
            "Data/Stages/Zone01/ActB.bin",
            "Data/Stages/Zone01/Act10.bin",
            "Data/Stages/Zone01/ActNote.txt",
            "Data/Stages/Zone01/StageConfig.bin",
        ] {
            source.insert(file, vec![0]);
        }
        assert_eq!(
            available_acts(&source, "data/stages/zone01"),
            ["1", "2", "10", "B"]
        );
        assert!(available_acts(&source, "Data/Stages/Missing").is_empty());
    }

    #[test]
    fn missing_act_file_reports_available_acts() {
        use retro_io::MemorySource;

        let mut source = MemorySource::new();
        source.insert("Settings.ini", "[Game]\ngameType=1\n");
        source.insert(
            "Data/Game/GameConfig.bin",
            game_config_bytes_with_scene("Zone01", "1"),
        );
        source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
        source.insert("Data/Stages/Zone01/Act1.bin", scene_bytes("FIRST"));
        source.insert("Data/Stages/Zone01/ActB.bin", scene_bytes("BONUS"));
        let source: Arc<dyn retro_io::DataSource> = Arc::new(source);

        let error = match load_world(&source, Some("Zone01"), Some("3")) {
            Err(error) => error,
            Ok(_) => panic!("expected MissingAct"),
        };
        let EngineError::MissingAct {
            folder,
            act,
            available,
        } = error
        else {
            panic!("expected MissingAct, got {error:?}");
        };
        assert_eq!(folder, "Zone01");
        assert_eq!(act, "3");
        assert_eq!(available, "1, B");
    }

    #[test]
    fn missing_settings_reports_the_file_name() {
        use retro_io::MemorySource;

        let mut source = MemorySource::new();
        source.insert(
            "Data/Game/GameConfig.bin",
            game_config_bytes_with_scene("Zone01", "1"),
        );
        let source: Arc<dyn retro_io::DataSource> = Arc::new(source);
        let error = match load_world(&source, None, None) {
            Err(error) => error,
            Ok(_) => panic!("expected MissingAsset"),
        };
        let EngineError::MissingAsset(path) = error else {
            panic!("expected MissingAsset, got {error:?}");
        };
        assert!(path.contains("Settings.ini"), "{path}");
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
            dim_limit_frames: 18000,
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
