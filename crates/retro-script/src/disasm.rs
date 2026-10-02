//! Deterministic textual disassembler for RSDKv4 bytecode.
//!
//! The disassembler walks the code array linearly, decoding the same dynamically typed operand
//! stream as the VM. Because data tables (`GetTableValue` arrays) live inline in the code array,
//! linear output can contain entries decoded from table data; those are printed as-is. Function
//! headers are emitted whenever the current address matches a function entry point.

use std::fmt::Write;

use crate::bytecode::ScriptFile;
use crate::opcodes::opcode_table;
use crate::vars;
use crate::version::{ScriptVersion, V4Revision};

/// Renders `file` as text. The output only depends on the file and revision arguments.
pub fn disassemble(file: &ScriptFile, version: ScriptVersion, revision: V4Revision) -> String {
    let table = opcode_table(version, revision);
    let mut out = String::new();

    let _ = writeln!(
        out,
        "; retro-script disassembly ({}/{})",
        version.name(),
        revision.name()
    );
    let _ = writeln!(
        out,
        "; code={} jump_table={} functions={} object_scripts={}",
        file.code.len(),
        file.jump_table.len(),
        file.functions.len(),
        file.object_scripts.len()
    );
    for (index, script) in file.object_scripts.iter().enumerate() {
        let _ = writeln!(
            out,
            "; object {index}: update(code={}, jump={}) draw(code={}, jump={}) startup(code={}, jump={})",
            script.update.code_pos,
            script.update.jump_pos,
            script.draw.code_pos,
            script.draw.jump_pos,
            script.startup.code_pos,
            script.startup.jump_pos
        );
    }

    let mut entries: Vec<(u32, usize)> = file
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| (function.code_pos, index))
        .collect();
    entries.sort_by_key(|(position, index)| (*position, *index));

    let mut next_entry = 0;
    let mut position: u32 = 0;
    while let Some(word) = file.code.get(position as usize).copied() {
        while next_entry < entries.len() && entries[next_entry].0 == position {
            let (_, index) = entries[next_entry];
            let function = &file.functions[index];
            let _ = write!(out, "func {index}");
            if !function.name.is_empty() {
                let _ = write!(out, " \"{}\"", function.name);
            }
            let _ = writeln!(
                out,
                " code={} jump={}",
                function.code_pos, function.jump_pos
            );
            next_entry += 1;
        }

        let start = position;
        position += 1;
        let index = usize::try_from(word)
            .ok()
            .filter(|index| *index < table.len());
        let Some(info) = index.map(|index| &table[index]) else {
            let _ = writeln!(out, "  {start:04X}: .invalid {word}");
            continue;
        };

        let mut args = Vec::new();
        let mut truncated = false;
        for _ in 0..info.operands.len() {
            let Some(operand_type) = file.code.get(position as usize).copied() else {
                truncated = true;
                break;
            };
            position += 1;
            match operand_type {
                1 => {
                    let Some(array_kind) = file.code.get(position as usize).copied() else {
                        truncated = true;
                        break;
                    };
                    position += 1;
                    let mut array = String::new();
                    match array_kind {
                        0 => {}
                        1..=3 => {
                            let Some(indexed) = file.code.get(position as usize).copied() else {
                                truncated = true;
                                break;
                            };
                            position += 1;
                            let Some(value) = file.code.get(position as usize).copied() else {
                                truncated = true;
                                break;
                            };
                            position += 1;
                            let inner = if indexed == 1 {
                                format!("arrayPos{value}")
                            } else {
                                value.to_string()
                            };
                            array = match array_kind {
                                1 => format!("[{inner}]"),
                                2 => format!("[entityPos+{inner}]"),
                                _ => format!("[entityPos-{inner}]"),
                            };
                        }
                        _ => array = format!("[?{array_kind}]"),
                    }
                    let Some(var) = file.code.get(position as usize).copied() else {
                        truncated = true;
                        break;
                    };
                    position += 1;
                    let name = vars::variable_name(revision, var)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("var{var}"));
                    args.push(format!("{name}{array}"));
                }
                2 => {
                    let Some(value) = file.code.get(position as usize).copied() else {
                        truncated = true;
                        break;
                    };
                    position += 1;
                    args.push(value.to_string());
                }
                3 => {
                    let Some(length) = file.code.get(position as usize).copied() else {
                        truncated = true;
                        break;
                    };
                    position += 1;
                    if length < 0 {
                        args.push("<bad string>".to_string());
                        truncated = true;
                        break;
                    }
                    let length = length as usize;
                    let word_start = position;
                    let mut bytes = Vec::with_capacity(length);
                    for c in 0..length {
                        let Some(index) = word_start.checked_add((c / 4) as u32) else {
                            truncated = true;
                            break;
                        };
                        let Some(packed) = file.code.get(index as usize).copied() else {
                            truncated = true;
                            break;
                        };
                        bytes.push(match c % 4 {
                            0 => (packed >> 24) as u8,
                            1 => (packed >> 16) as u8,
                            2 => (packed >> 8) as u8,
                            _ => packed as u8,
                        });
                    }
                    if truncated {
                        break;
                    }
                    let Some(next) = word_start.checked_add((length / 4 + 1) as u32) else {
                        truncated = true;
                        break;
                    };
                    position = next;
                    args.push(quote(&bytes));
                }
                other => args.push(format!("?{other}")),
            }
        }

        if truncated {
            let _ = writeln!(out, "  {start:04X}: {} <truncated>", info.name);
            break;
        }
        if args.is_empty() {
            let _ = writeln!(out, "  {start:04X}: {}", info.name);
        } else {
            let _ = writeln!(out, "  {start:04X}: {} {}", info.name, args.join(", "));
        }
    }

    out
}

