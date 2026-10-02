//! Errors returned by the bytecode loader, VM and host boundary.

use thiserror::Error;

/// Errors produced while loading, writing or executing script data.
///
/// The VM never panics on malformed input; every failure path maps to one of these variants.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScriptError {
    /// The byte at the current code position is not a valid opcode for the selected revision.
    #[error("invalid opcode")]
    InvalidOpcode,
    /// The input ended in the middle of a structure or operand.
    #[error("truncated script data")]
    Truncated,
    /// A function index or entry point is outside the loaded script.
    #[error("bad script function")]
    BadFunction,
    /// The VM executed more instructions than the configured budget allowed.
    #[error("instruction limit exceeded")]
    InstructionLimit,
    /// The host reported an engine-side failure.
    #[error("script host error: {0}")]
    HostError(String),
    /// The script or VM state is structurally invalid (zero-length block, stack overflow, ...).
    #[error("invalid script state: {0}")]
    InvalidState(String),
}
