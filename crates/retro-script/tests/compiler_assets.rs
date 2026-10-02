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
//!
//! `Data/Scripts` is a decompilation, not a byte-exact inverse of the shipped bytecode, so
//! `golden_bytecode_round_trip` reports partial match rates. Compiler correctness is pinned
//! instead by `oracle_parity_hashes`: the Rust output is byte-identical to the reference C++
//! `ParseScriptFile` for 71 of the 72 shipped groups (the exception is `S2/Mission_Zone02`, see
//! `mission_zone02_long_alias_divergence`). `tools/script-oracle` rebuilds that reference from a
//! pinned upstream commit at run time; `dump_compiled_groups` writes the Rust side for a manual
//! byte comparison.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use retro_format_v4::{GameConfig, StageConfig};
use retro_script::{
    CompileOptions, Compiler, Op, PlatformMode, ScriptFile, ScriptVersion, SymbolTables,
    V4Revision, load_bytecode, write_bytecode,
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
    base_options_for(PlatformMode::Origins)
}

fn base_options_for(platform: PlatformMode) -> CompileOptions {
    CompileOptions {
        platform,
        revision: V4Revision::Rev03,
        ..CompileOptions::default()
    }
}

fn blake3_of(file: &ScriptFile) -> String {
    blake3::hash(&write_bytecode(file).expect("serialise compiled group"))
        .to_hex()
        .to_string()
}

/// A compiled group (`GlobalCode` or one stage), produced exactly the way the engine links it.
struct CompiledGroup {
    name: String,
    file: ScriptFile,
    load_global: bool,
}

/// Compiles the global group and every stage group for `game` under `platform`.
fn compiled_groups(game: &str, platform: PlatformMode) -> Vec<CompiledGroup> {
    let data = load_game(game);
    let global_options = CompileOptions {
        symbols: symbols(
            &data,
            data.global_object_types.clone(),
            data.global_sfx_names.clone(),
        ),
        ..base_options_for(platform)
    };

    let mut global_compiler = Compiler::new(&global_options);
    for (name, source) in &data.global_files {
        global_compiler
            .compile_file(Some(name), source)
            .unwrap_or_else(|error| panic!("{game}/{name}: {error}"));
    }

    let mut groups = vec![CompiledGroup {
        name: "GlobalCode".to_string(),
        file: global_compiler.clone().finish(),
        load_global: false,
    }];

    for stage in &data.stages {
        let stage_symbols = symbols(&data, stage.object_types.clone(), stage.sfx_names.clone());
        let mut compiler = if stage.load_global {
            let mut compiler = global_compiler.clone();
            compiler.set_symbols(stage_symbols);
            compiler
        } else {
            let options = CompileOptions {
                symbols: stage_symbols,
                ..base_options_for(platform)
            };
            Compiler::new(&options)
        };
        let mark = compiler.mark();
        for (name, source) in &stage.files {
            compiler
                .compile_file(Some(name), source)
                .unwrap_or_else(|error| panic!("{game}/{}/{name}: {error}", stage.name));
        }
        groups.push(CompiledGroup {
            name: stage.name.clone(),
            file: compiler.finish_group(mark),
            load_global: stage.load_global,
        });
    }
    groups
}

/// Compiles a single named group (`GlobalCode` or one stage) under `platform`.
fn compile_named_group(game: &str, name: &str, platform: PlatformMode) -> ScriptFile {
    let data = load_game(game);
    let global_options = CompileOptions {
        symbols: symbols(
            &data,
            data.global_object_types.clone(),
            data.global_sfx_names.clone(),
        ),
        ..base_options_for(platform)
    };
    let mut compiler = Compiler::new(&global_options);
    for (file, source) in &data.global_files {
        compiler
            .compile_file(Some(file), source)
            .unwrap_or_else(|error| panic!("{game}/{file}: {error}"));
    }
    if name == "GlobalCode" {
        return compiler.finish();
    }

    let stage = data
        .stages
        .iter()
        .find(|stage| stage.name == name)
        .unwrap_or_else(|| panic!("{game}: no stage named {name}"));
    let stage_symbols = symbols(&data, stage.object_types.clone(), stage.sfx_names.clone());
    let mut compiler = if stage.load_global {
        compiler.set_symbols(stage_symbols);
        compiler
    } else {
        let options = CompileOptions {
            symbols: stage_symbols,
            ..base_options_for(platform)
        };
        Compiler::new(&options)
    };
    let mark = compiler.mark();
    for (file, source) in &stage.files {
        compiler
            .compile_file(Some(file), source)
            .unwrap_or_else(|error| panic!("{game}/{}/{file}: {error}", stage.name));
    }
    compiler.finish_group(mark)
}