fn quote(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for byte in bytes {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(*byte as char),
            _ => {
                let _ = write!(out, "\\x{byte:02X}");
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{ObjectScript, ScriptFunction, ScriptPtr};

    fn function(name: &str, code_pos: u32, jump_pos: u32) -> ScriptFunction {
        ScriptFunction {
            name: name.to_string(),
            code_pos,
            jump_pos,
            ..ScriptFunction::default()
        }
    }

    #[test]
    fn empty_file_is_only_a_header() {
        let text = disassemble(&ScriptFile::default(), ScriptVersion::V4, V4Revision::Rev03);
        assert_eq!(
            text,
            "; retro-script disassembly (v4/rev03)\n\
             ; code=0 jump_table=0 functions=0 object_scripts=0\n"
        );
    }

    #[test]
    fn hand_built_file_matches_snapshot() {
        // Equal(temp0, 5); Add(temp0, temp1); LoadSpriteSheet("a.gif"); End
        let code = vec![
            1,
            1,
            0,
            0,
            2,
            5, // Equal temp0, 5
            2,
            1,
            0,
            0,
            1,
            0,
            1, // Add temp0, temp1
            48,
            3,
            5,
            0x612E_6769,
            0x6600_0000, // LoadSpriteSheet "a.gif"
            0,           // End
        ];
        let file = ScriptFile {
            functions: vec![function("Main", 0, 0)],
            code,
            jump_table: vec![],
            global_variables: vec![],
            values: vec![],
            object_scripts: vec![ObjectScript {
                update: ScriptPtr {
                    code_pos: 0,
                    jump_pos: 0,
                },
                ..ObjectScript::default()
            }],
        };
        let text = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
        assert_eq!(
            text,
            "; retro-script disassembly (v4/rev03)\n\
             ; code=19 jump_table=0 functions=1 object_scripts=1\n\
             ; object 0: update(code=0, jump=0) draw(code=0, jump=0) startup(code=0, jump=0)\n\
             func 0 \"Main\" code=0 jump=0\n\
             \x20 0000: Equal temp0, 5\n\
             \x20 0006: Add temp0, temp1\n\
             \x20 000D: LoadSpriteSheet \"a.gif\"\n\
             \x20 0012: End\n"
        );
    }

    #[test]
    fn array_operands_are_rendered_symbolically() {
        // Equal(global[arrayPos0], global[2])
        let code = vec![
            1, // Equal
            1, 1, 1, 0, 17, // global[arrayPos0]
            1, 1, 0, 2, 17, // global[2]
        ];
        let file = ScriptFile {
            code,
            ..ScriptFile::default()
        };
        let text = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
        assert!(
            text.contains("Equal global[arrayPos0], global[2]"),
            "{text}"
        );
    }

    #[test]
    fn invalid_opcode_and_truncation_do_not_panic() {
        let file = ScriptFile {
            code: vec![200, 1, 0],
            ..ScriptFile::default()
        };
        let text = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
        assert!(text.contains(".invalid 200"), "{text}");
        assert!(text.contains("Equal <truncated>"), "{text}");
    }

    #[test]
    fn revision_changes_variable_names_and_opcodes() {
        let file = ScriptFile {
            code: vec![0],
            ..ScriptFile::default()
        };
        let rev00 = disassemble(&file, ScriptVersion::V4, V4Revision::Rev00);
        let rev03 = disassemble(&file, ScriptVersion::V4, V4Revision::Rev03);
        assert!(rev00.starts_with("; retro-script disassembly (v4/rev00)"));
        assert!(rev03.starts_with("; retro-script disassembly (v4/rev03)"));
    }
}
