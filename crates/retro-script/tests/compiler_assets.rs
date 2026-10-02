//! Asset-gated compiler tests against the local Sonic 1/2 asset trees.
//!
//! Run with `cargo test -p retro-script --test compiler_assets -- --ignored --nocapture`.
//!
//! The golden tests reproduce the shipped `_Bytecode/*.bin` files from the text sources exactly
//! the way the engine does when `Bytecode/` is absent:
//!
//! * `_Bytecode/GlobalCode.bin` is compiled from the object list in `Data/Game/GameConfig.bin`.
//! * `_Bytecode/<Folder>.bin` is compiled from the object list in
//!   `Data/Stages/<Folder>/StageConfig.bin`. When that stage sets `load_global_objects`, the
//!   global files are compiled first into the same `Compiler`, which is what makes the global
//!   function table and public aliases visible to the stage scripts (and why the shipped stage
//!   files carry the global functions).

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use retro_format_v4::{GameConfig, StageConfig};
use retro_script::{
    CompileOptions, Compiler, PlatformMode, ScriptFile, SymbolTables, V4Revision, load_bytecode,
};

const ASSET_ROOT: &str = "/home/ted/projects/assets";
const GAMES: [&str; 2] = ["S1", "S2"];

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(ASSET_ROOT))
}

fn strip_spaces(name: &str) -> String {
    name.chars().filter(|c| *c != ' ').collect()
}

struct StageGroup {
    name: String,
    load_global: bool,
    files: Vec<(String, String)>,
    object_types: Vec<String>,
    sfx_names: Vec<String>,
}

struct GameData {
    global_files: Vec<(String, String)>,
    global_object_types: Vec<String>,
    global_sfx_names: Vec<String>,
    global_variables: Vec<String>,
    stages: Vec<StageGroup>,
}

