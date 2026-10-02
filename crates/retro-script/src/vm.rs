//! RSDKv4 script virtual machine.
//!
//! Ported from `ProcessScript` in `RSDKv4/Script.cpp` (RSDKModding/RSDKv4-Decompilation
//! @ a7f5195). The VM decodes the dynamically typed operand stream (upstream `ScriptVarTypes`:
//! `VAR`, `INTCONST`, `STRCONST`), executes control flow, wrapping arithmetic, comparisons,
//! array/table access, `foreach`, function calls and returns, and hands every engine operation
//! to [`ScriptHost::engine_op`].
//!
//! Script variables that live purely in the interpreter (`temp0`..`temp7`, `checkResult`,
//! `arrayPos0`..`arrayPos7`, `global`, `local`) are resolved internally; every other variable
//! (`object.*`, `stage.*`, `screen.*`, input, audio, ...) goes through
//! [`ScriptHost::read_engine_var`] / [`ScriptHost::write_engine_var`]. `foreach` iteration goes
//! through [`ScriptHost::foreach_next`] because the object lists are engine state.
//!
//! Like upstream, the VM performs a "Set Values" pass after most operations that writes the
//! (possibly modified) operand values back to their decoded destinations. [`crate::opcodes::op_writes_back`]
//! lists the operations that skip that pass (control flow and engine operations that consume
//! their operands).
//!
//! Operations implemented by the VM itself: control flow (`End`, `If*`, `W*`, `else`, `endif`,
//! `loop`, `next`, `switch`, `break`, `endswitch`, `ForEach*`, `CallFunction`, `return`), pure
//! arithmetic/comparison (`Equal`, `Add`..`FlipSign`, `Not`, `Abs`, `Check*`), `Interpolate`,
//! `InterpolateXY`, and table access (`GetTableValue`, `SetTableValue`). Everything else -
//! including `Rand`, `Sin`, `Cos`, `ATan2`, drawing, audio, input and 3D operations - goes to
//! [`ScriptHost::engine_op`].
//!
//! # Divergences from upstream
//!
//! * Upstream's `scriptCode` is a zero-initialised `SCRIPTCODE_COUNT` array, so reads past the
//!   used region see `End` (0). This VM is strict: reads and jumps outside the loaded `code`
//!   return [`ScriptError::Truncated`] / [`ScriptError::BadFunction`].
//! * Division or modulo by zero returns [`ScriptError::InvalidState`] instead of trapping, and
//!   `i32::MIN / -1` wraps to `i32::MIN` instead of being undefined behaviour.
//! * Shift counts are masked (`wrapping_shl`/`wrapping_shr`) instead of being undefined
//!   behaviour for counts outside `0..32`.

use serde::Serialize;

use crate::bytecode::ScriptFile;
use crate::error::ScriptError;
use crate::opcodes::{self, Op, opcode_table};
use crate::vars;
use crate::version::{ScriptVersion, V4Revision};

/// Default per-call instruction budget, chosen well above any real event script.
pub const DEFAULT_INSTRUCTION_LIMIT: u64 = 1 << 22;

/// Maximum nesting depth for jump-table, function and foreach stacks (upstream `*_COUNT`).
const MAX_STACK: usize = 0x400;

/// One entry of the persistent `foreach` stack: the last candidate index tried at that nesting
/// level, or `-1` when the level is unused.
pub type ForeachStackEntry = i32;

/// Per-engine copy of the upstream `ScriptEngine` registers.
///
/// Upstream v4 keeps a single engine; [`VmState::script_engines`] exists for parity with engines
/// that keep an array of these. The VM executes against the top-level [`VmState`] fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ScriptEngineState {
    pub operands: [i32; 16],
    pub temp: [i32; 8],
    pub array_position: [i32; 9],
    pub check_result: i32,
}

/// Execution state shared between the VM and the host.
///
/// This mirrors upstream's `ScriptEngine` plus the globals that affect script results:
/// `scriptText`, the `globalVariables` array and the persistent `foreachStack`.
///
/// `script_text` and `script_engines` are additive fields beyond the original work-package
/// sketch: string constants decoded by the VM are exposed to engine operations here, and
/// `script_engines` mirrors upstream multi-engine hosts (v4 itself keeps only one engine).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VmState {
    /// Upstream `scriptEng.operands[0x10]`; operand `i` of the current instruction.
    pub operands: [i32; 16],
    /// Upstream `scriptEng.temp[8]`.
    pub temp: [i32; 8],
    /// Upstream `scriptEng.arrayPosition[9]`.
    pub array_position: [i32; 9],
    /// Upstream `scriptEng.checkResult`.
    pub check_result: i32,
    /// Upstream `globalVariables`; length is an engine concern.
    pub global_variables: Vec<i32>,
    /// Per-script engine register sets, for hosts that mirror upstream's engine array.
    pub script_engines: Vec<ScriptEngineState>,
    /// Persistent `foreachStack`, indexed by nesting depth (entry 0 is unused, as upstream).
    pub foreach_stack: Vec<ForeachStackEntry>,
    /// Upstream global `scriptText`; set by `STRCONST` operand decoding.
    pub script_text: String,
}

/// Engine-side operations required by the interpreter.
///
/// Only [`ScriptHost::engine_op`] is mandatory. The remaining callbacks have inert defaults so
/// that a host can start with just engine operations and add variable/foreach support as the
/// engine grows.
pub trait ScriptHost {
    /// Executes an engine operation. Decoded operands are in `state.operands[0..]` in upstream
    /// order (operand 0 is usually the destination/result); results are read back from there and
    /// written to the destination variables when [`crate::opcodes::op_writes_back`] is true.
    fn engine_op(&mut self, op: Op, state: &mut VmState) -> Result<(), ScriptError>;

