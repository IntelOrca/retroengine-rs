//! RSDKv4 bytecode container: block encoding plus the object-script, function, jump-table and
//! code sections.
//!
//! Ported from `LoadBytecode` in `RSDKv4/Script.cpp` (RSDKModding/RSDKv4-Decompilation
//! @ a7f5195). The file layout is:
//!
//! ```text
//! u32 code_words
//! blocks of code words            (see below)
//! u32 jump_table_words
//! blocks of jump table words
//! u16 object_script_count
//! object_script_count * 3 u32     update/draw/startup code positions
//! object_script_count * 3 u32     update/draw/startup jump table positions
//! u16 function_count
//! function_count * u32            function code positions
//! function_count * u32            function jump table positions
//! ```
//!
//! Each block starts with a header byte. The low seven bits hold the entry count (1..=0x7F).
//! When bit `0x80` is set every entry is a little-endian `i32`, otherwise every entry is a
//! single byte zero-extended to `i32`. The canonical writer below groups runs of values in
//! `0..=255` as byte blocks and everything else as 32-bit blocks; that reproduces all shipped
//! `_Bytecode/*.bin` files byte-for-byte (see the asset-gated tests).
//!
//! Positions inside the file are absolute offsets into the *global* script code space that the
//! engine builds by loading `GlobalCode.bin` and then the stage file into one `scriptCode`
//! array. The loader preserves them; it never rebases.

use serde::Serialize;

use crate::error::ScriptError;

/// Upstream `SCRIPTCODE_COUNT`: size of the engine's global script code array.
pub const SCRIPTCODE_COUNT: usize = 0x40000;
/// Upstream `JUMPTABLE_COUNT`: size of the engine's global jump table array.
pub const JUMPTABLE_COUNT: usize = 0x4000;
/// Upstream `FUNCTION_COUNT`: size of the engine's script function table.
pub const FUNCTION_COUNT: usize = 0x200;
/// Upstream default entry point for an object event that has no script (`SCRIPTCODE_COUNT - 1`).
pub const EMPTY_EVENT: u32 = SCRIPTCODE_COUNT as u32 - 1;

/// Function visibility, mirroring upstream `ScriptVarAccessModifier` for functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum Access {
    Private,
    #[default]
    Public,
}

/// One entry of the script function table.
///
/// The bytecode format only stores [`ScriptFunction::code_pos`] and [`ScriptFunction::jump_pos`];
/// `name`, `access` and `is_native` come from the text compiler or the engine. Loaded bytecode
/// therefore has empty names, [`Access::Public`] and `is_native == false`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScriptFunction {
    pub name: String,
    pub access: Access,
    pub code_pos: u32,
    pub jump_pos: u32,
    pub is_native: bool,
}

/// A code/jump-table entry point pair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ScriptPtr {
    pub code_pos: u32,
    pub jump_pos: u32,
}

/// The three event entry points of one object script, in upstream order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ObjectScript {
    pub update: ScriptPtr,
    pub draw: ScriptPtr,
    pub startup: ScriptPtr,
}

/// A parsed bytecode file.
///
/// `code` and `jump_table` hold exactly the words stored in the file. `functions` is in
/// declaration order and matches the runtime function table; `code_pos`/`jump_pos` are the
/// absolute global positions stored in the bytecode.
///
/// `object_scripts`, `global_variables` and `values` are additive fields beyond the original
/// work-package sketch: `object_scripts` is required to round-trip the file and for the engine
/// to find event entry points, while `global_variables`/`values` are populated only by the text
/// compiler (upstream v4 bytecode does not serialise them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScriptFile {
    pub functions: Vec<ScriptFunction>,
    pub code: Vec<i32>,
    pub jump_table: Vec<u32>,
    pub global_variables: Vec<String>,
    pub values: Vec<String>,
    pub object_scripts: Vec<ObjectScript>,
}