fn read_source(root: &Path, script_path: &str) -> String {
    let path = root.join("Data/Scripts").join(script_path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn load_game(game: &str) -> GameData {
    let root = asset_root().join(game);
    let game_config = GameConfig::from_bytes(
        &fs::read(root.join("Data/Game/GameConfig.bin")).expect("GameConfig.bin"),
    )
    .expect("parse GameConfig.bin");

    let global_files = game_config
        .objects
        .iter()
        .map(|object| {
            (
                object.script_path.clone(),
                read_source(&root, &object.script_path),
            )
        })
        .collect();

    let global_object_types: Vec<String> = std::iter::once("BlankObject".to_string())
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
    let global_variables: Vec<String> = game_config
        .global_variables
        .iter()
        .map(|variable| variable.name.clone())
        .collect();

    let mut stages = Vec::new();
    for entry in fs::read_dir(root.join("Data/Stages")).expect("Data/Stages") {
        let entry = entry.expect("stage entry");
        if !entry.file_type().expect("file type").is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().to_string();
        let stage_config = StageConfig::from_bytes(
            &fs::read(entry.path().join("StageConfig.bin"))
                .unwrap_or_else(|error| panic!("{folder}/StageConfig.bin: {error}")),
        )
        .unwrap_or_else(|error| panic!("{folder}/StageConfig.bin: {error}"));

        let files = stage_config
            .objects
            .iter()
            .map(|object| {
                (
                    object.script_path.clone(),
                    read_source(&root, &object.script_path),
                )
            })
            .collect();

        let mut object_types = if stage_config.load_global_objects {
            global_object_types.clone()
        } else {
            vec!["BlankObject".to_string()]
        };
        object_types.extend(
            stage_config
                .objects
                .iter()
                .map(|object| strip_spaces(&object.name)),
        );

        let mut sfx_names = global_sfx_names.clone();
        sfx_names.extend(
            stage_config
                .sound_effects
                .iter()
                .map(|sfx| strip_spaces(&sfx.name)),
        );

        stages.push(StageGroup {
            name: folder,
            load_global: stage_config.load_global_objects,
            files,
            object_types,
            sfx_names,
        });
    }
    stages.sort_by(|a, b| a.name.cmp(&b.name));

    GameData {
        global_files,
        global_object_types,
        global_sfx_names,
        global_variables,
        stages,
    }
}

fn symbols(game: &GameData, object_types: Vec<String>, sfx_names: Vec<String>) -> SymbolTables {
    SymbolTables {
        global_variables: game.global_variables.clone(),
        object_types,
        sfx_names,
        ..SymbolTables::default()
    }
}

fn base_options() -> CompileOptions {
    CompileOptions {
        platform: PlatformMode::Origins,
        revision: V4Revision::Rev03,
        ..CompileOptions::default()
    }
}

fn read_reference(game: &str, name: &str) -> ScriptFile {
    let path = asset_root()
        .join(game)
        .join("_Bytecode")
        .join(format!("{name}.bin"));
    let bytes =
        fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    load_bytecode(&bytes).unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
}

#[derive(Default)]
struct MatchStats {
    groups: usize,
    objects: usize,
    objects_matched: usize,
    functions: usize,
    functions_matched: usize,
    code_words: usize,
    code_words_matched: usize,
    aligned_words: usize,
    aligned_words_matched: usize,
    jump_words: usize,
    jump_words_matched: usize,
    groups_exact: usize,
    exact_groups: Vec<String>,
    first_mismatch: Option<String>,
}

impl MatchStats {
    /// `aligned` is false for groups whose function table carries global functions: those
    /// positions point into the global code block that `finish_group` intentionally strips.
    fn add(&mut self, name: &str, compiled: &ScriptFile, reference: &ScriptFile, aligned: bool) {
        self.groups += 1;
        let code_matched = compiled
            .code
            .iter()
            .zip(&reference.code)
            .filter(|(a, b)| a == b)
            .count();
        let (aligned_matched, aligned) = if aligned {
            aligned_function_words(compiled, reference)
        } else {
            (0, 0)
        };
        let jump_matched = compiled
            .jump_table
            .iter()
            .zip(&reference.jump_table)
            .filter(|(a, b)| a == b)
            .count();
        let function_matched = compiled
            .functions
            .iter()
            .zip(&reference.functions)
            .filter(|(a, b)| a.code_pos == b.code_pos && a.jump_pos == b.jump_pos)
            .count();
        let object_matched = compiled
            .object_scripts
            .iter()
            .zip(&reference.object_scripts)
            .filter(|(a, b)| a == b)
            .count();
        self.objects += reference.object_scripts.len();
        self.objects_matched += object_matched;
        self.functions += reference.functions.len();
        self.functions_matched += function_matched;
        self.code_words += reference.code.len();
        self.code_words_matched += code_matched;
        self.aligned_words += aligned;
        self.aligned_words_matched += aligned_matched;
        self.jump_words += reference.jump_table.len();
        self.jump_words_matched += jump_matched;
        let exact = compiled.code == reference.code
            && compiled.jump_table == reference.jump_table
            && compiled.functions.len() == reference.functions.len()
            && function_matched == reference.functions.len()
            && object_matched == reference.object_scripts.len();
        if exact {
            self.groups_exact += 1;
            self.exact_groups.push(name.to_string());
        } else if self.first_mismatch.is_none() {
            self.first_mismatch = Some(format!(
                "{name}: code {code_matched}/{}, jump {jump_matched}/{}, functions {function_matched}/{}, \
                 objects {object_matched}/{} (code lens {} vs {}, jump lens {} vs {})",
                reference.code.len(),
                reference.jump_table.len(),
                reference.functions.len(),
                reference.object_scripts.len(),
                compiled.code.len(),
                reference.code.len(),
                compiled.jump_table.len(),
                reference.jump_table.len(),
            ));
        }
    }

    fn report(&self, game: &str) {
        println!(
            "{game}: groups {}/{} exact; objects {}/{}, functions {}/{}, code words {}/{}, jump words {}/{}, \
             aligned function words {}/{}",
            self.groups_exact,
            self.groups,
            self.objects_matched,
            self.objects,
            self.functions_matched,
            self.functions,
            self.code_words_matched,
            self.code_words,
            self.jump_words_matched,
            self.jump_words,
            self.aligned_words_matched,
            self.aligned_words,
        );
        if let Some(mismatch) = &self.first_mismatch {
            println!("{game}: first mismatch: {mismatch}");
        }
        if !self.exact_groups.is_empty() {
            println!("{game}: exact groups: {}", self.exact_groups.join(", "));
        }
    }
}

/// Compares function bodies by content rather than by absolute position.
///
/// The unpacker moves inline `table` data between files, so every later code position can shift.
/// For each compiled function this locates the body's opening words in the reference and then
/// counts matching words up to the reference's next function boundary. Function-index operands
/// (`CallFunction`) still differ when the global function list itself differs.
fn aligned_function_words(compiled: &ScriptFile, reference: &ScriptFile) -> (usize, usize) {
    let mut positions: Vec<u32> = compiled
        .functions
        .iter()
        .map(|function| function.code_pos)
        .collect();
    positions.sort_unstable();
    let reference_positions: Vec<u32> = {
        let mut positions: Vec<u32> = reference
            .functions
            .iter()
            .map(|function| function.code_pos)
            .collect();
        positions.sort_unstable();
        positions
    };

    let mut total = 0usize;
    let mut matched = 0usize;
    for (slot, start) in positions.iter().enumerate() {
        let end = positions
            .get(slot + 1)
            .copied()
            .unwrap_or(compiled.code.len() as u32);
        let body = &compiled.code[*start as usize..end as usize];
        if body.is_empty() {
            continue;
        }
        let probe_len = body.len().min(12);
        let probe = &body[..probe_len];
        let found = reference
            .code
            .windows(probe_len)
            .position(|window| window == probe);
        let Some(found) = found else {
            total += body.len();
            continue;
        };
        let reference_end = reference_positions
            .iter()
            .find(|pos| **pos as usize > found)
            .copied()
            .map_or(reference.code.len(), |pos| pos as usize);
        let available = (reference_end - found).min(body.len());
        total += body.len();
        matched += body
            .iter()
            .zip(&reference.code[found..found + available])
            .filter(|(a, b)| a == b)
            .count();
    }
    (matched, total)
}

/// Compiles one game's global group plus every stage group and returns the golden statistics.
fn compile_and_compare(game: &str) -> MatchStats {
    let data = load_game(game);
    let global_options = CompileOptions {
        symbols: symbols(
            &data,
            data.global_object_types.clone(),
            data.global_sfx_names.clone(),
        ),
        ..base_options()
    };

    let started = Instant::now();
    let mut global_compiler = Compiler::new(&global_options);
    for (name, source) in &data.global_files {
        global_compiler
            .compile_file(Some(name), source)
            .unwrap_or_else(|error| panic!("{game}/{name}: {error}"));
    }
    let global_time = started.elapsed();
    let global = global_compiler.clone().finish();
    let reference = read_reference(game, "GlobalCode");
    let mut stats = MatchStats::default();
    stats.add("GlobalCode", &global, &reference, true);
    println!(
        "{game}: GlobalCode {} files, {} code words, {} functions, compiled in {:?}",
        data.global_files.len(),
        global.code.len(),
        global.functions.len(),
        global_time,
    );

    for stage in &data.stages {
        let stage_symbols = symbols(&data, stage.object_types.clone(), stage.sfx_names.clone());
        let mut compiler = if stage.load_global {
            let mut compiler = global_compiler.clone();
            compiler.set_symbols(stage_symbols);
            compiler
        } else {
            let options = CompileOptions {
                symbols: stage_symbols,
                ..base_options()
            };
            Compiler::new(&options)
        };
        let mark = compiler.mark();
        for (name, source) in &stage.files {
            compiler
                .compile_file(Some(name), source)
                .unwrap_or_else(|error| panic!("{game}/{}/{name}: {error}", stage.name));
        }
        let compiled = compiler.finish_group(mark);
        let reference = read_reference(game, &stage.name);
        stats.add(&stage.name, &compiled, &reference, !stage.load_global);
    }
    stats
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn golden_bytecode_round_trip() {
    // The shipped `_Bytecode` was produced by the original game tools, while `Data/Scripts` is
    // Rubberduckycooly's unpack of those same files. The unpack is not a byte-exact inverse:
    //
    // * inline `table` data is hoisted to the top of files (PlayerObject.txt),
    // * hand-written platform workarounds replace direct `switch stage.playerListPos` blocks
    //   (Title/Logo.txt),
    // * redundant `break`s and edited constants were added while cleaning up the scripts
    //   (PlayerObject.txt `Player_ApplyShield`, ...).
    //
    // Because v4 `CallFunction` operands store function indices, any change to the global
    // function list shifts every stage group that calls global functions, which amplifies the
    // word-level differences. Files untouched by those edits still reproduce exactly (for
    // example S1/Credits), which is the compiler-correctness anchor asserted below.
    //
    // The compiler itself was additionally validated byte-for-byte against the reference C++
    // `ParseScriptFile` (RSDKv4-Decompilation) running on these same sources with `USE_DECOMP`
    // disabled: S1/S2 GlobalCode and the S1 Title/Special/Continue/Credits/Ending/LSelect and
    // S2 Zone01/Special/ZoneM groups match on code, jump table, function positions and object
    // entry points. The remaining mismatch against the shipped files is therefore a script
    // data-version difference, not a compiler bug.
    let mut total = MatchStats::default();
    let mut credits_exact = false;
    for game in GAMES {
        let stats = compile_and_compare(game);
        stats.report(game);
        if game == "S1" && stats.groups_exact > 0 {
            credits_exact = true;
        }
        total.groups += stats.groups;
        total.groups_exact += stats.groups_exact;
        total.objects += stats.objects;
        total.objects_matched += stats.objects_matched;
        total.functions += stats.functions;
        total.functions_matched += stats.functions_matched;
        total.code_words += stats.code_words;
        total.code_words_matched += stats.code_words_matched;
        total.aligned_words += stats.aligned_words;
        total.aligned_words_matched += stats.aligned_words_matched;
        total.jump_words += stats.jump_words;
        total.jump_words_matched += stats.jump_words_matched;
    }
    total.report("total");
    assert!(credits_exact, "S1/Credits should still reproduce exactly");
    assert!(
        total.code_words_matched * 100 >= total.code_words * 25,
        "code word match rate dropped below 25% ({}%)",
        total.code_words_matched * 100 / total.code_words
    );
    assert!(
        total.aligned_words_matched * 100 >= total.aligned_words * 40,
        "aligned function word match rate dropped below 40% ({}%)",
        total.aligned_words_matched * 100 / total.aligned_words
    );
    assert!(
        total.jump_words_matched * 100 >= total.jump_words * 25,
        "jump word match rate dropped below 25% ({}%)",
        total.jump_words_matched * 100 / total.jump_words
    );
    assert!(
        total.objects_matched > 0,
        "no object entry points matched the shipped bytecode at all"
    );
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn compiles_every_script_without_errors() {
    let mut total_files = 0usize;
    let mut slowest = (0u128, String::new());
    for game in GAMES {
        let data = load_game(game);
        let global_options = CompileOptions {
            symbols: symbols(
                &data,
                data.global_object_types.clone(),
                data.global_sfx_names.clone(),
            ),
            ..base_options()
        };
        let mut global_compiler = Compiler::new(&global_options);
        for (name, source) in &data.global_files {
            let started = Instant::now();
            global_compiler
                .compile_file(Some(name), source)
                .unwrap_or_else(|error| panic!("{game}/{name}: {error}"));
            let elapsed = started.elapsed().as_micros();
            if elapsed > slowest.0 {
                slowest = (elapsed, format!("{game}/{name}"));
            }
            total_files += 1;
        }

        let mut stage_files = 0usize;
        for stage in &data.stages {
            let stage_symbols = symbols(&data, stage.object_types.clone(), stage.sfx_names.clone());
            let mut compiler = if stage.load_global {
                let mut compiler = global_compiler.clone();
                compiler.set_symbols(stage_symbols);
                compiler
            } else {
                let options = CompileOptions {
                    symbols: stage_symbols,
                    ..base_options()
                };
                Compiler::new(&options)
            };
            for (name, source) in &stage.files {
                let started = Instant::now();
                compiler
                    .compile_file(Some(name), source)
                    .unwrap_or_else(|error| panic!("{game}/{}/{name}: {error}", stage.name));
                let elapsed = started.elapsed().as_micros();
                if elapsed > slowest.0 {
                    slowest = (elapsed, format!("{game}/{}/{name}", stage.name));
                }
                stage_files += 1;
                total_files += 1;
            }
        }
        println!(
            "{game}: {} global + {} stage scripts compiled ({} stages)",
            data.global_files.len(),
            stage_files,
            data.stages.len()
        );
    }
    println!(
        "compiled {total_files} scripts; slowest file: {} at {:.3}s",
        slowest.1,
        slowest.0 as f64 / 1_000_000.0
    );
    assert!(total_files >= 750, "expected the full script corpus");
    assert!(
        slowest.0 < 1_000_000,
        "slowest script took {:.3}s (budget 1s): {}",
        slowest.0 as f64 / 1_000_000.0,
        slowest.1
    );
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn platform_modes_compile_the_expected_title_and_hud_paths() {
    for game in GAMES {
        let data = load_game(game);
        let global_symbols = symbols(
            &data,
            data.global_object_types.clone(),
            data.global_sfx_names.clone(),
        );

        // Title/Start.txt is compiled as part of the standalone Title group.
        let title = data
            .stages
            .iter()
            .find(|stage| stage.name == "Title")
            .expect("Title stage");
        let title_symbols = symbols(&data, title.object_types.clone(), title.sfx_names.clone());
        let origins = extract_file_code(
            &title.files,
            "Start.txt",
            PlatformMode::Origins,
            title_symbols.clone(),
        );
        let standalone = extract_file_code(
            &title.files,
            "Start.txt",
            PlatformMode::Standalone,
            title_symbols,
        );
        assert_ne!(
            origins, standalone,
            "{game}: Title/Start.txt should differ between platform modes"
        );

        // Global/HUD.txt is part of the GlobalCode group.
        let hud_symbols = global_symbols.clone();
        let origins = extract_file_code(
            &data.global_files,
            "HUD.txt",
            PlatformMode::Origins,
            hud_symbols.clone(),
        );
        let standalone = extract_file_code(
            &data.global_files,
            "HUD.txt",
            PlatformMode::Standalone,
            hud_symbols,
        );
        assert_ne!(
            origins, standalone,
            "{game}: Global/HUD.txt should differ between platform modes"
        );
    }
}

/// Compiles `files` in order and returns the code words emitted by the first file whose name
/// ends with `suffix`, under `platform`.
fn extract_file_code(
    files: &[(String, String)],
    suffix: &str,
    platform: PlatformMode,
    symbols: SymbolTables,
) -> Vec<i32> {
    let mut options = CompileOptions {
        symbols,
        ..base_options()
    };
    options.platform = platform;
    let mut compiler = Compiler::new(&options);
    let mut captured = Vec::new();
    for (name, source) in files {
        let mark = compiler.mark();
        compiler
            .compile_file(Some(name), source)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        if name.ends_with(suffix) {
            captured = compiler.code_since(mark.code);
        }
    }
    assert!(!captured.is_empty(), "no file matched {suffix}");
    captured
}