    /// Reads an engine-owned variable. `array_index` is the resolved array/entity index (the
    /// value of `objectEntityPos` for `VARARR_NONE`). The default returns `0`.
    fn read_engine_var(
        &mut self,
        var: i32,
        array_index: i32,
        state: &mut VmState,
    ) -> Result<i32, ScriptError> {
        let _ = (var, array_index, state);
        Ok(0)
    }

    /// Writes an engine-owned variable. The default ignores the write.
    fn write_engine_var(
        &mut self,
        var: i32,
        array_index: i32,
        value: i32,
        state: &mut VmState,
    ) -> Result<(), ScriptError> {
        let _ = (var, array_index, value, state);
        Ok(())
    }

    /// Returns the entity slot used as the implicit array index (`objectEntityPos`).
    fn object_entity_pos(&self) -> i32 {
        0
    }

    /// Returns the entity reference for candidate `loop_index` of a `foreach` loop, or `None`
    /// when the candidate is past the end of the list.
    ///
    /// `op` is [`Op::ForEachActive`] (iterate a type group) or [`Op::ForEachAll`] (iterate all
    /// entities of `selector`). `loop_index` starts at 0 for each loop execution. The default
    /// reports an empty list, which makes the loop exit immediately.
    fn foreach_next(
        &mut self,
        op: Op,
        selector: i32,
        loop_index: i32,
        state: &mut VmState,
    ) -> Result<Option<i32>, ScriptError> {
        let _ = (op, selector, loop_index, state);
        Ok(None)
    }
}

/// Interpreter over one [`ScriptFile`].
///
/// The VM executes the v4 revision selected at construction ([`V4Revision::Rev03`] by default,
/// matching the Origins assets). Call [`Vm::call`] once per object event per frame, passing the
/// host and the persistent [`VmState`].
#[derive(Debug, Clone)]
pub struct Vm {
    file: ScriptFile,
    version: ScriptVersion,
    revision: V4Revision,
}

/// Current function frame: absolute positions into `code` and `jump_table`.
#[derive(Debug, Clone, Copy)]
struct Frame {
    code_start: u32,
    jump_start: u32,
    code_pos: u32,
}

/// Per-call stacks, reset by every [`Vm::call`] like upstream `ProcessScript`.
#[derive(Debug, Default)]
struct Stacks {
    jump: Vec<u32>,
    calls: Vec<Frame>,
    foreach_pos: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Continue,
    Stop,
}

impl Vm {
    /// Creates a VM for v4 revision rev03 (Origins), the revision used by the shipped assets.
    pub fn new(file: ScriptFile) -> Self {
        Self::with_revision(file, ScriptVersion::V4, V4Revision::Rev03)
    }

    /// Creates a VM for an explicit script version/revision.
    pub fn with_revision(file: ScriptFile, version: ScriptVersion, revision: V4Revision) -> Self {
        Self {
            file,
            version,
            revision,
        }
    }

    /// The file backing this VM.
    pub fn file(&self) -> &ScriptFile {
        &self.file
    }

    /// Finds a function by its upstream name (exact, case-sensitive match).
    pub fn find_function(&self, name: &str) -> Option<usize> {
        self.file
            .functions
            .iter()
            .position(|function| function.name == name)
    }

    /// Executes `function` until it ends or returns to the top level, with the default
    /// instruction budget.
    pub fn call(
        &mut self,
        host: &mut dyn ScriptHost,
        function: usize,
        state: &mut VmState,
    ) -> Result<(), ScriptError> {
        self.call_with_limit(host, function, state, DEFAULT_INSTRUCTION_LIMIT)
    }

    /// Executes `function` with an explicit instruction budget.
    ///
    /// Returns [`ScriptError::InstructionLimit`] when the budget is exhausted; this is the
    /// runaway-loop guard required for untrusted bytecode.
    pub fn call_with_limit(
        &mut self,
        host: &mut dyn ScriptHost,
        function: usize,
        state: &mut VmState,
        instruction_limit: u64,
    ) -> Result<(), ScriptError> {
        let (entry_code, entry_jump) = self
            .file
            .functions
            .get(function)
            .map(|entry| (entry.code_pos, entry.jump_pos))
            .ok_or(ScriptError::BadFunction)?;
        self.validate_entry(entry_code)?;
        self.validate_entry(entry_jump)?;

        let mut frame = Frame {
            code_start: entry_code,
            jump_start: entry_jump,
            code_pos: entry_code,
        };
        let mut stacks = Stacks::default();
        let table = opcode_table(self.version, self.revision);
        let mut executed: u64 = 0;

        while executed < instruction_limit {
            executed += 1;

            let opcode_pos = frame.code_pos;
            let raw = self.code_word(opcode_pos)?;
            let index = usize::try_from(raw)
                .ok()
                .filter(|index| *index < table.len())
                .ok_or(ScriptError::InvalidOpcode)?;
            let info = &table[index];
            frame.code_pos = frame.code_pos.wrapping_add(1);
            let operands_start = frame.code_pos;

            state.script_text.clear();
            self.decode_operands(host, state, &mut frame.code_pos, info.operands.len())?;

            let outcome = self.execute(host, state, info.op, &mut frame, &mut stacks)?;

            if opcodes::op_writes_back(info.op) && !info.operands.is_empty() {
                let mut ptr = operands_start;
                self.write_back_operands(host, state, &mut ptr, info.operands.len())?;
            }

            if outcome == Outcome::Stop {
                return Ok(());
            }
        }

        Err(ScriptError::InstructionLimit)
    }

    fn validate_entry(&self, position: u32) -> Result<(), ScriptError> {
        if usize::try_from(position).is_ok_and(|pos| pos <= self.file.code.len()) {
            Ok(())
        } else {
            Err(ScriptError::BadFunction)
        }
    }