/// Creates a [`ScriptFile`] with every vector empty.
pub fn new_empty() -> ScriptFile {
    ScriptFile::default()
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8, ScriptError> {
        let byte = *self.bytes.get(self.pos).ok_or(ScriptError::Truncated)?;
        self.pos += 1;
        Ok(byte)
    }

    fn u16(&mut self) -> Result<u16, ScriptError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, ScriptError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ScriptError> {
        let end = self.pos.checked_add(count).ok_or(ScriptError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(ScriptError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }
}

fn read_blocks_i32(cursor: &mut Cursor<'_>) -> Result<Vec<i32>, ScriptError> {
    let count = cursor.u32()? as usize;
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        let header = cursor.u8()?;
        let block = (header & 0x7F) as usize;
        if block == 0 {
            return Err(ScriptError::InvalidState(
                "zero-length bytecode block".to_string(),
            ));
        }
        if values.len() + block > count {
            return Err(ScriptError::InvalidState(
                "bytecode block overruns declared size".to_string(),
            ));
        }
        if header & 0x80 != 0 {
            for _ in 0..block {
                values.push(cursor.u32()? as i32);
            }
        } else {
            for _ in 0..block {
                values.push(cursor.u8()? as i32);
            }
        }
    }
    Ok(values)
}

fn read_blocks_u32(cursor: &mut Cursor<'_>) -> Result<Vec<u32>, ScriptError> {
    let count = cursor.u32()? as usize;
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        let header = cursor.u8()?;
        let block = (header & 0x7F) as usize;
        if block == 0 {
            return Err(ScriptError::InvalidState(
                "zero-length bytecode block".to_string(),
            ));
        }
        if values.len() + block > count {
            return Err(ScriptError::InvalidState(
                "bytecode block overruns declared size".to_string(),
            ));
        }
        if header & 0x80 != 0 {
            for _ in 0..block {
                values.push(cursor.u32()?);
            }
        } else {
            for _ in 0..block {
                values.push(cursor.u8()? as u32);
            }
        }
    }
    Ok(values)
}

/// Loads a bytecode file exactly as upstream `LoadBytecode` reads it.
pub fn load_bytecode(bytes: &[u8]) -> Result<ScriptFile, ScriptError> {
    let mut cursor = Cursor::new(bytes);

    let code = read_blocks_i32(&mut cursor)?;
    let jump_table = read_blocks_u32(&mut cursor)?;

    let script_count = cursor.u16()? as usize;
    let mut object_scripts = vec![ObjectScript::default(); script_count];
    for script in &mut object_scripts {
        script.update.code_pos = cursor.u32()?;
        script.draw.code_pos = cursor.u32()?;
        script.startup.code_pos = cursor.u32()?;
    }
    for script in &mut object_scripts {
        script.update.jump_pos = cursor.u32()?;
        script.draw.jump_pos = cursor.u32()?;
        script.startup.jump_pos = cursor.u32()?;
    }

    let function_count = cursor.u16()? as usize;
    let mut functions = Vec::with_capacity(function_count);
    for _ in 0..function_count {
        functions.push(ScriptFunction {
            name: String::new(),
            access: Access::Public,
            code_pos: cursor.u32()?,
            jump_pos: 0,
            is_native: false,
        });
    }
    for function in &mut functions {
        function.jump_pos = cursor.u32()?;
    }

    Ok(ScriptFile {
        functions,
        code,
        jump_table,
        global_variables: Vec::new(),
        values: Vec::new(),
        object_scripts,
    })
}

/// Appends one canonical block encoding of `values`.
fn write_blocks(values: &[i32], out: &mut Vec<u8>) {
    let mut index = 0;
    while index < values.len() {
        let byte_block = (0..=255).contains(&values[index]);
        let start = index;
        while index < values.len()
            && index - start < 0x7F
            && ((0..=255).contains(&values[index]) == byte_block)
        {
            index += 1;
        }
        let count = (index - start) as u8;
        if byte_block {
            out.push(count);
            for value in &values[start..index] {
                out.push(*value as u8);
            }
        } else {
            out.push(0x80 | count);
            for value in &values[start..index] {
                out.extend_from_slice(&(*value as u32).to_le_bytes());
            }
        }
    }
}