/// True when `haystack` contains `needle` as a contiguous subsequence.
fn contains(haystack: &[i32], needle: &[i32]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn op(op: Op) -> i32 {
    retro_script::encoded_opcode(ScriptVersion::V4, V4Revision::Rev03, op).unwrap() as i32
}

/// Finds a switch whose `[min, max, default, end, case0..caseN]` block starts at `min`/`max` and
/// whose case number `case_index` points at `case_value`.
fn find_switch(
    jump_table: &[u32],
    min: u32,
    max: u32,
    case_index: usize,
    case_value: u32,
) -> Option<usize> {
    (0..jump_table.len().saturating_sub(4 + case_index + 1)).find(|index| {
        jump_table[*index] == min
            && jump_table[*index + 1] == max
            && jump_table[*index + 4 + case_index] == case_value
    })
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
    let groups = compiled_groups(game, PlatformMode::Origins);
    let mut stats = MatchStats::default();
    for group in &groups {
        let reference = read_reference(game, &group.name);
        stats.add(&group.name, &group.file, &reference, !group.load_global);
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
    // `ParseScriptFile` (RSDKv4-Decompilation @ a7f5195, run through `tools/script-oracle` with
    // `USE_DECOMP` disabled to mirror the original tools): **71 of the 72 groups are identical**
    // on code words, jump table entries, function positions and object entry points. The single
    // exception is S2 `Mission_Zone02`, where the reference stores alias names in
    // `char name[0x20]`; the 32-character `EGGMANSIGNPOST_SPAWNFALLSIGNPOST` leaves no room for a
    // NUL terminator, so the reference's `StrComp` reads into the following `value` field and
    // fails to resolve the alias (its case table keeps the default position 921). This port uses
    // `String` and resolves it (case body 364); the shipped `_Bytecode` agrees with this port.
    // See `mission_zone02_long_alias_divergence` and `oracle_parity_hashes` below.
    //
    // The remaining mismatch against the shipped files is therefore a script data-version
    // difference, not a compiler bug.
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

/// blake3 of the canonical `write_bytecode` output for every group where this port is
/// byte-identical to the reference C++ compiler (71 of 72 groups; the exception is
/// `S2/Mission_Zone02`, covered by `mission_zone02_long_alias_divergence`).
///
/// Reproduce with `tools/script-oracle/run.sh <assets>/<game> --all <dir>` and compare the
/// files byte-for-byte; see `dump_compiled_groups` for writing the Rust side.
const ORACLE_PARITY_HASHES: &[(&str, &str, &str)] = &[
    (
        "S1",
        "BR1Zone01",
        "7e5784424cde3b2e93eac1ee441a73610d05b856ca1e797b2ed0a157af012cae",
    ),
    (
        "S1",
        "BR2Zone02",
        "fc7237ea4876af308ba6f8d8e0de8f3ad4fd36b695415ad7ecfa93f8227913bb",
    ),
    (
        "S1",
        "BR3Zone03",
        "599f629de62d1b0959a6e50efbcd9307cec7c656c2fec2a2eaf8e7d8ad378127",
    ),
    (
        "S1",
        "BR4Zone04",
        "5a60952393cfd903a3e57a83933bc2a7d9db0e02daa95a1b5b1130df8a9623cb",
    ),
    (
        "S1",
        "BR5Zone05",
        "71392dec64fd02e30f307c08b3f625994b2ca0baa7ef374e9cd399b78e220a81",
    ),
    (
        "S1",
        "BR6Zone06",
        "4ee5c136db37f82235951f7eb69cea08d06483d08dd9209bb45fe937c78027bd",
    ),
    (
        "S1",
        "Continue",
        "9dcc80b986749bb3093bf96d5e816731360c70bd409d18ce5dcc15d44704be06",
    ),
    (
        "S1",
        "Credits",
        "928e4d71efcfac5247aa9b19dbed86f30e3295b6ce3e35d446ea0121348c132b",
    ),
    (
        "S1",
        "DLC_Zone01",
        "b0663b003017fd8f88dc7b5a261c135966c6dea6b04df1db6d36fa339688f504",
    ),
    (
        "S1",
        "DLC_Zone02",
        "b219bfa8c39e5c95fbd5d7304129a159dbab356860819fe92ba71cbb04d2ef44",
    ),
    (
        "S1",
        "DLC_Zone06",
        "94c29f6169192e75ec88308ed3cc864a06081be3e31449cf1c50273477c73a6e",
    ),
    (
        "S1",
        "Ending",
        "9278cd4ce1a226f19c3d93b9b62634d82fbf9d94c25840e417cb1be3d4cb8268",
    ),
    (
        "S1",
        "GlobalCode",
        "ae95a4bf4450f132408e928956c1a283968fc195ded88d072110e608ddcf98cb",
    ),
    (
        "S1",
        "LSelect",
        "96b09fceeb2d58d50cf50d99a372fb9c498fa71d8de527e1118fd38e2228cd88",
    ),
    (
        "S1",
        "Mission_M",
        "7f240e74bb932d9e97c27e1605eee6bad625fb88302b2d59243725b740a40976",
    ),
    (
        "S1",
        "Mission_Zone01",
        "7437fefb01e929971929132c7e5e95e1c74dc23e4c81d48fc0b3a7dc4993a67c",
    ),
    (
        "S1",
        "Mission_Zone02",
        "a1bd15fe05164d2cec6d7bffc51662d31542f916fe07177de0039d3eb8360f0e",
    ),
    (
        "S1",
        "Mission_Zone03",
        "2d3fb17b19bcec0e330ae3a1e1dd75f91a86a7a66006dfb08395682c7b6ffe0b",
    ),
    (
        "S1",
        "Mission_Zone04",
        "28b8054be36e9fcad02cad622c0f141f9d8e7a20644452b1e7c9703720eb816a",
    ),
    (
        "S1",
        "Mission_Zone05",
        "3f70b3af6c65709bdc8c63a72d846e73578723b26314e32f719fd07bbe4f6c50",
    ),
    (
        "S1",
        "Mission_Zone06",
        "7757d2f52cf5427356641bb7084c51b77d274603dd299c72d5bd4e1461742fa6",
    ),
    (
        "S1",
        "Special",
        "3b3f135e20c51abce945bd7f4626decc74e3a20b7d8cc48ce7f48157dedf6866",
    ),
    (
        "S1",
        "Title",
        "3208e28ea9a3b5242ce1257a1d16660570baec3ec51093701b35e41d4ca41ee2",
    ),
    (
        "S1",
        "Zone01",
        "7f240e74bb932d9e97c27e1605eee6bad625fb88302b2d59243725b740a40976",
    ),
    (
        "S1",
        "Zone02",
        "7e27fdae55f36e3b592aa4273719a04c046c9adb66f2ac6e0d96a47733f9415f",
    ),
    (
        "S1",
        "Zone03",
        "895268e753f5d2c5b581b658cb26a53118a04de8a46ebaa4d38e92bf8e651ae2",
    ),
    (
        "S1",
        "Zone04",
        "23969eb13824d9061d102c087cd2a73da9d709791e59570fce822b169ef66243",
    ),
    (
        "S1",
        "Zone05",
        "aa170d752896908430a6aa3b869ba4588a1964e46fef41b71287406e02a5880e",
    ),
    (
        "S1",
        "Zone06",
        "4ee5c136db37f82235951f7eb69cea08d06483d08dd9209bb45fe937c78027bd",
    ),
    (
        "S2",
        "BR1Zone01",
        "ae0b81a7912d972d7759322395e2753076feaf5e01e22e8fcf39670a463c512b",
    ),
    (
        "S2",
        "BR2Zone02",
        "21143c2801bc4f54bcccd462cd2afacee170bfcd34ed870276b7be75a3e24d50",
    ),
    (
        "S2",
        "BR3Zone03",
        "ea85c33eaea8fb83af8e69b6ea907dd2b7b68319984243894812946b2dbebd54",
    ),
    (
        "S2",
        "BR4Zone04",
        "336fb67c0be90a1a13392b7349dd9b06281b31e360f2febc44dc9ee04a6c86f9",
    ),
    (
        "S2",
        "BR5Zone05",
        "14d81e2b142b7e13666bd9ea8b41587e4702a64b6ca051ad00386ef884a63fc8",
    ),
    (
        "S2",
        "BR6Zone06",
        "28260c007005fb19cd174c9456f6d6b13ffdbf39e51e025494548154e7c62380",
    ),
    (
        "S2",
        "BR7Zone07",
        "b66817bd8b40ee49667d5357b2ec5a77e2fc0b6a9631ebf19364ae81c4b8c178",
    ),
    (
        "S2",
        "BR8Zone09",
        "9b19608fb20066ede1383bb66471609de85d32388edfeab330c0ae1dfc3dcee9",
    ),
    (
        "S2",
        "BR9Zone11",
        "58d039a14b7e99a01e22fe147fa830b09db7419064bf7aedda8681e82ebf7446",
    ),
    (
        "S2",
        "BR9Zone12",
        "851c98c004272a36a20ce265da1756ff99e21980bd2ff6bee9b0baf89a68e946",
    ),
    (
        "S2",
        "Continue",
        "890b6ec25e1b970c8a0b832a665ceec8f4ad785296b9984057fb263da4934d03",
    ),
    (
        "S2",
        "Credits",
        "4a9eb4fc37e8786b3ffe6edfb27a6a42b29a176c8a375077f9cd2b6a86e6f3b4",
    ),
    (
        "S2",
        "DLC_Zone02",
        "28352bf6c76ac19e7f31bb0fe1714779639e28e49d377570fdc3de745ca36254",
    ),
    (
        "S2",
        "DLC_Zone06",
        "5b50dc90ce552413e701026305056ca9eb5587415cc8a36481ce395474e2a57a",
    ),
    (
        "S2",
        "DLC_Zone09",
        "da48b9515c311c0830bddccf6bcab05c6ae843d34e2060b688c43a0c7d809684",
    ),
    (
        "S2",
        "Ending",
        "5fa865fa4e1825298fd091b454fdda2f45e7f39de7d1e8d16754d185b0bd1285",
    ),
    (
        "S2",
        "GlobalCode",
        "842a65f6be546092711cc867c25bed39573a4ab53199c7425c415969609e61a3",
    ),
    (
        "S2",
        "LSelect",
        "e1b1e6f15384bc0198a86e7bd898c7ae352890ddabaf85843afa6d323cc0ba35",
    ),
    (
        "S2",
        "Mission_Zone01",
        "f2b7d3b31813e19d913337fe1996ee198329f2d7006706932b35f7c3d8b6efc4",
    ),
    (
        "S2",
        "Mission_Zone03",
        "852ae55cbd2b8ca98fd83a6e8176a70518f1f01a04f951f4ca67982f4f09508f",
    ),
    (
        "S2",
        "Mission_Zone04",
        "89aa25d0492f641091b4e83dceae5a8ef982972666667d268fd67ef39cd278be",
    ),
    (
        "S2",
        "Mission_Zone05",
        "4ba08fa6d5c0e0b467e2e5ddd4649f9dc5b8ec06b2179e7a39c2ab918017748b",
    ),
    (
        "S2",
        "Mission_Zone06",
        "c69985f6cd1a0a2c995f2b39ad37cc4bef29dff8533e40f1efe5d82a6a14eabe",
    ),
    (
        "S2",
        "Mission_Zone07",
        "8ae75758fea68ce7afd6d5201e80d62e12f486988afa412cb08e98535d684071",
    ),
    (
        "S2",
        "Mission_Zone09",
        "0e124e163ac67e31e37b2e42b0b9550d0d2d05285e3c5624f8d15d27866461a0",
    ),
    (
        "S2",
        "Mission_Zone10",
        "4c90af1f7821c37ddecfa3f235284e899f1f426177602d266b4ef60bc5a86a48",
    ),
    (
        "S2",
        "Mission_Zone11",
        "31931849868d7723525256c2d49e9cb5cfc19535c17ee38595b4da27fe0042a1",
    ),
    (
        "S2",
        "Special",
        "efd6b8d3532fee3fa2c64099bcc5181a02f79d2b7556930dedc2c109a3d34428",
    ),
    (
        "S2",
        "Title",
        "fb038d5ad5bda99f62633490b4115129840c00bbc9da3df64d7939da05ff1e33",
    ),
    (
        "S2",
        "Zone01",
        "96b30ed1b47654fc872621453e5ec6c1f7074f6e16bd4de432236b18dc904af1",
    ),
    (
        "S2",
        "Zone02",
        "28a0fe2815745c5caaae8633ef9cd23a1b3510ad03b75a30b7289b8c9f999c5f",
    ),
    (
        "S2",
        "Zone03",
        "a2d95843244ea656a634ec6e77e9a9b9972bc428fb78803ace75c29339a6b141",
    ),
    (
        "S2",
        "Zone04",
        "750c82a274387dc1ab611d5414b613422efa98c068094cdfe769076afedaee5e",
    ),
    (
        "S2",
        "Zone05",
        "80754da97afb8c57fa937d9f546bef913a36c9aacfe2e84a0d8988ea49cfe6b1",
    ),
    (
        "S2",
        "Zone06",
        "a2ff38731314208dac77d2ca6c593a8ec10cdb67a1fad3d98571eb92ba8c6236",
    ),
    (
        "S2",
        "Zone07",
        "83df43045710bdd36f210c9b54ea4d66fe23d931365094a9ea239ef571ef5038",
    ),
    (
        "S2",
        "Zone08",
        "ddd4cf0f609bb25e14615cb53f17d05520d470a6e9997a99b81221f5261d0a00",
    ),
    (
        "S2",
        "Zone09",
        "5eaf02ba4114bc2a7e9a698bd8a2971e0cda86201215171dc2ed4930bc1b7cfe",
    ),
    (
        "S2",
        "Zone10",
        "b6674e92c3d4460ed4420aef667e36be0f8e6d0cf87fef9d82a45d0a5283263b",
    ),
    (
        "S2",
        "Zone11",
        "1a6fc8d0c47cc7c61b4479303f7febcb40c12749f7fd084df8a575bebc7e606a",
    ),
    (
        "S2",
        "Zone12",
        "851c98c004272a36a20ce265da1756ff99e21980bd2ff6bee9b0baf89a68e946",
    ),
    (
        "S2",
        "ZoneM",
        "1d212fce79e8df7aa137560c5556ff6da1c729a17751792bff86ccf3408523c1",
    ),
];

/// blake3 of the canonical `write_bytecode` output for the `GlobalCode` and `Title` groups
/// under each platform mode. Both modes were validated against the reference C++ compiler.
const PLATFORM_MODE_HASHES: &[(&str, &str, &str, &str)] = &[
    (
        "S1",
        "GlobalCode",
        "ae95a4bf4450f132408e928956c1a283968fc195ded88d072110e608ddcf98cb",
        "144da009d382e3a05379d01f16c07da1b9adb27e436e0725abac392c5e24159a",
    ),
    (
        "S1",
        "Title",
        "3208e28ea9a3b5242ce1257a1d16660570baec3ec51093701b35e41d4ca41ee2",
        "93f37d2544d6d518ba32bb54de3aa6d9bc22ac3c5f09a8009a182f06b9780650",
    ),
    (
        "S2",
        "GlobalCode",
        "842a65f6be546092711cc867c25bed39573a4ab53199c7425c415969609e61a3",
        "8e45783f0743110776ff2b2578133f432a7d7f8d42a4b8e186e8246fd5c4bcce",
    ),
    (
        "S2",
        "Title",
        "fb038d5ad5bda99f62633490b4115129840c00bbc9da3df64d7939da05ff1e33",
        "d503f75b6762a5ff611b3663f15ee6ab062135ddd81afef438350e861ac4105f",
    ),
];

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn oracle_parity_hashes() {
    assert_eq!(
        ORACLE_PARITY_HASHES.len(),
        71,
        "expected 71 oracle-identical groups"
    );
    assert!(
        !ORACLE_PARITY_HASHES
            .iter()
            .any(|(game, name, _)| *game == "S2" && *name == "Mission_Zone02"),
        "Mission_Zone02 diverges from the reference and must not carry a parity hash"
    );
    for game in GAMES {
        for group in compiled_groups(game, PlatformMode::Origins) {
            if game == "S2" && group.name == "Mission_Zone02" {
                continue;
            }
            let expected = ORACLE_PARITY_HASHES
                .iter()
                .find(|(entry_game, entry_name, _)| {
                    *entry_game == game && *entry_name == group.name
                })
                .unwrap_or_else(|| panic!("missing golden hash for {game}/{}", group.name));
            assert_eq!(
                blake3_of(&group.file),
                expected.2,
                "{game}/{}: compiler output changed; re-run tools/script-oracle before updating the hash",
                group.name
            );
        }
    }
    println!("all 71 oracle-identical groups match their committed blake3 goldens");
}

/// Documents the single known divergence from the reference C++ compiler.
///
/// The decomp reference declares `char name[0x20]` for aliases. A 32-character alias name fills
/// that array exactly, leaving no NUL terminator, so the reference's `StrComp` continues into the
/// adjacent `value` field and the alias never matches. The original tools (and this port) resolve
/// it. The shipped `_Bytecode` agrees with this port, which is the strongest evidence that the
/// divergence is a reference buffer bug rather than a port bug.
#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn mission_zone02_long_alias_divergence() {
    let groups = compiled_groups("S2", PlatformMode::Origins);
    let compiled = &groups
        .iter()
        .find(|group| group.name == "Mission_Zone02")
        .expect("Mission_Zone02")
        .file;
    let reference = read_reference("S2", "Mission_Zone02");

    // `case EGGMANSIGNPOST_SPAWNFALLSIGNPOST` is case 5 of a min=0/max=8 switch in
    // `Mission/EggmanSignPost.txt`. The case body starts at 364 in both the compiled output and
    // the shipped file; the reference instead leaves the entry at the default position 921.
    let compiled_switch = find_switch(&compiled.jump_table, 0, 8, 5, 364)
        .expect("compiled switch with resolved long alias");
    let reference_switch = find_switch(&reference.jump_table, 0, 8, 5, 364)
        .expect("shipped switch with resolved long alias");
    assert_eq!(compiled.jump_table[compiled_switch + 4 + 5], 364);
    assert_eq!(reference.jump_table[reference_switch + 4 + 5], 364);
    assert_eq!(
        compiled.jump_table[compiled_switch + 2],
        921,
        "compiled default case position"
    );
    println!(
        "S2/Mission_Zone02: long alias resolved by this port (case 5 -> 364, matching the shipped \
         bytecode); the reference C++ compiler truncates the name and leaves 921"
    );
}

/// Writes every compiled group in the canonical container so it can be compared with
/// `tools/script-oracle/run.sh --all`. Set `RETRO_DUMP_DIR` to override `target/compiler-groups`.
#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn dump_compiled_groups() {
    let root = std::env::var("RETRO_DUMP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/compiler-groups")
        });
    for game in GAMES {
        let directory = root.join(game);
        fs::create_dir_all(&directory).expect("create dump directory");
        for group in compiled_groups(game, PlatformMode::Origins) {
            let bytes = write_bytecode(&group.file).expect("serialise compiled group");
            fs::write(directory.join(format!("{}.bin", group.name)), bytes).expect("write group");
        }
        println!("wrote {game} groups to {}", directory.display());
    }
    println!("compare with: tools/script-oracle/run.sh <assets>/<game> --all <dir>");
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

        // Whole-group hashes for both modes, validated against the reference compiler.
        for (entry_game, name, origins_hash, standalone_hash) in PLATFORM_MODE_HASHES {
            if *entry_game != game {
                continue;
            }
            let origins = compile_named_group(game, name, PlatformMode::Origins);
            let standalone = compile_named_group(game, name, PlatformMode::Standalone);
            assert_eq!(
                blake3_of(&origins),
                *origins_hash,
                "{game}/{name}: Origins output changed"
            );
            assert_eq!(
                blake3_of(&standalone),
                *standalone_hash,
                "{game}/{name}: Standalone output changed"
            );
            assert_ne!(
                origins.code, standalone.code,
                "{game}/{name}: the two platform modes must compile different code"
            );
        }

        // Title/Start.txt: the Origins block checks `input.pressButton` (a global variable) while
        // the Standalone block checks `keyPress[0].buttonC` (variable 190, no array).
        let press_button = data
            .global_variables
            .iter()
            .position(|name| name == "input.pressButton")
            .expect("input.pressButton global variable") as i32;
        let title = data
            .stages
            .iter()
            .find(|stage| stage.name == "Title")
            .expect("Title stage");
        let title_symbols = symbols(&data, title.object_types.clone(), title.sfx_names.clone());
        let origins_start = extract_file_code(
            &title.files,
            "Start.txt",
            PlatformMode::Origins,
            title_symbols.clone(),
        );
        let standalone_start = extract_file_code(
            &title.files,
            "Start.txt",
            PlatformMode::Standalone,
            title_symbols,
        );
        assert!(
            contains(&origins_start, &[1, 1, 0, press_button, 17]),
            "{game}: Origins Start.txt must contain input.pressButton -> global[{press_button}]"
        );
        assert!(
            !contains(&origins_start, &[1, 1, 0, 0, 190]),
            "{game}: Origins Start.txt must not contain keyPress.buttonC"
        );
        assert!(
            contains(&standalone_start, &[1, 1, 0, 0, 190]),
            "{game}: Standalone Start.txt must contain keyPress.buttonC"
        );
        assert!(
            !contains(&standalone_start, &[1, 1, 0, press_button, 17]),
            "{game}: Standalone Start.txt must not contain input.pressButton"
        );
        assert_ne!(
            origins_start, standalone_start,
            "{game}: Title/Start.txt should differ between platform modes"
        );

        // Global/HUD.txt:
        // * Origins sets `temp4 = game.coinMode` (global variable) inside a `#platform: USE_ORIGINS`
        //   block; Standalone skips it.
        // * Standalone draws the minutes/seconds from `stage.seconds`; Origins uses a temp copy.
        let coin_mode = data
            .global_variables
            .iter()
            .position(|name| name == "game.coinMode")
            .expect("game.coinMode global variable") as i32;
        let origins_hud = extract_file_code(
            &data.global_files,
            "HUD.txt",
            PlatformMode::Origins,
            global_symbols.clone(),
        );
        let standalone_hud = extract_file_code(
            &data.global_files,
            "HUD.txt",
            PlatformMode::Standalone,
            global_symbols,
        );
        let origins_coin_mode = [1, 1, 0, 4, 1, 1, 0, coin_mode, 17];
        let standalone_seconds = [
            op(Op::DrawNumbers),
            2,
            0,
            2,
            80,
            2,
            29,
            1,
            0,
            126,
            2,
            2,
            2,
            8,
            2,
            1,
        ];
        assert!(
            contains(&origins_hud, &origins_coin_mode),
            "{game}: Origins HUD must contain the game.coinMode block"
        );
        assert!(
            !contains(&standalone_hud, &origins_coin_mode),
            "{game}: Standalone HUD must not contain the game.coinMode block"
        );
        assert!(
            contains(&standalone_hud, &standalone_seconds),
            "{game}: Standalone HUD must draw time from stage.seconds"
        );
        assert!(
            !contains(&origins_hud, &standalone_seconds),
            "{game}: Origins HUD must not draw time from stage.seconds"
        );
        assert_ne!(
            origins_hud, standalone_hud,
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
