//! Asset-gated integration tests against the local Sonic 1/2 asset trees.
//!
//! Run with `cargo test -p retro-script -- --ignored --nocapture`. The tests load every
//! `_Bytecode/*.bin` file in `/home/ted/projects/assets/{S1,S2}` through a `DirSource`, verify
//! that the canonical writer reproduces the original bytes exactly, and disassemble the whole
//! corpus.

#![forbid(unsafe_code)]

use retro_io::{DataSource, DirSource};
use retro_script::disasm::disassemble;
use retro_script::{
    Op, ScriptError, ScriptFile, ScriptHost, ScriptVersion, V4Revision, Vm, VmState, load_bytecode,
    write_bytecode,
};

const ASSET_ROOT: &str = "/home/ted/projects/assets";
const GAMES: [&str; 2] = ["S1", "S2"];

fn bytecode_files(game: &str) -> Vec<(String, Vec<u8>)> {
    let root = format!("{ASSET_ROOT}/{game}");
    let source =
        DirSource::new(&root).unwrap_or_else(|error| panic!("cannot open {root}: {error}"));
    source
        .enumerate("_Bytecode")
        .unwrap_or_else(|error| panic!("{game}: cannot enumerate _Bytecode: {error}"))
        .into_iter()
        .map(|path| {
            let bytes = source
                .read(&path)
                .unwrap_or_else(|error| panic!("{game}: cannot read {path}: {error}"));
            (path, bytes)
        })
        .collect()
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn loads_and_round_trips_every_bytecode_file() {
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut total_code_words = 0u64;
    let mut total_jump_words = 0u64;
    let mut total_functions = 0usize;
    let mut total_objects = 0usize;

    for game in GAMES {
        let files = bytecode_files(game);
        assert!(!files.is_empty(), "{game}: no bytecode files found");
        let mut game_bytes = 0u64;
        for (path, bytes) in &files {
            let file = load_bytecode(bytes)
                .unwrap_or_else(|error| panic!("{game}/{path}: load failed: {error}"));
            let written = write_bytecode(&file)
                .unwrap_or_else(|error| panic!("{game}/{path}: write failed: {error}"));
            assert_eq!(
                written, *bytes,
                "{game}/{path}: canonical writer is not byte-exact"
            );
            total_code_words += file.code.len() as u64;
            total_jump_words += file.jump_table.len() as u64;
            total_functions += file.functions.len();
            total_objects += file.object_scripts.len();
            game_bytes += bytes.len() as u64;
        }
        total_files += files.len();
        total_bytes += game_bytes;
        println!("{game}: {} files, {game_bytes} bytes", files.len());
    }

    println!(
        "totals: {total_files} files, {total_bytes} bytes, {total_code_words} code words, \
         {total_jump_words} jump words, {total_functions} functions, {total_objects} object scripts"
    );
    assert!(total_files >= 72, "expected at least 72 bytecode files");
    assert!(total_functions > 4000, "expected thousands of functions");
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn disassembles_every_bytecode_file() {
    let mut hasher = blake3::Hasher::new();
    let mut total_files = 0usize;
    let mut total_lines = 0usize;

    for game in GAMES {
        for (path, bytes) in bytecode_files(game) {
            let file = load_bytecode(&bytes).unwrap();
            let text = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
            assert!(!text.is_empty(), "{game}/{path}: empty disassembly");
            hasher.update(text.as_bytes());
            total_files += 1;
            total_lines += text.lines().count();
        }
    }

    let hash = hasher.finalize().to_hex().to_string();
    println!("disassembled {total_files} files, {total_lines} lines, aggregate blake3 {hash}");
    assert_eq!(
        hash, "b82cedfc7311e2a9c5d6de272e77f667d524baced9d4d872dc780b0856eb19e2",
        "aggregate disassembly hash changed; update the snapshot after reviewing the diff"
    );
}

fn disassembly_prefix(game: &str, name: &str, lines: usize) -> String {
    let files = bytecode_files(game);
    let (path, bytes) = files
        .iter()
        .find(|(path, _)| path.ends_with(name))
        .unwrap_or_else(|| panic!("{game}/{name}"));
    let file = load_bytecode(bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
    let text = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
    text.lines().take(lines).collect::<Vec<_>>().join("\n")
}

/// Inert host used by the execution smoke test: engine operations do nothing, engine variables
/// read as zero and `foreach` lists are empty.
#[derive(Default)]
struct NullHost;

impl ScriptHost for NullHost {
    fn engine_op(&mut self, _op: Op, _state: &mut VmState) -> Result<(), ScriptError> {
        Ok(())
    }

    fn foreach_next(
        &mut self,
        _op: Op,
        _selector: i32,
        _loop_index: i32,
        _state: &mut VmState,
    ) -> Result<Option<i32>, ScriptError> {
        Ok(None)
    }
}

/// Executes every function of every stage bytecode against the matching `GlobalCode.bin` prefix
/// and reports how many terminate. Reserved-but-undefined function slots and loops that depend
/// on engine variables may not terminate; the assertion is deliberately loose.
#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn vm_executes_real_functions() {
    let mut grand_total = 0usize;
    let mut grand_ok = 0usize;

    for game in GAMES {
        let files = bytecode_files(game);
        let (global_path, global_bytes) = files
            .iter()
            .find(|(path, _)| path.ends_with("GlobalCode.bin"))
            .unwrap_or_else(|| panic!("{game}/GlobalCode.bin"));
        let global = load_bytecode(global_bytes).unwrap();
        println!(
            "{game}/{global_path}: {} code words, {} functions",
            global.code.len(),
            global.functions.len()
        );

        let mut total = 0usize;
        let mut ok = 0usize;
        let mut limited = 0usize;
        let mut bad = 0usize;
        let mut invalid = 0usize;
        let mut truncated = 0usize;
        let mut invalid_state = 0usize;
        let mut first_errors: Vec<String> = Vec::new();
        for (path, bytes) in &files {
            if path.ends_with("GlobalCode.bin") {
                continue;
            }
            let stage = load_bytecode(bytes).unwrap();
            // Presentation stages are compiled standalone (positions fit inside the file);
            // everything else stores global-absolute positions and needs `GlobalCode.bin`
            // in front of it.
            let standalone = stage
                .functions
                .iter()
                .all(|function| (function.code_pos as usize) < stage.code.len())
                && stage
                    .object_scripts
                    .iter()
                    .flat_map(|script| [script.update, script.draw, script.startup])
                    .all(|ptr| {
                        ptr.code_pos == retro_script::EMPTY_EVENT
                            || (ptr.code_pos as usize) < stage.code.len()
                    });
            let combined = if standalone {
                stage.clone()
            } else {
                let mut combined = ScriptFile {
                    code: global.code.clone(),
                    jump_table: global.jump_table.clone(),
                    functions: stage.functions.clone(),
                    ..ScriptFile::default()
                };
                combined.code.extend_from_slice(&stage.code);
                combined.jump_table.extend_from_slice(&stage.jump_table);
                combined
            };
            let mut vm = Vm::new(combined);
            for index in 0..stage.functions.len() {
                total += 1;
                let mut state = VmState::default();
                match vm.call_with_limit(&mut NullHost, index, &mut state, 200_000) {
                    Ok(()) => ok += 1,
                    Err(ScriptError::InstructionLimit) => limited += 1,
                    Err(ScriptError::BadFunction) => bad += 1,
                    Err(ScriptError::InvalidOpcode) => invalid += 1,
                    Err(ScriptError::Truncated) => truncated += 1,
                    Err(error @ ScriptError::InvalidState(_)) => {
                        invalid_state += 1;
                        if first_errors.len() < 5 {
                            first_errors.push(format!("{path}#{index}: {error}"));
                        }
                    }
                    Err(ScriptError::HostError(_)) => {}
                }
            }
        }
        println!(
            "{game}: {total} stage functions: {ok} terminate, {limited} hit the limit, \
             {bad} bad entry points, {invalid} invalid opcodes, {truncated} truncated, \
             {invalid_state} invalid state"
        );
        for error in &first_errors {
            println!("{game}: first errors: {error}");
        }
        grand_total += total;
        grand_ok += ok;
    }

    println!("total: {grand_ok}/{grand_total} stage functions terminated");
    assert!(grand_total > 3000, "expected thousands of stage functions");
    assert!(
        grand_ok * 100 > grand_total * 95,
        "expected at least 95% of stage functions to terminate"
    );
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn real_file_disassembly_prefix_snapshots() {
    assert_eq!(
        disassembly_prefix("S1", "Credits.bin", 11),
        "\
; retro-script disassembly (v4/rev03)
; code=1628 jump_table=118 functions=0 object_scripts=3
; object 0: update(code=0, jump=0) draw(code=262143, jump=16383) startup(code=617, jump=52)
; object 1: update(code=697, jump=54) draw(code=1124, jump=89) startup(code=1129, jump=89)
; object 2: update(code=1576, jump=116) draw(code=262143, jump=16383) startup(code=1619, jump=118)
  0000: switch 0, object.state
  0006: Equal object.value0, 320
  000C: SetScreenFade 0, 0, 0, object.value0
  0016: Inc object.state
  001A: IfEqual 11, global[104], 0
  0024: PlayMusic 0"
    );
    assert_eq!(
        disassembly_prefix("S1", "Continue.bin", 12),
        "\
; retro-script disassembly (v4/rev03)
; code=1718 jump_table=125 functions=108 object_scripts=3
; object 0: update(code=59330, jump=3821) draw(code=60119, jump=3892) startup(code=60123, jump=3892)
; object 1: update(code=60349, jump=3909) draw(code=60404, jump=3915) startup(code=60618, jump=3927)
; object 2: update(code=60948, jump=3940) draw(code=60949, jump=3940) startup(code=60953, jump=3940)
  0000: switch 0, object.state
  0006: Equal object.value0, 320
  000C: SetScreenFade 0, 0, 0, object.value0
  0016: Inc object.state
  001A: break
  001B: IfGreater 12, object.value0, 0
  0023: Sub object.value0, 8"
    );
}
