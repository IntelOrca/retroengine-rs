//! Bytecode loader, disassembler and virtual machine for RSDKv4 script data.
//!
//! This crate is the foundation for the RSDKv4 (Origins rev03) script pipeline:
//!
//! * [`opcodes`] maps the encoded opcode bytes of every revision to canonical [`opcodes::Op`]
//!   values, with upstream operand counts.
//! * [`bytecode`] loads and writes the `_Bytecode/*.bin` container.
//! * [`vm`] executes bytecode; engine operations, engine variables and `foreach` iteration are
//!   delegated to a [`vm::ScriptHost`] implementation.
//! * [`disasm`] renders bytecode as deterministic text.
//!
//! The text compiler (work package WP2b) builds against the same opcode and variable tables.
//! Only v4 tables are ported so far; [`version::ScriptVersion`] keeps the API ready for v2/v3.
//!
//! # Example
//!
//! ```
//! use retro_script::{bytecode, vm, VmState};
//!
//! let bytes = bytecode::write_bytecode(&bytecode::new_empty()).unwrap();
//! let file = bytecode::load_bytecode(&bytes).unwrap();
//! assert!(file.code.is_empty());
//!
//! let mut vm = vm::Vm::new(file);
//! assert_eq!(vm.find_function("main"), None);
//! let mut state = VmState::default();
//! let _ = state;
//! ```

#![forbid(unsafe_code)]

pub mod bytecode;
pub mod disasm;
pub mod error;
pub mod opcodes;
pub mod vars;
pub mod version;
pub mod vm;

pub use bytecode::{
    Access, EMPTY_EVENT, FUNCTION_COUNT, JUMPTABLE_COUNT, ObjectScript, SCRIPTCODE_COUNT,
    ScriptFile, ScriptFunction, ScriptPtr, load_bytecode, new_empty, write_bytecode,
};
pub use error::ScriptError;
pub use opcodes::{
    Op, OpInfo, OperandKind, canonical_op, encoded_opcode, op_writes_back, opcode_by_name,
    opcode_table, revision_table,
};
pub use version::{ScriptVersion, V4Revision};
pub use vm::{
    DEFAULT_INSTRUCTION_LIMIT, ForeachStackEntry, ScriptEngineState, ScriptHost, Vm, VmState,
};