    fn code_word(&self, position: u32) -> Result<i32, ScriptError> {
        let index = usize::try_from(position).map_err(|_| ScriptError::Truncated)?;
        self.file
            .code
            .get(index)
            .copied()
            .ok_or(ScriptError::Truncated)
    }

    fn jump_word(&self, index: u32) -> Result<i32, ScriptError> {
        let index = usize::try_from(index).map_err(|_| ScriptError::Truncated)?;
        self.file
            .jump_table
            .get(index)
            .copied()
            .map(|value| value as i32)
            .ok_or(ScriptError::Truncated)
    }

    fn read_word(&self, ptr: &mut u32) -> Result<i32, ScriptError> {
        let word = self.code_word(*ptr)?;
        *ptr = ptr.wrapping_add(1);
        Ok(word)
    }

    /// Resolves the array index part of a `VAR` operand (upstream `ScriptVarArrTypes`).
    fn decode_array_index(
        &self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        ptr: &mut u32,
    ) -> Result<i32, ScriptError> {
        let array_kind = self.read_word(ptr)?;
        let entity_pos = || host.object_entity_pos();
        match array_kind {
            0 => Ok(entity_pos()),
            1..=3 => {
                let indexed = self.read_word(ptr)?;
                let value = if indexed == 1 {
                    let array_pos = self.read_word(ptr)?;
                    usize::try_from(array_pos)
                        .ok()
                        .and_then(|pos| state.array_position.get(pos).copied())
                        .unwrap_or(0)
                } else {
                    self.read_word(ptr)?
                };
                Ok(match array_kind {
                    1 => value,
                    2 => entity_pos().wrapping_add(value),
                    _ => entity_pos().wrapping_sub(value),
                })
            }
            _ => Ok(0),
        }
    }

    /// Decodes `count` operands into `state.operands`, mirroring upstream's "Get Values" pass.
    fn decode_operands(
        &self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        ptr: &mut u32,
        count: usize,
    ) -> Result<(), ScriptError> {
        for i in 0..count {
            let operand_type = self.read_word(ptr)?;
            match operand_type {
                1 => {
                    let array_index = self.decode_array_index(host, state, ptr)?;
                    let var = self.read_word(ptr)?;
                    state.operands[i] = self.read_variable(host, state, var, array_index)?;
                }
                2 => {
                    state.operands[i] = self.read_word(ptr)?;
                }
                3 => {
                    let length = self.read_word(ptr)?;
                    if length < 0 {
                        return Err(ScriptError::InvalidState(
                            "negative string constant length".to_string(),
                        ));
                    }
                    let length = length as usize;
                    let start = *ptr;
                    let mut bytes = Vec::with_capacity(length);
                    for c in 0..length {
                        let index = start
                            .checked_add((c / 4) as u32)
                            .ok_or(ScriptError::Truncated)?;
                        let word = self.code_word(index)?;
                        let byte = match c % 4 {
                            0 => (word >> 24) as u8,
                            1 => (word >> 16) as u8,
                            2 => (word >> 8) as u8,
                            _ => word as u8,
                        };
                        bytes.push(byte);
                    }
                    *ptr = start
                        .checked_add((length / 4 + 1) as u32)
                        .ok_or(ScriptError::Truncated)?;
                    state.script_text = String::from_utf8_lossy(&bytes).into_owned();
                }
                // Upstream ignores unknown operand types, leaving the previous operand value.
                _ => {}
            }
        }
        Ok(())
    }