/// Serialises a [`ScriptFile`] using the canonical block grouping.
///
/// `name`, `access`, `is_native`, `global_variables` and `values` are not part of the v4
/// bytecode format and are not written. Loading a shipped `_Bytecode/*.bin` and writing it back
/// reproduces the original bytes exactly.
pub fn write_bytecode(file: &ScriptFile) -> Result<Vec<u8>, ScriptError> {
    let code_size = u32::try_from(file.code.len())
        .map_err(|_| ScriptError::InvalidState("code table too large".to_string()))?;
    let jump_size = u32::try_from(file.jump_table.len())
        .map_err(|_| ScriptError::InvalidState("jump table too large".to_string()))?;
    let script_count = u16::try_from(file.object_scripts.len())
        .map_err(|_| ScriptError::InvalidState("too many object scripts".to_string()))?;
    let function_count = u16::try_from(file.functions.len())
        .map_err(|_| ScriptError::InvalidState("too many functions".to_string()))?;

    let mut out = Vec::new();
    out.extend_from_slice(&code_size.to_le_bytes());
    write_blocks(&file.code, &mut out);

    out.extend_from_slice(&jump_size.to_le_bytes());
    let jump_values: Vec<i32> = file.jump_table.iter().map(|value| *value as i32).collect();
    write_blocks(&jump_values, &mut out);

    out.extend_from_slice(&script_count.to_le_bytes());
    for script in &file.object_scripts {
        out.extend_from_slice(&script.update.code_pos.to_le_bytes());
        out.extend_from_slice(&script.draw.code_pos.to_le_bytes());
        out.extend_from_slice(&script.startup.code_pos.to_le_bytes());
    }
    for script in &file.object_scripts {
        out.extend_from_slice(&script.update.jump_pos.to_le_bytes());
        out.extend_from_slice(&script.draw.jump_pos.to_le_bytes());
        out.extend_from_slice(&script.startup.jump_pos.to_le_bytes());
    }

    out.extend_from_slice(&function_count.to_le_bytes());
    for function in &file.functions {
        out.extend_from_slice(&function.code_pos.to_le_bytes());
    }
    for function in &file.functions {
        out.extend_from_slice(&function.jump_pos.to_le_bytes());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> ScriptFile {
        ScriptFile {
            functions: vec![
                ScriptFunction {
                    name: "Main".to_string(),
                    access: Access::Public,
                    code_pos: 0,
                    jump_pos: 0,
                    is_native: false,
                },
                ScriptFunction {
                    name: "Helper".to_string(),
                    access: Access::Private,
                    code_pos: 8,
                    jump_pos: 4,
                    is_native: false,
                },
            ],
            code: vec![0, 1, 2, 255, 256, -1, 0x1234_5678, 7, 0, 2, 0, 9],
            jump_table: vec![0, 4, u32::MAX, 262_143],
            global_variables: vec!["lives".to_string()],
            values: vec!["SCORE".to_string()],
            object_scripts: vec![ObjectScript {
                update: ScriptPtr {
                    code_pos: 0,
                    jump_pos: 0,
                },
                draw: ScriptPtr {
                    code_pos: EMPTY_EVENT,
                    jump_pos: EMPTY_EVENT,
                },
                startup: ScriptPtr {
                    code_pos: 8,
                    jump_pos: 4,
                },
            }],
        }
    }

    #[test]
    fn empty_file_round_trips() {
        let file = new_empty();
        let bytes = write_bytecode(&file).unwrap();
        assert_eq!(bytes, vec![0; 12]);
        let loaded = load_bytecode(&bytes).unwrap();
        assert_eq!(loaded, file);
    }

    #[test]
    fn hand_written_minimal_bytecode() {
        // code = [End, 300, -2], jump = [9], one object script, one function.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.push(1);
        bytes.push(0);
        bytes.push(0x82);
        bytes.extend_from_slice(&300i32.to_le_bytes());
        bytes.extend_from_slice(&(-2i32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&EMPTY_EVENT.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&EMPTY_EVENT.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());

        let file = load_bytecode(&bytes).unwrap();
        assert_eq!(file.code, vec![0, 300, -2]);
        assert_eq!(file.jump_table, vec![9]);
        assert_eq!(file.object_scripts.len(), 1);
        assert_eq!(file.object_scripts[0].draw.code_pos, EMPTY_EVENT);
        assert_eq!(file.object_scripts[0].startup.jump_pos, 5);
        assert_eq!(file.functions.len(), 1);
        assert_eq!(file.functions[0].code_pos, 0);
        assert_eq!(file.functions[0].jump_pos, 3);
        assert_eq!(file.functions[0].access, Access::Public);
        assert!(!file.functions[0].is_native);
        assert!(file.functions[0].name.is_empty());
    }

    #[test]
    fn canonical_writer_round_trips_blocks_and_values() {
        let file = sample_file();
        let bytes = write_bytecode(&file).unwrap();
        let loaded = load_bytecode(&bytes).unwrap();
        assert_eq!(loaded.code, file.code);
        assert_eq!(loaded.jump_table, file.jump_table);
        assert_eq!(loaded.object_scripts, file.object_scripts);
        assert_eq!(loaded.functions.len(), file.functions.len());
        for (a, b) in loaded.functions.iter().zip(&file.functions) {
            assert_eq!(a.code_pos, b.code_pos);
            assert_eq!(a.jump_pos, b.jump_pos);
        }
        // Rewriting the loaded file is idempotent.
        assert_eq!(write_bytecode(&loaded).unwrap(), bytes);
    }

    #[test]
    fn byte_runs_split_at_seven_bit_boundary() {
        let file = ScriptFile {
            code: (0..=0x100).collect(),
            ..new_empty()
        };
        let bytes = write_bytecode(&file).unwrap();
        let loaded = load_bytecode(&bytes).unwrap();
        assert_eq!(loaded.code, file.code);
        // First block: 0x7F bytes (0..=0x7E), second block: 0x7F bytes (0x7F..=0xFD),
        // then the remaining byte-run and the 4-byte value 0x100.
        assert_eq!(bytes[4], 0x7F);
        assert_eq!(bytes[4 + 1 + 0x7F], 0x7F);
    }

    #[test]
    fn zero_length_block_is_rejected() {
        let bytes = [1, 0, 0, 0, 0];
        assert!(matches!(
            load_bytecode(&bytes),
            Err(ScriptError::InvalidState(_))
        ));
    }

    #[test]
    fn block_overrun_is_rejected() {
        let bytes = [1, 0, 0, 0, 2, 0, 0];
        assert!(matches!(
            load_bytecode(&bytes),
            Err(ScriptError::InvalidState(_))
        ));
    }

    #[test]
    fn truncation_is_reported() {
        assert!(matches!(load_bytecode(&[]), Err(ScriptError::Truncated)));
        assert!(matches!(
            load_bytecode(&[1, 0, 0, 0]),
            Err(ScriptError::Truncated)
        ));
        // Header says two 4-byte entries but only one is present.
        assert!(matches!(
            load_bytecode(&[2, 0, 0, 0, 0x82, 0, 0, 0, 0]),
            Err(ScriptError::Truncated)
        ));
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let file = new_empty();
        let mut bytes = write_bytecode(&file).unwrap();
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(load_bytecode(&bytes).unwrap(), file);
    }

    #[test]
    fn too_many_functions_is_rejected() {
        let file = ScriptFile {
            functions: vec![ScriptFunction::default(); u16::MAX as usize + 1],
            ..new_empty()
        };
        assert!(matches!(
            write_bytecode(&file),
            Err(ScriptError::InvalidState(_))
        ));
    }
}