    /// Writes modified operand values back to their destinations, mirroring upstream's
    /// "Set Values" pass (including re-resolving array indices at write time).
    fn write_back_operands(
        &mut self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        ptr: &mut u32,
        count: usize,
    ) -> Result<(), ScriptError> {
        for i in 0..count {
            let operand_type = self.read_word(ptr)?;
            match operand_type {
                1 => {
                    let array_index = self.decode_array_index(host, state, ptr)?;
                    let var = self.read_word(ptr)?;
                    let value = state.operands[i];
                    self.write_variable(host, state, var, array_index, value)?;
                }
                2 => {
                    self.read_word(ptr)?;
                }
                3 => {
                    let length = self.read_word(ptr)?;
                    if length < 0 {
                        return Err(ScriptError::InvalidState(
                            "negative string constant length".to_string(),
                        ));
                    }
                    *ptr = ptr.wrapping_add((length as u32) / 4 + 1);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads a script variable by upstream `ScrVar` id.
    fn read_variable(
        &self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        var: i32,
        array_index: i32,
    ) -> Result<i32, ScriptError> {
        match var {
            vars::VAR_TEMP0..=vars::VAR_TEMP7 => Ok(state.temp[(var - vars::VAR_TEMP0) as usize]),
            vars::VAR_CHECK_RESULT => Ok(state.check_result),
            vars::VAR_ARRAY_POS0..=vars::VAR_ARRAY_POS7 => {
                Ok(state.array_position[(var - vars::VAR_ARRAY_POS0) as usize])
            }
            vars::VAR_GLOBAL => Ok(usize::try_from(array_index)
                .ok()
                .and_then(|index| state.global_variables.get(index).copied())
                .unwrap_or(0)),
            vars::VAR_LOCAL => Ok(usize::try_from(array_index)
                .ok()
                .and_then(|index| self.file.code.get(index).copied())
                .unwrap_or(0)),
            _ => host.read_engine_var(var, array_index, state),
        }
    }

    /// Writes a script variable by upstream `ScrVar` id. Out-of-range core variables are
    /// ignored rather than panicking.
    fn write_variable(
        &mut self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        var: i32,
        array_index: i32,
        value: i32,
    ) -> Result<(), ScriptError> {
        match var {
            vars::VAR_TEMP0..=vars::VAR_TEMP7 => {
                state.temp[(var - vars::VAR_TEMP0) as usize] = value;
                Ok(())
            }
            vars::VAR_CHECK_RESULT => {
                state.check_result = value;
                Ok(())
            }
            vars::VAR_ARRAY_POS0..=vars::VAR_ARRAY_POS7 => {
                state.array_position[(var - vars::VAR_ARRAY_POS0) as usize] = value;
                Ok(())
            }
            vars::VAR_GLOBAL => {
                if let Some(index) = usize::try_from(array_index)
                    .ok()
                    .filter(|index| *index < state.global_variables.len())
                {
                    state.global_variables[index] = value;
                }
                Ok(())
            }
            vars::VAR_LOCAL => {
                if let Some(index) = usize::try_from(array_index)
                    .ok()
                    .filter(|index| *index < self.file.code.len())
                {
                    self.file.code[index] = value;
                }
                Ok(())
            }
            _ => host.write_engine_var(var, array_index, value, state),
        }
    }

    fn push_jump(&self, stacks: &mut Stacks, index: i32) -> Result<(), ScriptError> {
        if stacks.jump.len() >= MAX_STACK {
            return Err(ScriptError::InvalidState(
                "jump table stack overflow".to_string(),
            ));
        }
        if index < 0 {
            return Err(ScriptError::InvalidState(
                "negative jump table index".to_string(),
            ));
        }
        stacks.jump.push(index as u32);
        Ok(())
    }

    fn pop_jump(&self, stacks: &mut Stacks) -> Result<u32, ScriptError> {
        stacks
            .jump
            .pop()
            .ok_or_else(|| ScriptError::InvalidState("jump table stack underflow".to_string()))
    }

    /// Jumps to `frame.code_start + relative`.
    fn jump_relative(&self, frame: &mut Frame, relative: i32) -> Result<(), ScriptError> {
        let target = (frame.code_start as i64) + (relative as i64);
        if target < 0 || target > self.file.code.len() as i64 {
            return Err(ScriptError::BadFunction);
        }
        frame.code_pos = target as u32;
        Ok(())
    }

    fn jump_table_target(
        &self,
        frame: &mut Frame,
        index: i32,
        offset: i32,
    ) -> Result<(), ScriptError> {
        if index < 0 {
            return Err(ScriptError::InvalidState(
                "negative jump table index".to_string(),
            ));
        }
        let table_index = (index as u32).wrapping_add(offset as u32);
        let relative = self.jump_word(frame.jump_start.wrapping_add(table_index))?;
        self.jump_relative(frame, relative)
    }

    fn execute(
        &mut self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        op: Op,
        frame: &mut Frame,
        stacks: &mut Stacks,
    ) -> Result<Outcome, ScriptError> {
        match op {
            Op::End => return Ok(Outcome::Stop),
            Op::Equal => state.operands[0] = state.operands[1],
            Op::Add => {
                state.operands[0] = state.operands[0].wrapping_add(state.operands[1]);
            }
            Op::Sub => {
                state.operands[0] = state.operands[0].wrapping_sub(state.operands[1]);
            }
            Op::Inc => state.operands[0] = state.operands[0].wrapping_add(1),
            Op::Dec => state.operands[0] = state.operands[0].wrapping_sub(1),
            Op::Mul => {
                state.operands[0] = state.operands[0].wrapping_mul(state.operands[1]);
            }
            Op::Div => {
                if state.operands[1] == 0 {
                    return Err(ScriptError::InvalidState("division by zero".to_string()));
                }
                state.operands[0] = state.operands[0].wrapping_div(state.operands[1]);
            }
            Op::ShR => {
                state.operands[0] = state.operands[0].wrapping_shr(state.operands[1] as u32);
            }
            Op::ShL => {
                state.operands[0] = state.operands[0].wrapping_shl(state.operands[1] as u32);
            }
            Op::And => state.operands[0] &= state.operands[1],
            Op::Or => state.operands[0] |= state.operands[1],
            Op::Xor => state.operands[0] ^= state.operands[1],
            Op::Mod => {
                if state.operands[1] == 0 {
                    return Err(ScriptError::InvalidState("modulo by zero".to_string()));
                }
                state.operands[0] = state.operands[0].wrapping_rem(state.operands[1]);
            }
            Op::FlipSign => state.operands[0] = state.operands[0].wrapping_neg(),
            Op::Not => state.operands[0] = !state.operands[0],
            Op::Abs => state.operands[0] = state.operands[0].wrapping_abs(),
            Op::CheckEqual => state.check_result = (state.operands[0] == state.operands[1]) as i32,
            Op::CheckGreater => state.check_result = (state.operands[0] > state.operands[1]) as i32,
            Op::CheckLower => state.check_result = (state.operands[0] < state.operands[1]) as i32,
            Op::CheckNotEqual => {
                state.check_result = (state.operands[0] != state.operands[1]) as i32;
            }
            Op::IfEqual => {
                if state.operands[1] != state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::IfGreater => {
                if state.operands[1] <= state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::IfGreaterOrEqual => {
                if state.operands[1] < state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::IfLower => {
                if state.operands[1] >= state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::IfLowerOrEqual => {
                if state.operands[1] > state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::IfNotEqual => {
                if state.operands[1] == state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 0)?;
                }
                self.push_jump(stacks, state.operands[0])?;
            }
            Op::Else => {
                let index = self.pop_jump(stacks)?;
                self.jump_table_target(frame, index as i32, 1)?;
            }
            Op::EndIf => {
                self.pop_jump(stacks)?;
            }
            Op::WEqual => {
                if state.operands[1] != state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::WGreater => {
                if state.operands[1] <= state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::WGreaterOrEqual => {
                if state.operands[1] < state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::WLower => {
                if state.operands[1] >= state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::WLowerOrEqual => {
                if state.operands[1] > state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::WNotEqual => {
                if state.operands[1] == state.operands[2] {
                    self.jump_table_target(frame, state.operands[0], 1)?;
                } else {
                    self.push_jump(stacks, state.operands[0])?;
                }
            }
            Op::Loop => {
                let index = self.pop_jump(stacks)?;
                self.jump_table_target(frame, index as i32, 0)?;
            }
            Op::ForEachActive | Op::ForEachAll => {
                self.foreach_step(host, state, frame, stacks, op)?;
            }
            Op::Next => {
                let index = self.pop_jump(stacks)?;
                self.jump_table_target(frame, index as i32, 0)?;
                if stacks.foreach_pos == 0 {
                    return Err(ScriptError::InvalidState(
                        "foreach stack underflow".to_string(),
                    ));
                }
                stacks.foreach_pos -= 1;
            }
            Op::Switch => {
                let index = state.operands[0];
                let value = state.operands[1];
                self.push_jump(stacks, index)?;
                let first = self.jump_word(frame.jump_start.wrapping_add(index as u32))?;
                let last =
                    self.jump_word(frame.jump_start.wrapping_add(index as u32).wrapping_add(1))?;
                if value < first || value > last {
                    self.jump_table_target(frame, index, 2)?;
                } else {
                    let offset = value.wrapping_sub(first);
                    self.jump_table_target(frame, index, 4i32.wrapping_add(offset))?;
                }
            }
            Op::Break => {
                let index = self.pop_jump(stacks)?;
                self.jump_table_target(frame, index as i32, 3)?;
            }
            Op::EndSwitch => {
                self.pop_jump(stacks)?;
            }
            Op::CallFunction => {
                let function = state.operands[0];
                let target = usize::try_from(function)
                    .ok()
                    .and_then(|index| self.file.functions.get(index))
                    .map(|function| (function.code_pos, function.jump_pos))
                    .ok_or(ScriptError::BadFunction)?;
                self.validate_entry(target.0)?;
                self.validate_entry(target.1)?;
                if stacks.calls.len() >= MAX_STACK {
                    return Err(ScriptError::InvalidState(
                        "function stack overflow".to_string(),
                    ));
                }
                stacks.calls.push(*frame);
                frame.code_start = target.0;
                frame.jump_start = target.1;
                frame.code_pos = target.0;
            }
            Op::Return => {
                if let Some(caller) = stacks.calls.pop() {
                    *frame = caller;
                } else {
                    return Ok(Outcome::Stop);
                }
            }
            Op::GetTableValue => {
                let array_index = state.operands[1];
                if array_index >= 0 {
                    let position = state.operands[2];
                    let position = u32::try_from(position).map_err(|_| ScriptError::Truncated)?;
                    let size = self.code_word(position)?;
                    if array_index < size {
                        state.operands[0] =
                            self.code_word(position.wrapping_add(array_index as u32 + 1))?;
                    }
                }
            }
            Op::SetTableValue => {
                let array_index = state.operands[1];
                if array_index >= 0 {
                    let position = state.operands[2];
                    let position = u32::try_from(position).map_err(|_| ScriptError::Truncated)?;
                    let size = self.code_word(position)?;
                    if array_index < size {
                        let target = position.wrapping_add(array_index as u32 + 1);
                        let index = usize::try_from(target).map_err(|_| ScriptError::Truncated)?;
                        if let Some(slot) = self.file.code.get_mut(index) {
                            *slot = state.operands[0];
                        } else {
                            return Err(ScriptError::Truncated);
                        }
                    }
                }
            }
            Op::Interpolate => {
                let a = state.operands[1];
                let b = state.operands[2];
                let factor = state.operands[3];
                state.operands[0] = (b.wrapping_mul(0x100i32.wrapping_sub(factor)))
                    .wrapping_add(factor.wrapping_mul(a))
                    >> 8;
            }
            Op::InterpolateXY => {
                let x1 = state.operands[2];
                let y1 = state.operands[4];
                let x2 = state.operands[3];
                let y2 = state.operands[5];
                let factor = state.operands[6];
                state.operands[0] = (x2.wrapping_mul(0x100i32.wrapping_sub(factor)) >> 8)
                    .wrapping_add((factor.wrapping_mul(x1)) >> 8);
                state.operands[1] = (y2.wrapping_mul(0x100i32.wrapping_sub(factor)) >> 8)
                    .wrapping_add((factor.wrapping_mul(y1)) >> 8);
            }
            _ => host.engine_op(op, state)?,
        }
        Ok(Outcome::Continue)
    }

    fn foreach_step(
        &self,
        host: &mut dyn ScriptHost,
        state: &mut VmState,
        frame: &mut Frame,
        stacks: &mut Stacks,
        op: Op,
    ) -> Result<(), ScriptError> {
        let index = state.operands[0];
        let selector = state.operands[1];
        if index < 0 {
            return Err(ScriptError::InvalidState(
                "negative jump table index".to_string(),
            ));
        }
        stacks.foreach_pos += 1;
        let position = stacks.foreach_pos;
        if position > MAX_STACK {
            return Err(ScriptError::InvalidState(
                "foreach stack overflow".to_string(),
            ));
        }
        if state.foreach_stack.len() <= position {
            state.foreach_stack.resize(position + 1, -1);
        }
        let candidate = state.foreach_stack[position].wrapping_add(1);
        state.foreach_stack[position] = candidate;

        match host.foreach_next(op, selector, candidate, state)? {
            Some(entity) => {
                state.operands[2] = entity;
                self.push_jump(stacks, index)?;
            }
            None => {
                state.foreach_stack[position] = -1;
                stacks.foreach_pos -= 1;
                self.jump_table_target(frame, index, 1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::ScriptFunction;
    use crate::opcodes::opcode_by_name;
    use std::collections::HashMap;

    fn rev03(name: &str) -> i32 {
        opcode_by_name(ScriptVersion::V4, V4Revision::Rev03, name).unwrap() as i32
    }

    /// Tiny assembler for hand-built bytecode; only the pieces tests need.
    #[derive(Default)]
    struct Asm {
        code: Vec<i32>,
        jump: Vec<u32>,
    }

    impl Asm {
        fn op(&mut self, name: &str) -> &mut Self {
            self.code.push(rev03(name));
            self
        }

        fn raw_op(&mut self, opcode: i32) -> &mut Self {
            self.code.push(opcode);
            self
        }

        fn int(&mut self, value: i32) -> &mut Self {
            self.code.push(2);
            self.code.push(value);
            self
        }

        fn var(&mut self, var: i32) -> &mut Self {
            self.code.push(1);
            self.code.push(0);
            self.code.push(var);
            self
        }

        fn var_array(&mut self, var: i32, index: i32) -> &mut Self {
            self.code.push(1);
            self.code.push(1);
            self.code.push(0);
            self.code.push(index);
            self.code.push(var);
            self
        }

        fn var_array_pos(&mut self, var: i32, array_pos: i32) -> &mut Self {
            self.code.push(1);
            self.code.push(1);
            self.code.push(1);
            self.code.push(array_pos);
            self.code.push(var);
            self
        }

        fn string(&mut self, text: &str) -> &mut Self {
            self.code.push(3);
            self.code.push(text.len() as i32);
            let bytes = text.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                let mut word = 0i32;
                for shift in 0..4 {
                    let byte = bytes.get(index + shift).copied().unwrap_or(0);
                    word |= (byte as i32) << (24 - shift * 8);
                }
                self.code.push(word);
                index += 4;
            }
            if bytes.len().is_multiple_of(4) {
                self.code.push(0);
            }
            self
        }

        fn jump(&mut self, relative: i32) -> &mut Self {
            self.jump.push(relative as u32);
            self
        }

        fn function(&self, name: &str, code_pos: u32, jump_pos: u32) -> ScriptFunction {
            ScriptFunction {
                name: name.to_string(),
                code_pos,
                jump_pos,
                ..Default::default()
            }
        }

        fn file(&self, functions: Vec<ScriptFunction>) -> ScriptFile {
            ScriptFile {
                functions,
                code: self.code.clone(),
                jump_table: self.jump.clone(),
                ..ScriptFile::default()
            }
        }
    }

    #[derive(Default)]
    struct MockHost {
        ops: Vec<Op>,
        operands: Vec<[i32; 16]>,
        texts: Vec<String>,
        entity_pos: i32,
        engine_vars: HashMap<i32, i32>,
        writes: Vec<(i32, i32, i32)>,
        foreach: Vec<Option<i32>>,
        foreach_calls: Vec<(Op, i32, i32)>,
    }

    impl ScriptHost for MockHost {
        fn engine_op(&mut self, op: Op, state: &mut VmState) -> Result<(), ScriptError> {
            self.ops.push(op);
            self.operands.push(state.operands);
            self.texts.push(state.script_text.clone());
            Ok(())
        }

        fn read_engine_var(
            &mut self,
            var: i32,
            _array_index: i32,
            _state: &mut VmState,
        ) -> Result<i32, ScriptError> {
            Ok(self.engine_vars.get(&var).copied().unwrap_or(0))
        }

        fn write_engine_var(
            &mut self,
            var: i32,
            array_index: i32,
            value: i32,
            _state: &mut VmState,
        ) -> Result<(), ScriptError> {
            self.writes.push((var, array_index, value));
            self.engine_vars.insert(var, value);
            Ok(())
        }

        fn object_entity_pos(&self) -> i32 {
            self.entity_pos
        }

        fn foreach_next(
            &mut self,
            op: Op,
            selector: i32,
            loop_index: i32,
            _state: &mut VmState,
        ) -> Result<Option<i32>, ScriptError> {
            self.foreach_calls.push((op, selector, loop_index));
            Ok(self
                .foreach
                .get(loop_index.max(0) as usize)
                .copied()
                .flatten())
        }
    }

    #[test]
    fn wraps_arithmetic_and_guards_min_div_minus_one() {
        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(i32::MIN);
        asm.op("Div").var(0).int(-1);
        asm.op("Mod").var(0).int(0); // error path tested separately
        asm.op("End");
        let file = asm.file(vec![asm.function("main", 0, 0)]);
        let mut vm = Vm::new(file);
        let mut state = VmState::default();
        let mut host = MockHost::default();
        // Mod by zero errors before End.
        assert!(matches!(
            vm.call(&mut host, 0, &mut state),
            Err(ScriptError::InvalidState(_))
        ));

        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(i32::MIN);
        asm.op("Div").var(0).int(-1);
        asm.op("Add").var(0).int(1);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(state.temp[0], i32::MIN.wrapping_add(1));

        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(7);
        asm.op("Div").var(0).int(0);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        assert!(matches!(
            vm.call(&mut host, 0, &mut state),
            Err(ScriptError::InvalidState(_))
        ));
    }

    #[test]
    fn shifts_are_masked() {
        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(1);
        asm.op("ShL").var(0).int(33);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 1i32.wrapping_shl(33));
    }

    #[test]
    fn comparisons_write_check_result() {
        let mut asm = Asm::default();
        asm.op("CheckGreater").int(5).int(3);
        asm.op("CheckLower").int(5).int(3);
        asm.op("CheckEqual").int(3).int(3);
        asm.op("CheckNotEqual").int(3).int(3);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.check_result, 0);
    }

    #[test]
    fn if_without_else_skips_when_false() {
        // 0: IfEqual(jt0, 5, 5)   [7 words]
        // 7: Add(temp0, 1)        [6 words]
        // 13: EndIf               [1 word]
        // 14: End
        // jump[0] = 13 (endif), jump[1] = 14 (after endif)
        let mut asm = Asm::default();
        asm.op("IfEqual").int(0).int(5).int(5);
        asm.op("Add").var(0).int(1);
        asm.op("endif");
        asm.op("End");
        asm.jump(13).jump(14);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 1);

        let mut asm = Asm::default();
        asm.op("IfEqual").int(0).int(5).int(6);
        asm.op("Add").var(0).int(1);
        asm.op("endif");
        asm.op("End");
        asm.jump(13).jump(14);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 0);
    }

    #[test]
    fn if_else_takes_the_right_branch() {
        // 0: IfEqual(jt0, 5, 6)   [7]
        // 7: Add(temp0, 1)        [6]
        // 13: Else                [1]
        // 14: Add(temp0, 10)      [6]
        // 20: EndIf               [1]
        // 21: End
        // jump[0] = 14 (else body), jump[1] = 21 (after endif)
        for (left, right, expected) in [(5, 6, 10), (5, 5, 1)] {
            let mut asm = Asm::default();
            asm.op("IfEqual").int(0).int(left).int(right);
            asm.op("Add").var(0).int(1);
            asm.op("else");
            asm.op("Add").var(0).int(10);
            asm.op("endif");
            asm.op("End");
            asm.jump(14).jump(21);
            let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
            let mut state = VmState::default();
            vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
            assert_eq!(state.temp[0], expected);
        }
    }

    #[test]
    fn while_loop_runs_until_false() {
        // 0: WLower(jt0, temp0, 3) [8 words]
        // 8: Add(temp0, 1)         [6]
        // 14: loop                 [1]
        // 15: End
        // jump[0] = 0 (loop start), jump[1] = 15 (after loop)
        let mut asm = Asm::default();
        asm.op("WLower").int(0).var(0).int(3);
        asm.op("Add").var(0).int(1);
        asm.op("loop");
        asm.op("End");
        asm.jump(0).jump(15);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 3);
    }

    #[test]
    fn switch_selects_case_and_break_exits() {
        // Layout:
        // 0: switch(jt0, temp0)   [6]
        // 6: Add(temp1, 1)  (case 0) [6]
        // 12: break               [1]
        // 13: Add(temp1, 10) (case 1) [6]
        // 19: break               [1]
        // 20: Add(temp1, 100) (default) [6]
        // 26: endswitch           [1]
        // 27: End
        // jump[0]=0, jump[1]=1, jump[2]=20, jump[3]=27, jump[4]=6, jump[5]=13
        let build = || {
            let mut asm = Asm::default();
            asm.op("switch").int(0).var(0);
            asm.op("Add").var(1).int(1);
            asm.op("break");
            asm.op("Add").var(1).int(10);
            asm.op("break");
            asm.op("Add").var(1).int(100);
            asm.op("endswitch");
            asm.op("End");
            asm.jump(0).jump(1).jump(20).jump(27).jump(6).jump(13);
            asm
        };
        for (value, expected) in [(0, 1), (1, 10), (7, 100)] {
            let asm = build();
            let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
            let mut state = VmState::default();
            state.temp[0] = value;
            vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
            assert_eq!(state.temp[1], expected, "switch value {value}");
        }
    }

    #[test]
    fn arrays_read_and_write_globals_and_locals() {
        let mut asm = Asm::default();
        // global[2] = 40; global[2] += 2; temp1 = global[2]; local[1] = 9; temp2 = local[1]
        asm.op("Equal").var_array(17, 2).int(40);
        asm.op("Add").var_array(17, 2).int(2);
        asm.op("Equal").var(1).var_array(17, 2);
        asm.op("Equal").var_array(18, 1).int(9);
        asm.op("Equal").var(2).var_array(18, 1);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState {
            global_variables: vec![0; 8],
            ..VmState::default()
        };
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.global_variables[2], 42);
        assert_eq!(state.temp[1], 42);
        assert_eq!(state.temp[2], 9);
        assert_eq!(vm.file().code[1], 9);
    }

    #[test]
    fn array_index_comes_from_array_position() {
        let mut asm = Asm::default();
        asm.op("Equal").var_array_pos(17, 0).int(77); // global[arrayPos0] = 77
        asm.op("Equal").var(1).var_array_pos(17, 0);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState {
            global_variables: vec![0; 4],
            ..VmState::default()
        };
        state.array_position[0] = 3;
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.global_variables[3], 77);
        assert_eq!(state.temp[1], 77);
    }

    #[test]
    fn engine_variables_go_through_the_host() {
        let mut asm = Asm::default();
        asm.op("Equal").var(0).var(500); // temp0 = engineVar500
        asm.op("Equal").var(500).int(12); // engineVar500 = 12 (writeback)
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        let mut host = MockHost::default();
        host.engine_vars.insert(500, 99);
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 99);
        // Equal writes both operands back; the last write carries the new value.
        assert_eq!(host.writes.last(), Some(&(500, 0, 12)));
        assert_eq!(host.engine_vars[&500], 12);
    }

    #[test]
    fn engine_ops_reach_the_host_with_decoded_operands() {
        let mut asm = Asm::default();
        asm.op("DrawSprite").int(42);
        asm.op("DrawTintRect").int(1).int(2).int(3).int(4);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        let mut host = MockHost::default();
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(host.ops, vec![Op::DrawSprite, Op::DrawTintRect]);
        assert_eq!(host.operands[0][0], 42);
        assert_eq!(&host.operands[1][..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn string_constants_are_decoded_into_script_text() {
        let mut asm = Asm::default();
        asm.op("LoadSpriteSheet").string("Sprites/Player.gif");
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        let mut host = MockHost::default();
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(host.ops, vec![Op::LoadSpriteSheet]);
        assert_eq!(host.texts, vec!["Sprites/Player.gif".to_string()]);
    }

    #[test]
    fn functions_call_and_return() {
        // main: temp0 = 1; CallFunction(1); temp0 += 100; End   (16 words)
        // helper: temp0 += 41; Return                            (7 words)
        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(1);
        asm.op("CallFunction").int(1);
        asm.op("Add").var(0).int(100);
        asm.op("End");
        let helper_pos = asm.code.len() as u32;
        asm.op("Add").var(0).int(41);
        asm.op("return");
        let functions = vec![
            asm.function("main", 0, 0),
            asm.function("helper", helper_pos, 0),
        ];

        let mut vm = Vm::new(asm.file(functions.clone()));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 142);

        let mut vm = Vm::new(asm.file(functions));
        let mut state = VmState::default();
        // Calling the helper directly returns to the top level.
        vm.call(&mut MockHost::default(), 1, &mut state).unwrap();
        assert_eq!(state.temp[0], 41);
    }

    #[test]
    fn find_function_uses_names() {
        let mut asm = Asm::default();
        asm.op("End");
        let file = asm.file(vec![
            asm.function("Main", 0, 0),
            asm.function("Helper", 0, 0),
        ]);
        let vm = Vm::new(file);
        assert_eq!(vm.find_function("Main"), Some(0));
        assert_eq!(vm.find_function("Helper"), Some(1));
        assert_eq!(vm.find_function("Missing"), None);
    }

    #[test]
    fn foreach_iterates_and_exits() {
        // 0: ForEachActive(jt0, group 0, temp1) [8]
        // 8: Add(temp0, 1)                      [6]
        // 14: next                              [1]
        // 15: End
        // jump[0] = 0 (foreach), jump[1] = 15 (after)
        let mut asm = Asm::default();
        asm.op("ForEachActive").int(0).int(0).var(1);
        asm.op("Add").var(0).int(1);
        asm.op("next");
        asm.op("End");
        asm.jump(0).jump(15);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        let mut host = MockHost {
            foreach: vec![Some(3), Some(7)],
            ..MockHost::default()
        };
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 2);
        assert_eq!(
            host.foreach_calls,
            vec![
                (Op::ForEachActive, 0, 0),
                (Op::ForEachActive, 0, 1),
                (Op::ForEachActive, 0, 2),
            ]
        );
    }

    #[test]
    fn foreach_breaks_out_when_host_returns_none() {
        let mut asm = Asm::default();
        asm.op("ForEachAll").int(0).int(99).var(1);
        asm.op("Add").var(0).int(1);
        asm.op("next");
        asm.op("End");
        asm.jump(0).jump(15);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 0);
        assert_eq!(state.foreach_stack.len(), 2);
        assert_eq!(state.foreach_stack[1], -1);
    }

    #[test]
    fn instruction_limit_stops_runaway_loops() {
        // 0: WEqual(jt0, 0, 0) [7]
        // 7: loop              [1]
        // 8: End
        // jump[0] = 0, jump[1] = 8
        let mut asm = Asm::default();
        asm.op("WEqual").int(0).int(0).int(0);
        asm.op("loop");
        asm.op("End");
        asm.jump(0).jump(8);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        assert!(matches!(
            vm.call_with_limit(&mut MockHost::default(), 0, &mut state, 64),
            Err(ScriptError::InstructionLimit)
        ));
    }

    #[test]
    fn invalid_opcode_and_bad_function_are_reported() {
        let mut asm = Asm::default();
        asm.raw_op(200);
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        assert!(matches!(
            vm.call(&mut MockHost::default(), 0, &mut state),
            Err(ScriptError::InvalidOpcode)
        ));
        assert!(matches!(
            vm.call(&mut MockHost::default(), 9, &mut state),
            Err(ScriptError::BadFunction)
        ));
    }

    #[test]
    fn table_values_read_and_write_code_tables() {
        // A table is stored inline in code: [size, values...].
        // 0: Equal(temp0, 0) [6]
        // 6: GetTableValue(temp0, 1, tablePos) [8]
        // 14: SetTableValue(5, 0, tablePos) [7]
        // 21: End
        // 22: table: [2, 11, 22]
        let mut asm = Asm::default();
        asm.op("Equal").var(0).int(0);
        asm.op("GetTableValue").var(0).int(1).int(22);
        asm.op("SetTableValue").int(5).int(0).int(22);
        asm.op("End");
        asm.code.extend_from_slice(&[2, 11, 22]);
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        vm.call(&mut MockHost::default(), 0, &mut state).unwrap();
        assert_eq!(state.temp[0], 22);
        assert_eq!(vm.file().code[23], 5);
    }

    #[test]
    fn string_operand_encoding_consumes_padding_word() {
        // A 4-byte string writes a full word and a padding word, matching upstream decode.
        let mut asm = Asm::default();
        asm.op("Print").string("abcd").int(0).int(0);
        let end_position = asm.code.len();
        asm.op("End");
        let mut vm = Vm::new(asm.file(vec![asm.function("main", 0, 0)]));
        let mut state = VmState::default();
        let mut host = MockHost::default();
        vm.call(&mut host, 0, &mut state).unwrap();
        assert_eq!(host.texts, vec!["abcd".to_string()]);
        // op + (type,len,word,padding) + int + int = 9 words before End.
        assert_eq!(end_position, 9);
    }
}
