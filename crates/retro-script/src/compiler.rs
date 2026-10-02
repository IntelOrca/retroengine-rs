//! RetroScript v4 text compiler.
//!
//! Ports the compiler half of `ParseScriptFile` and its helpers from
//! RSDKModding/RSDKv4-Decompilation (`RSDKv4/Script.cpp`, main @ 2026-09-06), which is itself the
//! official RSDKv5U v4 legacy compiler. The engine uses this path when `Bytecode/` is absent.
//!
//! The port is validated byte-for-byte against that C++ compiler on the shipped script sources:
//! 71 of the 72 shipped groups match on code words, jump table entries, function positions and
//! object entry points. The single divergence is S2 `Mission_Zone02`: the reference stores alias
//! names in a 32-byte buffer, so the 32-character `EGGMANSIGNPOST_SPAWNFALLSIGNPOST` loses its
//! NUL terminator and fails to resolve there, while this port resolves it exactly like the
//! original tools (the shipped `_Bytecode` agrees). See `tests/compiler_assets.rs` for the golden
//! hashes, the divergence test and the script data-version caveats, and `tools/script-oracle` for
//! the reproducible reference harness.
//!
//! # Fidelity notes
//!
//! * Bytecode positions are global-absolute. [`CompileOptions::base_code_pos`] /
//!   [`CompileOptions::base_jump_pos`] offset the first emitted word, exactly like the engine's
//!   `scriptCodePos`/`jumpTablePos` after `GlobalCode.bin` has been loaded.
//! * Groups are compiled with one shared [`Compiler`] state, because upstream `ParseScriptFile`
//!   keeps public functions, public aliases/values/tables and reserved function slots alive
//!   between files. A stage group's function table therefore contains the global functions, just
//!   like the shipped `_Bytecode/<Stage>.bin` files. Use [`Compiler::mark`] +
//!   [`Compiler::finish_group`] to extract one group out of a global+stage compile.
//! * `#platform` handling models upstream `Engine.releaseType`: [`PlatformMode::Standalone`]
//!   activates `USE_STANDALONE` and [`PlatformMode::Origins`] activates `USE_ORIGINS`. The
//!   compile-time platform/renderer/haptics tags (`STANDARD`, `MOBILE`, `SW_RENDERING`,
//!   `HW_RENDERING`, ...) and the decomp-only `USE_DECOMP`/`USE_MOD_LOADER` tags are treated as
//!   inactive, matching the original tools that produced the shipped `_Bytecode` files.
//! * Upstream only understands `//` comments; `/* ... */` block comments and nested `#platform`
//!   blocks are supported as extensions (neither occurs in the shipped scripts).
//! * Malformed input returns a [`CompileError`] with a 1-based line number instead of aborting
//!   or panicking. A few upstream conditions are only errors in strict mode
//!   ([`CompileOptions::strict`]) because the shipped scripts contain deliberately unclosed
//!   `if` blocks that upstream leaves unresolved.
//! * Engine symbol tables (`VarName[]`, `TypeName[]`, `SfxName[]`, ...) are supplied through
//!   [`CompileOptions::symbols`]; upstream reads them from `GameConfig.bin`/`StageConfig.bin`.

use std::fmt;

use serde::Serialize;

use crate::bytecode::{
    Access, EMPTY_EVENT, FUNCTION_COUNT, JUMPTABLE_COUNT, ObjectScript, SCRIPTCODE_COUNT,
    ScriptFile, ScriptFunction, ScriptPtr,
};
use crate::opcodes::{Op, opcode_table};
use crate::version::{ScriptVersion, V4Revision};

/// `ScriptVarTypes` in upstream: first word of a dynamically typed operand.
const SCRIPTVAR_VAR: i32 = 1;
const SCRIPTVAR_INTCONST: i32 = 2;
const SCRIPTVAR_STRCONST: i32 = 3;

/// `ScriptVarArrTypes` in upstream: array kind of a variable operand.
const VARARR_NONE: i32 = 0;
const VARARR_ARRAY: i32 = 1;
const VARARR_ENTNOPLUS1: i32 = 2;
const VARARR_ENTNOMINUS1: i32 = 3;

/// Upstream jump-table placeholder for an entry that has not been patched yet.
const JUMP_UNSET: u32 = u32::MAX;
/// Upstream default entry point for an event with no script (`JUMPTABLE_COUNT - 1`).
const EMPTY_JUMP_POS: u32 = JUMPTABLE_COUNT as u32 - 1;

/// Which `#platform: USE_*` release tag is active, mirroring upstream `Engine.releaseType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PlatformMode {
    /// `Engine.releaseType == "USE_STANDALONE"` (Sonic 1/2 2013 mobile releases).
    Standalone,
    /// `Engine.releaseType == "USE_ORIGINS"` (Sonic Origins content, used by these assets).
    Origins,
}

impl PlatformMode {
    /// Platform tags that make a `#platform: <tag>` block compile in this mode.
    pub const fn active_tags(self) -> &'static [&'static str] {
        match self {
            PlatformMode::Standalone => &["USE_STANDALONE"],
            PlatformMode::Origins => &["USE_ORIGINS"],
        }
    }

    /// Lower-case name, useful for diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            PlatformMode::Standalone => "standalone",
            PlatformMode::Origins => "origins",
        }
    }
}

/// Engine symbol tables consulted while resolving operands.
///
/// Upstream populates these from `Data/Game/GameConfig.bin`, the active `StageConfig.bin` and
/// `Achievements.bin`. Object type and sound effect names are stored with spaces stripped, the
/// same transformation `SetObjectTypeName`/`SetSfxName` apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SymbolTables {
    /// Global variable names in file order (`VarName[]`, and bare references like
    /// `options.stageSelectFlag`).
    pub global_variables: Vec<String>,
    /// Object type names in engine load order, index 0 being `BlankObject` (`TypeName[]`).
    pub object_types: Vec<String>,
    /// Sound effect names, globals followed by stage effects (`SfxName[]`).
    pub sfx_names: Vec<String>,
    /// Player names (`PlayerName[]`); spaces are ignored during matching.
    pub players: Vec<String>,
    /// Achievement identifiers (`AchievementName[]`); spaces are ignored during matching.
    pub achievements: Vec<String>,
    /// Scene names per stage list, used by `StageName[]`.
    pub scenes: SceneNames,
}

/// Scene names grouped the way upstream `GetSceneID` indexes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SceneNames {
    pub presentation: Vec<String>,
    pub regular: Vec<String>,
    pub special: Vec<String>,
    pub bonus: Vec<String>,
}

/// Compiler configuration.
///
/// The first five fields mirror the work-package sketch. `symbols` and `strict` are additions:
/// the v4 bytecode format carries no symbol tables, so `VarName[]`/`TypeName[]`/`SfxName[]`
/// cannot be resolved without the engine data, and upstream tolerates some malformed control
/// flow that callers may want reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    /// Active `#platform` release tag.
    pub platform: PlatformMode,
    /// Opcode table revision to target.
    pub revision: V4Revision,
    /// Absolute code position of the first emitted word (`scriptCodePos`).
    pub base_code_pos: u32,
    /// Absolute jump table position of the first emitted entry (`jumpTablePos`).
    pub base_jump_pos: u32,
    /// Mirrors `StageConfig::load_global_objects`. The compiler is stateful: when true, callers
    /// should compile the global object files into the same [`Compiler`] before the stage files
    /// so that global functions and public aliases resolve.
    pub include_global_scripts: bool,
    /// Engine symbol tables used by `VarName[]`/`TypeName[]`/`SfxName[]`/...
    pub symbols: SymbolTables,
    /// Reports conditions upstream silently tolerates (unclosed `if`/`while`/`foreach`/`switch`,
    /// unterminated `#platform`/`table` blocks, malformed alias declarations).
    pub strict: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            platform: PlatformMode::Standalone,
            revision: V4Revision::Rev03,
            base_code_pos: 0,
            base_jump_pos: 0,
            include_global_scripts: false,
            symbols: SymbolTables::default(),
            strict: false,
        }
    }
}

/// A compile failure, always carrying a 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// 1-based line number inside the file, or the line that opened the offending construct.
    pub line: usize,
    /// Human readable description.
    pub message: String,
    /// File name, when the caller supplied one.
    pub file: Option<String>,
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(formatter, "{}:{}: {}", file, self.line, self.message),
            None => write!(formatter, "line {}: {}", self.line, self.message),
        }
    }
}

impl std::error::Error for CompileError {}

/// A checkpoint of the compiler's append-only tables, used by [`Compiler::finish_group`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupMark {
    /// Number of code words appended so far.
    pub code: usize,
    /// Number of jump table entries appended so far.
    pub jump_table: usize,
    /// Number of functions registered so far.
    pub functions: usize,
    /// Number of object scripts registered so far.
    pub object_scripts: usize,
}

/// Compiles a single script file from a fresh state.
pub fn compile_script(source: &str, options: &CompileOptions) -> Result<ScriptFile, CompileError> {
    let mut compiler = Compiler::new(options);
    compiler.compile_file(None, source)?;
    Ok(compiler.finish())
}

/// Compiles a single object script file from a fresh state.
///
/// Identical to [`compile_script`]; it exists to make the object-file entry point explicit. The
/// result always contains exactly one [`ObjectScript`] entry whose event pointers default to
/// [`EMPTY_EVENT`] when the file does not define them.
pub fn compile_object_script(
    source: &str,
    options: &CompileOptions,
) -> Result<ScriptFile, CompileError> {
    compile_script(source, options)
}

/// Compiles and links several script files into one group, mirroring a `Bytecode/<Group>.bin`.
///
/// Files are compiled in order in one shared state. The returned [`ScriptFile::code`] and
/// [`ScriptFile::jump_table`] contain only the words emitted by these files (starting at
/// `options.base_code_pos` / `base_jump_pos`), while functions and object scripts keep their
/// absolute positions. For a stage group that follows `GlobalCode.bin`, use a [`Compiler`] and
/// [`Compiler::finish_group`] instead so the global function table and public aliases are
/// visible.
pub fn compile_group(
    files: &[(&str, &str)],
    options: &CompileOptions,
) -> Result<ScriptFile, CompileError> {
    let mut compiler = Compiler::new(options);
    for (name, source) in files {
        compiler.compile_file(Some(name), source)?;
    }
    Ok(compiler.finish())
}

/// Stateful compiler, matching upstream `ParseScriptFile`'s persistence between files.
#[derive(Debug, Clone)]
pub struct Compiler {
    options: CompileOptions,
    code: Vec<i32>,
    jump_table: Vec<u32>,
    code_pos: u32,
    jump_pos: u32,
    code_offset: u32,
    jump_offset: u32,
    jump_stack: Vec<JumpEntry>,
    functions: Vec<FunctionState>,
    values: Vec<ValueState>,
    object_scripts: Vec<ObjectScript>,
    current_file: Option<String>,
    current_line: usize,
    platform_depth: usize,
    platform_open_line: usize,
    table_open_line: usize,
}

#[derive(Debug, Clone)]
struct FunctionState {
    name: String,
    access: AccessState,
    code_pos: u32,
    jump_pos: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessState {
    None,
    Public,
    Private,
}

#[derive(Debug, Clone)]
struct ValueState {
    access: Access,
    name: String,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpKind {
    If,
    While,
    Foreach,
    Switch,
}

#[derive(Debug, Clone, Copy)]
struct JumpEntry {
    pos: u32,
    line: usize,
    kind: JumpKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseMode {
    ScopeLess,
    PlatformSkip,
    Function,
    TableRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Update,
    Draw,
    Startup,
}

#[derive(Debug, Clone)]
struct SourceLine {
    text: String,
    number: usize,
}

impl Compiler {
    /// Creates a compiler with a fresh state, including the built-in public aliases.
    pub fn new(options: &CompileOptions) -> Self {
        let mut values = Vec::new();
        for (name, value) in common_aliases(options.revision) {
            values.push(ValueState {
                access: Access::Public,
                name: (*name).to_string(),
                value: (*value).to_string(),
            });
        }
        Self {
            options: options.clone(),
            code: Vec::new(),
            jump_table: Vec::new(),
            code_pos: options.base_code_pos,
            jump_pos: options.base_jump_pos,
            code_offset: options.base_code_pos,
            jump_offset: options.base_jump_pos,
            jump_stack: Vec::new(),
            functions: Vec::new(),
            values,
            object_scripts: Vec::new(),
            current_file: None,
            current_line: 0,
            platform_depth: 0,
            platform_open_line: 0,
            table_open_line: 0,
        }
    }

    /// Replaces the engine symbol tables (for example when switching to another stage).
    pub fn set_symbols(&mut self, symbols: SymbolTables) {
        self.options.symbols = symbols;
    }

    /// Enables or disables strict diagnostics.
    pub fn set_strict(&mut self, strict: bool) {
        self.options.strict = strict;
    }

    /// Number of code words appended so far (excluding `base_code_pos`).
    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    /// Number of jump table entries appended so far (excluding `base_jump_pos`).
    pub fn jump_table_len(&self) -> usize {
        self.jump_table.len()
    }

    /// Number of functions registered so far.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Number of object scripts registered so far.
    pub fn object_script_count(&self) -> usize {
        self.object_scripts.len()
    }

    /// Returns a checkpoint for [`Compiler::finish_group`].
    pub fn mark(&self) -> GroupMark {
        GroupMark {
            code: self.code.len(),
            jump_table: self.jump_table.len(),
            functions: self.functions.len(),
            object_scripts: self.object_scripts.len(),
        }
    }

    /// Copies the code words appended since `mark` (a [`GroupMark::code`] index).
    pub fn code_since(&self, mark: usize) -> Vec<i32> {
        self.code[mark.min(self.code.len())..].to_vec()
    }

    /// Compiles one file into this state, mirroring upstream `ParseScriptFile`.
    pub fn compile_file(&mut self, file: Option<&str>, source: &str) -> Result<(), CompileError> {
        self.current_file = file.map(str::to_string);
        self.current_line = 0;
        self.jump_stack.clear();
        self.platform_depth = 0;

        // Upstream clears the names of non-public functions so later files cannot call them.
        for function in &mut self.functions {
            if function.access != AccessState::Public {
                function.name.clear();
            }
        }

        // ...and compacts the value list, dropping non-public aliases/values/tables.
        let common = common_value_count(self.options.revision);
        let mut new_count = common;
        for index in common..self.values.len() {
            if self.values[index].access != Access::Public {
                self.values[index].name.clear();
            } else {
                if new_count != index {
                    self.values[new_count] = self.values[index].clone();
                }
                new_count += 1;
            }
        }
        self.values.truncate(new_count);

        self.object_scripts.push(ObjectScript {
            update: empty_ptr(),
            draw: empty_ptr(),
            startup: empty_ptr(),
        });

        let lines = tokenize(source);
        let mut mode = ParseMode::ScopeLess;
        let mut index = 0usize;
        while index < lines.len() {
            let line = &lines[index];
            self.current_line = line.number;
            match mode {
                ParseMode::ScopeLess => self.parse_scopeless(line, &mut mode)?,
                ParseMode::Function => self.parse_function(&lines, &mut index, &mut mode)?,
                ParseMode::PlatformSkip => {
                    if starts_with_token(&line.text, "#platform:") {
                        self.platform_depth += 1;
                    } else if starts_with_token(&line.text, "#endplatform") {
                        self.platform_depth -= 1;
                        if self.platform_depth == 0 {
                            mode = ParseMode::Function;
                        }
                    }
                }
                ParseMode::TableRead => {
                    if starts_with_token(&line.text, "endtable") {
                        mode = ParseMode::ScopeLess;
                    } else if !line.text.is_empty() {
                        self.read_table_values(&line.text)?;
                    }
                }
            }
            index += 1;
        }

        if self.options.strict {
            if self.platform_depth > 0 {
                return Err(self.error_at(
                    self.platform_open_line,
                    "#platform block is missing '#endplatform'",
                ));
            }
            if mode == ParseMode::TableRead {
                return Err(self.error_at(self.table_open_line, "table is missing 'end table'"));
            }
            if mode == ParseMode::Function {
                return Err(self.error("script is missing 'end event'/'end function'"));
            }
            if let Some(entry) = self.jump_stack.first() {
                let keyword = match entry.kind {
                    JumpKind::If => "end if",
                    JumpKind::While => "loop",
                    JumpKind::Foreach => "next",
                    JumpKind::Switch => "end switch",
                };
                return Err(
                    self.error_at(entry.line, format!("control block is missing '{keyword}'"))
                );
            }
        }
        Ok(())
    }

    /// Consumes the compiler and returns the full compilation result.
    pub fn finish(self) -> ScriptFile {
        let functions = self.functions.iter().map(function_to_script).collect();
        let global_variables = self.options.symbols.global_variables.clone();
        let values = self.value_names();
        ScriptFile {
            functions,
            code: self.code,
            jump_table: self.jump_table,
            global_variables,
            values,
            object_scripts: self.object_scripts,
        }
    }

    /// Consumes the compiler and returns one group, keeping the full function table.
    ///
    /// The shipped `_Bytecode/<Stage>.bin` files store only the stage's code and jump table but
    /// carry the global functions as well; this reproduces that layout.
    pub fn finish_group(self, mark: GroupMark) -> ScriptFile {
        let functions = self.functions.iter().map(function_to_script).collect();
        let global_variables = self.options.symbols.global_variables.clone();
        let values = self.value_names();
        let mut code = self.code;
        code.drain(..mark.code);
        let mut jump_table = self.jump_table;
        jump_table.drain(..mark.jump_table);
        let mut object_scripts = self.object_scripts;
        object_scripts.drain(..mark.object_scripts);
        ScriptFile {
            functions,
            code,
            jump_table,
            global_variables,
            values,
            object_scripts,
        }
    }

    fn value_names(&self) -> Vec<String> {
        let common = common_value_count(self.options.revision);
        self.values
            .iter()
            .skip(common)
            .map(|value| value.name.clone())
            .filter(|name| !name.is_empty())
            .collect()
    }

    fn error(&self, message: impl Into<String>) -> CompileError {
        CompileError {
            line: self.current_line,
            message: message.into(),
            file: self.current_file.clone(),
        }
    }

    fn error_at(&self, line: usize, message: impl Into<String>) -> CompileError {
        CompileError {
            line,
            message: message.into(),
            file: self.current_file.clone(),
        }
    }

    fn emit(&mut self, value: i32) -> Result<(), CompileError> {
        if self.code_pos as usize >= SCRIPTCODE_COUNT {
            return Err(self.error("script code overflow"));
        }
        self.code.push(value);
        self.code_pos += 1;
        Ok(())
    }

    fn emit_op(&mut self, op: Op) -> Result<(), CompileError> {
        let encoded = crate::opcodes::encoded_opcode(ScriptVersion::V4, self.options.revision, op)
            .ok_or_else(|| {
                self.error(format!(
                    "opcode {op:?} is not available in {}",
                    self.options.revision.name()
                ))
            })?;
        self.emit(encoded as i32)
    }

    fn push_jump(&mut self, value: u32) -> Result<(), CompileError> {
        if self.jump_pos as usize >= JUMPTABLE_COUNT {
            return Err(self.error("jump table overflow"));
        }
        self.jump_table.push(value);
        self.jump_pos += 1;
        Ok(())
    }

    fn code_at(&self, pos: u32) -> Option<i32> {
        let index = pos.checked_sub(self.options.base_code_pos)? as usize;
        self.code.get(index).copied()
    }

    fn set_code(&mut self, pos: u32, value: i32) -> Result<(), CompileError> {
        let Some(index) = pos.checked_sub(self.options.base_code_pos) else {
            return Err(self.error("internal code position out of range"));
        };
        let Some(slot) = self.code.get_mut(index as usize) else {
            return Err(self.error("internal code position out of range"));
        };
        *slot = value;
        Ok(())
    }

    fn jump_at(&self, pos: u32) -> Option<u32> {
        let index = pos.checked_sub(self.options.base_jump_pos)? as usize;
        self.jump_table.get(index).copied()
    }

    fn set_jump_at(&mut self, pos: u32, value: u32) -> Result<(), CompileError> {
        let Some(index) = pos.checked_sub(self.options.base_jump_pos) else {
            return Err(self.error("internal jump table position out of range"));
        };
        let Some(slot) = self.jump_table.get_mut(index as usize) else {
            return Err(self.error("internal jump table position out of range"));
        };
        *slot = value;
        Ok(())
    }

    fn push_jump_stack(&mut self, pos: u32, kind: JumpKind) {
        self.jump_stack.push(JumpEntry {
            pos,
            line: self.current_line,
            kind,
        });
    }

    // ---------------------------------------------------------------------------------------
    // Scope-level statements
    // ---------------------------------------------------------------------------------------

    fn parse_scopeless(
        &mut self,
        line: &SourceLine,
        mode: &mut ParseMode,
    ) -> Result<(), CompileError> {
        let text = line.text.as_str();
        self.check_alias_text(text)?;
        self.check_static_text(text)?;
        if self.check_table_text(text)? {
            self.table_open_line = line.number;
            *mode = ParseMode::TableRead;
            return Ok(());
        }

        if str_comp(text, "eventObjectUpdate") {
            self.begin_event(EventKind::Update);
            *mode = ParseMode::Function;
        }
        if str_comp(text, "eventObjectDraw") {
            self.begin_event(EventKind::Draw);
            *mode = ParseMode::Function;
        }
        if str_comp(text, "eventObjectStartup") {
            self.begin_event(EventKind::Startup);
            *mode = ParseMode::Function;
        }

        if starts_with_token(text, "reservefunction") {
            let name = text.get(15..).unwrap_or_default();
            if self.functions.len() >= FUNCTION_COUNT {
                return Err(self.error("too many functions"));
            }
            if self.find_function(name).is_none() {
                self.functions.push(FunctionState {
                    name: name.to_string(),
                    access: AccessState::None,
                    code_pos: EMPTY_EVENT,
                    jump_pos: EMPTY_JUMP_POS,
                });
            }
        } else if starts_with_token(text, "publicfunction") {
            let name = text.get(14..).unwrap_or_default().to_string();
            self.declare_function(name, AccessState::Public)?;
            *mode = ParseMode::Function;
        } else if starts_with_token(text, "privatefunction") {
            let name = text.get(15..).unwrap_or_default().to_string();
            self.declare_function(name, AccessState::Private)?;
            *mode = ParseMode::Function;
        }
        Ok(())
    }

    fn declare_function(&mut self, name: String, access: AccessState) -> Result<(), CompileError> {
        let code_pos = self.code_pos;
        let jump_pos = self.jump_pos;
        match self.find_function(&name) {
            Some(id) => {
                self.functions[id].name = name;
                self.functions[id].access = access;
                self.functions[id].code_pos = code_pos;
                self.functions[id].jump_pos = jump_pos;
            }
            None => {
                if self.functions.len() >= FUNCTION_COUNT {
                    return Err(self.error("too many functions"));
                }
                self.functions.push(FunctionState {
                    name,
                    access,
                    code_pos,
                    jump_pos,
                });
            }
        }
        self.code_offset = code_pos;
        self.jump_offset = jump_pos;
        Ok(())
    }

    fn begin_event(&mut self, kind: EventKind) {
        let ptr = ScriptPtr {
            code_pos: self.code_pos,
            jump_pos: self.jump_pos,
        };
        if let Some(script) = self.object_scripts.last_mut() {
            match kind {
                EventKind::Update => script.update = ptr,
                EventKind::Draw => script.draw = ptr,
                EventKind::Startup => script.startup = ptr,
            }
        }
        self.code_offset = self.code_pos;
        self.jump_offset = self.jump_pos;
    }

    fn find_function(&self, name: &str) -> Option<usize> {
        let mut found = None;
        for (index, function) in self.functions.iter().enumerate() {
            if str_comp(name, &function.name) {
                found = Some(index);
            }
        }
        found
    }

    fn check_alias_text(&mut self, text: &str) -> Result<(), CompileError> {
        let (prefix, access) = if starts_with_token(text, "publicalias") {
            (11usize, Access::Public)
        } else if starts_with_token(text, "privatealias") {
            (12usize, Access::Private)
        } else {
            return Ok(());
        };
        let bytes = text.as_bytes();
        let mut value = String::new();
        let mut name = String::new();
        let mut mode = 0u8;
        let mut pos = prefix;
        while pos < bytes.len() {
            let c = bytes[pos];
            if mode == 0 {
                if c == b':' {
                    mode = 1;
                } else {
                    value.push(c as char);
                }
            } else {
                name.push(c as char);
            }
            pos += 1;
        }
        if mode == 0 && self.options.strict {
            return Err(self.error("alias declaration is missing ':'"));
        }
        self.values.push(ValueState {
            access,
            name,
            value,
        });
        Ok(())
    }

    fn check_static_text(&mut self, text: &str) -> Result<(), CompileError> {
        let (prefix, access) = if starts_with_token(text, "publicvalue") {
            (11usize, Access::Public)
        } else if starts_with_token(text, "privatevalue") {
            (12usize, Access::Private)
        } else {
            return Ok(());
        };
        let bytes = text.as_bytes();
        let mut name = String::new();
        let mut value = String::new();
        let mut mode = 0u8;
        let mut pos = prefix;
        while pos < bytes.len() {
            let c = bytes[pos];
            if mode == 0 {
                if c == b'=' {
                    mode = 1;
                } else {
                    name.push(c as char);
                }
            } else {
                value.push(c as char);
            }
            pos += 1;
        }
        let initial = if value.is_empty() {
            0
        } else {
            parse_int(&value).unwrap_or(0)
        };
        let slot = self.code_pos;
        self.emit(initial)?;
        self.values.push(ValueState {
            access,
            name,
            value: format!("local[{slot}]"),
        });
        Ok(())
    }

    fn check_table_text(&mut self, text: &str) -> Result<bool, CompileError> {
        let (prefix, access) = if starts_with_token(text, "publictable") {
            (11usize, Access::Public)
        } else if starts_with_token(text, "privatetable") {
            (12usize, Access::Private)
        } else {
            return Ok(false);
        };
        let bytes = text.as_bytes();
        let mut name = String::new();
        let mut pos = prefix;
        while pos < bytes.len() {
            let c = bytes[pos];
            if c == b'[' || c == b']' {
                pos += 1;
                break;
            }
            name.push(c as char);
            pos += 1;
        }

        if find_token(text, "]").is_none() {
            let slot = self.code_pos;
            self.emit(0)?;
            self.code_offset = slot;
            self.values.push(ValueState {
                access,
                name,
                value: slot.to_string(),
            });
            return Ok(true);
        }

        let mut size_text = String::new();
        while pos < bytes.len() {
            let c = bytes[pos];
            if c == b'[' || c == b']' {
                break;
            }
            size_text.push(c as char);
            pos += 1;
        }
        for value in &self.values {
            if str_comp(&size_text, &value.name) {
                size_text = value.value.clone();
            }
        }
        let count = parse_int(&size_text).unwrap_or(1);
        let slot = self.code_pos;
        self.emit(count)?;
        for _ in 0..count.max(0) {
            self.emit(0)?;
        }
        self.values.push(ValueState {
            access,
            name,
            value: slot.to_string(),
        });
        Ok(false)
    }

    fn read_table_values(&mut self, text: &str) -> Result<(), CompileError> {
        let bytes = text.as_bytes();
        let mut pos = 0usize;
        let mut buffer = String::new();
        while pos < bytes.len() {
            buffer.push(bytes[pos] as char);
            pos += 1;
            while bytes.get(pos) == Some(&b',') {
                self.bump_table_count()?;
                self.emit(parse_int(&buffer).unwrap_or(0))?;
                buffer.clear();
                pos += 1;
            }
        }
        if !buffer.is_empty() {
            self.bump_table_count()?;
            self.emit(parse_int(&buffer).unwrap_or(0))?;
        }
        Ok(())
    }

    fn bump_table_count(&mut self) -> Result<(), CompileError> {
        let current = self
            .code_at(self.code_offset)
            .ok_or_else(|| self.error("internal table position out of range"))?;
        self.set_code(self.code_offset, current.wrapping_add(1))
    }

    // ---------------------------------------------------------------------------------------
    // Function/event statements
    // ---------------------------------------------------------------------------------------

    fn parse_function(
        &mut self,
        lines: &[SourceLine],
        index: &mut usize,
        mode: &mut ParseMode,
    ) -> Result<(), CompileError> {
        let text = lines[*index].text.as_str();
        if text.is_empty() {
            return Ok(());
        }
        if str_comp(text, "endevent") {
            self.emit_op(Op::End)?;
            *mode = ParseMode::ScopeLess;
            return Ok(());
        }
        if str_comp(text, "endfunction") {
            self.emit_op(Op::Return)?;
            *mode = ParseMode::ScopeLess;
            return Ok(());
        }
        if starts_with_token(text, "#platform:") {
            let active = self
                .options
                .platform
                .active_tags()
                .iter()
                .any(|tag| find_token(text, tag).is_some());
            if !active {
                self.platform_depth = 1;
                self.platform_open_line = self.current_line;
                *mode = ParseMode::PlatformSkip;
            }
            return Ok(());
        }
        if find_token(text, "#endplatform").is_some() {
            return Ok(());
        }

        let converted = self.convert_conditional(text)?;
        let converted = match self.convert_switch(&converted)? {
            Some(switch_text) => {
                self.scan_switch(lines, *index)?;
                switch_text
            }
            None => converted,
        };
        let converted = convert_arithmetic(&converted, self.options.revision);
        if !self.read_switch_case(&converted)? {
            self.convert_function(&converted)?;
        }
        Ok(())
    }

    /// Rewrites `if`/`while`/`foreach` into an opcode call and pushes jump table entries.
    fn convert_conditional(&mut self, text: &str) -> Result<String, CompileError> {
        if starts_with_token(text, "if") {
            if let Some((compare_op, str_pos)) = find_compare(text) {
                let names = [
                    "IfEqual",
                    "IfGreater",
                    "IfGreaterOrEqual",
                    "IfLower",
                    "IfLowerOrEqual",
                    "IfNotEqual",
                ];
                let jpos = self.jump_pos - self.jump_offset;
                let mut dest = format!("{}({jpos},", names[compare_op]);
                let bytes = text.as_bytes();
                for (offset, byte) in bytes.iter().enumerate().skip(2) {
                    let c = if offset == str_pos { b',' } else { *byte };
                    if c != b'=' && c != b'(' && c != b')' {
                        dest.push(c as char);
                    }
                }
                dest.push(')');
                self.push_jump_stack(self.jump_pos, JumpKind::If);
                self.push_jump(JUMP_UNSET)?;
                self.push_jump(0)?;
                return Ok(dest);
            }
        } else if starts_with_token(text, "while") {
            if let Some((compare_op, str_pos)) = find_compare(text) {
                let names = [
                    "WEqual",
                    "WGreater",
                    "WGreaterOrEqual",
                    "WLower",
                    "WLowerOrEqual",
                    "WNotEqual",
                ];
                let jpos = self.jump_pos - self.jump_offset;
                let mut dest = format!("{}({jpos},", names[compare_op]);
                let bytes = text.as_bytes();
                for (offset, byte) in bytes.iter().enumerate().skip(5) {
                    let c = if offset == str_pos { b',' } else { *byte };
                    if c != b'=' && c != b'(' && c != b')' {
                        dest.push(c as char);
                    }
                }
                dest.push(')');
                self.push_jump_stack(self.jump_pos, JumpKind::While);
                self.push_jump(self.code_pos - self.code_offset)?;
                self.push_jump(0)?;
                return Ok(dest);
            }
        } else if starts_with_token(text, "foreach")
            && let Some(arg_pos) = find_token_n(text, ",", 2)
        {
            let bytes = text.as_bytes();
            let active = bytes.get(arg_pos + 2) == Some(&b'C');
            let name = if active {
                "ForEachActive"
            } else {
                "ForEachAll"
            };
            let jpos = self.jump_pos - self.jump_offset;
            let mut dest = format!("{name}({jpos},");
            for byte in bytes.iter().take(arg_pos).skip(7) {
                if *byte != b'(' && *byte != b')' {
                    dest.push(*byte as char);
                }
            }
            dest.push(')');
            self.push_jump_stack(self.jump_pos, JumpKind::Foreach);
            self.push_jump(self.code_pos - self.code_offset)?;
            self.push_jump(0)?;
            return Ok(dest);
        }
        Ok(text.to_string())
    }

    /// Rewrites `switch x` into `switch(jpos,x)` and pushes the four switch entries.
    fn convert_switch(&mut self, text: &str) -> Result<Option<String>, CompileError> {
        if !starts_with_token(text, "switch") {
            return Ok(None);
        }
        let jpos = self.jump_pos - self.jump_offset;
        let mut converted = format!("switch({jpos},");
        for byte in text.as_bytes().iter().skip(6) {
            if *byte != b'=' && *byte != b'(' && *byte != b')' {
                converted.push(*byte as char);
            }
        }
        converted.push(')');
        self.push_jump_stack(self.jump_pos, JumpKind::Switch);
        self.push_jump(0x1_0000)?;
        self.push_jump((-0x1_0000i32) as u32)?;
        self.push_jump(JUMP_UNSET)?;
        self.push_jump(0)?;
        Ok(Some(converted))
    }

    fn scan_switch(&mut self, lines: &[SourceLine], index: usize) -> Result<(), CompileError> {
        let Some(entry) = self.jump_stack.last().copied() else {
            return Ok(());
        };
        let jpos = entry.pos;
        let mut depth = 0usize;
        let mut found_end = false;
        let mut cursor = index + 1;
        while cursor < lines.len() {
            let text = lines[cursor].text.as_str();
            if starts_with_token(text, "switch") {
                depth += 1;
            }
            if depth > 0 {
                if starts_with_token(text, "endswitch") {
                    depth -= 1;
                }
            } else if starts_with_token(text, "endswitch") {
                found_end = true;
                break;
            } else {
                self.current_line = lines[cursor].number;
                self.check_case_number(text)?;
            }
            cursor += 1;
        }
        if !found_end {
            if self.options.strict {
                return Err(self.error_at(entry.line, "switch statement is missing 'end switch'"));
            }
            return Ok(());
        }
        let min = self
            .jump_at(jpos)
            .ok_or_else(|| self.error("internal switch position out of range"))?
            as i32;
        let max = self
            .jump_at(jpos + 1)
            .ok_or_else(|| self.error("internal switch position out of range"))?
            as i32;
        let count = max.wrapping_sub(min).unsigned_abs() as usize + 1;
        if count > JUMPTABLE_COUNT {
            return Err(self.error("switch case range is too large"));
        }
        for _ in 0..count {
            self.push_jump(JUMP_UNSET)?;
        }
        Ok(())
    }

    fn check_case_number(&mut self, text: &str) -> Result<(), CompileError> {
        if !starts_with_token(text, "case") {
            return Ok(());
        }
        let bytes = text.as_bytes();
        let mut case_string = String::new();
        let mut pos = 5usize;
        let mut case_char = bytes.get(4).copied().unwrap_or(0);
        while case_char != 0 {
            if case_char != b':' {
                case_string.push(case_char as char);
            }
            case_char = bytes.get(pos).copied().unwrap_or(0);
            pos += 1;
        }
        let Some(case_id) = self.resolve_case(&case_string) else {
            return Ok(());
        };
        let Some(jpos) = self.jump_stack.last().map(|entry| entry.pos) else {
            return Ok(());
        };
        let min = self
            .jump_at(jpos)
            .ok_or_else(|| self.error("internal switch position out of range"))?
            as i32;
        let max = self
            .jump_at(jpos + 1)
            .ok_or_else(|| self.error("internal switch position out of range"))?
            as i32;
        if case_id < min {
            self.set_jump_at(jpos, case_id as u32)?;
        }
        if case_id > max {
            self.set_jump_at(jpos + 1, case_id as u32)?;
        }
        Ok(())
    }

    fn read_switch_case(&mut self, text: &str) -> Result<bool, CompileError> {
        if starts_with_token(text, "case") {
            let mut case_text = String::new();
            for byte in text.as_bytes().iter().skip(4) {
                if *byte != b':' {
                    case_text.push(*byte as char);
                }
            }
            let Some(case_id) = self.resolve_case(&case_text) else {
                return Ok(true);
            };
            let Some(jpos) = self.jump_stack.last().map(|entry| entry.pos) else {
                if self.options.strict {
                    return Err(self.error("'case' outside of a switch"));
                }
                return Ok(true);
            };
            let min = self
                .jump_at(jpos)
                .ok_or_else(|| self.error("internal switch position out of range"))?
                as i32;
            let target = jpos as i64 + 4 + (case_id as i64 - min as i64);
            if target < 0 || target > u32::MAX as i64 {
                return Err(self.error("case value is outside the switch range"));
            }
            let target = target as u32;
            if self.jump_at(target).is_none() {
                return Err(self.error("case value is outside the switch range"));
            }
            self.set_jump_at(target, self.code_pos - self.code_offset)?;
            return Ok(true);
        }
        if starts_with_token(text, "default") {
            let Some(jpos) = self.jump_stack.last().map(|entry| entry.pos) else {
                if self.options.strict {
                    return Err(self.error("'default' outside of a switch"));
                }
                return Ok(true);
            };
            let pos = self.code_pos - self.code_offset;
            self.set_jump_at(jpos + 2, pos)?;
            let min = self
                .jump_at(jpos)
                .ok_or_else(|| self.error("internal switch position out of range"))?
                as i32;
            let max = self
                .jump_at(jpos + 1)
                .ok_or_else(|| self.error("internal switch position out of range"))?
                as i32;
            let count = max.wrapping_sub(min).unsigned_abs() as usize + 1;
            if count > JUMPTABLE_COUNT {
                return Err(self.error("switch case range is too large"));
            }
            for offset in 0..count {
                let target = jpos + 4 + offset as u32;
                if (self.jump_at(target).unwrap_or(0) as i32) < 0 {
                    self.set_jump_at(target, pos)?;
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Resolves a `case`/`default` value the way upstream `CheckCaseNumber` does.
    fn resolve_case(&self, case_text: &str) -> Option<i32> {
        let mut case_string = case_text.to_string();
        let mut found = false;
        if find_token(&case_string, "[").is_some() {
            let bytes = case_string.as_bytes();
            let mut case_value = String::new();
            let mut array_str = String::new();
            let mut mode = 0u8;
            let mut pos = 0usize;
            while pos < bytes.len() && bytes[pos] != b':' {
                let c = bytes[pos];
                if mode == 1 {
                    if c == b']' {
                        mode = 0;
                    } else {
                        array_str.push(c as char);
                    }
                } else if c == b'[' {
                    mode = 1;
                } else {
                    case_value.push(c as char);
                }
                pos += 1;
            }
            if str_comp(&case_value, "TypeName") {
                case_value = self.resolve_object_type(&array_str);
            }
            if str_comp(&case_value, "SfxName") {
                case_value = self.resolve_sfx(&array_str);
            }
            if str_comp(&case_value, "VarName") {
                case_value = self.resolve_var_name(&array_str);
            }
            if str_comp(&case_value, "AchievementName") {
                case_value = self.resolve_achievement(&array_str);
            }
            if str_comp(&case_value, "PlayerName") {
                case_value = self.resolve_player(&array_str);
            }
            if str_comp(&case_value, "StageName") {
                case_value = self.resolve_stage_name(&array_str);
            }
            case_string = case_value;
            found = true;
        }
        if !found {
            for value in &self.values {
                if str_comp(&value.name, &case_string) {
                    case_string = value.value.clone();
                    break;
                }
            }
        }
        parse_int(&case_string)
    }

    fn convert_function(&mut self, text: &str) -> Result<(), CompileError> {
        let table = opcode_table(ScriptVersion::V4, self.options.revision);
        let paren = text.find('(').unwrap_or(text.len());
        let func_name = &text[..paren];
        let mut lookup = None;
        for (index, info) in table.iter().enumerate() {
            if str_comp(func_name, info.name) {
                lookup = Some((index, info.op, info.name.len()));
                break;
            }
        }
        let Some((opcode, op, name_len)) = lookup else {
            return Err(self.error(format!("opcode not found: {func_name}")));
        };
        if opcode == 0 {
            return Err(self.error(format!("opcode not found: {func_name}")));
        }
        self.emit(opcode as i32)?;

        match op {
            Op::Else => {
                if let Some(entry) = self.jump_stack.last().copied() {
                    self.set_jump_at(entry.pos, self.code_pos - self.code_offset)?;
                } else if self.options.strict {
                    return Err(self.error("'else' outside of an if statement"));
                }
            }
            Op::EndIf => {
                if let Some(entry) = self.jump_stack.pop() {
                    let pos = self.code_pos - self.code_offset;
                    self.set_jump_at(entry.pos + 1, pos)?;
                    if self.jump_at(entry.pos) == Some(JUMP_UNSET) {
                        self.set_jump_at(entry.pos, pos.wrapping_sub(1))?;
                    }
                } else if self.options.strict {
                    return Err(self.error("'end if' without a matching 'if'"));
                }
            }
            Op::EndSwitch => {
                if let Some(entry) = self.jump_stack.pop() {
                    let pos = self.code_pos - self.code_offset;
                    self.set_jump_at(entry.pos + 3, pos)?;
                    if self.jump_at(entry.pos + 2) == Some(JUMP_UNSET) {
                        self.set_jump_at(entry.pos + 2, pos.wrapping_sub(1))?;
                        let min = self.jump_at(entry.pos).unwrap_or(0) as i32;
                        let max = self.jump_at(entry.pos + 1).unwrap_or(0) as i32;
                        let count = max.wrapping_sub(min).unsigned_abs() as usize + 1;
                        if count > JUMPTABLE_COUNT {
                            return Err(self.error("switch case range is too large"));
                        }
                        for offset in 0..count {
                            let target = entry.pos + 4 + offset as u32;
                            if (self.jump_at(target).unwrap_or(0) as i32) < 0 {
                                self.set_jump_at(target, pos.wrapping_sub(1))?;
                            }
                        }
                    }
                } else if self.options.strict {
                    return Err(self.error("'end switch' without a matching 'switch'"));
                }
            }
            Op::Loop | Op::Next => {
                if let Some(entry) = self.jump_stack.pop() {
                    self.set_jump_at(entry.pos + 1, self.code_pos - self.code_offset)?;
                } else if self.options.strict {
                    return Err(self.error("'loop'/'next' without a matching 'while'/'foreach'"));
                }
            }
            _ => {}
        }

        let opcode_size = table[opcode].operands.len();
        let mut text_pos = name_len;
        for _ in 0..opcode_size {
            text_pos += 1;
            let mut func_name = String::new();
            let mut array_str = String::new();
            let mut mode = 0u8;
            let mut prev_mode = 0u8;
            let bytes = text.as_bytes();
            loop {
                let c = bytes.get(text_pos).copied().unwrap_or(0);
                let keep_going = ((c != b',' && c != b')') || mode == 2) && c != 0;
                if !keep_going {
                    break;
                }
                match mode {
                    0 => match c {
                        b'[' => mode = 1,
                        b'"' => {
                            prev_mode = 0;
                            mode = 2;
                            func_name.push('"');
                        }
                        _ => func_name.push(c as char),
                    },
                    1 => match c {
                        b']' => mode = 0,
                        b'"' => {
                            prev_mode = 1;
                            mode = 2;
                        }
                        _ => array_str.push(c as char),
                    },
                    _ => match c {
                        b'"' => {
                            mode = prev_mode;
                            func_name.push('"');
                        }
                        _ => func_name.push(c as char),
                    },
                }
                text_pos += 1;
            }
            self.resolve_operand(&mut func_name, &mut array_str);
            self.store_operand(&func_name, &mut array_str)?;
        }
        Ok(())
    }

    /// Alias/global/function/special-array resolution for one operand, in upstream order.
    fn resolve_operand(&self, func_name: &mut String, array_str: &mut String) {
        for value in &self.values {
            if str_comp(func_name, &value.name) {
                *func_name = copy_alias_str(&value.value, false);
                if find_token(&value.value, "[").is_some() {
                    *array_str = copy_alias_str(&value.value, true);
                }
            }
        }

        if !array_str.is_empty() {
            let mut start = 0usize;
            let first = array_str.as_bytes()[0];
            if first == b'+' || first == b'-' {
                start = 1;
            }
            let buffer = array_str[start..].to_string();
            for value in &self.values {
                if str_comp(&buffer, &value.name) {
                    let prefix = array_str.as_bytes()[0] as char;
                    *array_str = copy_alias_str(&value.value, false);
                    if prefix == '+' || prefix == '-' {
                        array_str.insert(0, prefix);
                    }
                }
            }
        }

        for (index, name) in self.options.symbols.global_variables.iter().enumerate() {
            if str_comp(func_name, name) {
                *func_name = "global".to_string();
                *array_str = index.to_string();
            }
        }

        for (index, function) in self.functions.iter().enumerate() {
            if str_comp(func_name, &function.name) {
                *func_name = index.to_string();
            }
        }

        if str_comp(func_name, "TypeName") {
            *func_name = self.resolve_object_type(array_str);
        }
        if str_comp(func_name, "SfxName") {
            *func_name = self.resolve_sfx(array_str);
        }
        if str_comp(func_name, "VarName") {
            *func_name = self.resolve_var_name(array_str);
        }
        if str_comp(func_name, "AchievementName") {
            *func_name = self.resolve_achievement(array_str);
        }
        if str_comp(func_name, "PlayerName") {
            *func_name = self.resolve_player(array_str);
        }
        if str_comp(func_name, "StageName") {
            *func_name = self.resolve_stage_name(array_str);
        }
    }

    fn resolve_object_type(&self, name: &str) -> String {
        for (index, candidate) in self.options.symbols.object_types.iter().enumerate() {
            if str_comp(name, candidate) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn resolve_sfx(&self, name: &str) -> String {
        for (index, candidate) in self.options.symbols.sfx_names.iter().enumerate() {
            if str_comp(name, candidate) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn resolve_var_name(&self, name: &str) -> String {
        for (index, candidate) in self.options.symbols.global_variables.iter().enumerate() {
            if str_comp(name, candidate) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn resolve_achievement(&self, name: &str) -> String {
        for (index, candidate) in self.options.symbols.achievements.iter().enumerate() {
            let stripped: String = candidate.chars().filter(|c| *c != ' ').collect();
            if str_comp(name, &stripped) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn resolve_player(&self, name: &str) -> String {
        for (index, candidate) in self.options.symbols.players.iter().enumerate() {
            let stripped: String = candidate.chars().filter(|c| *c != ' ').collect();
            if str_comp(name, &stripped) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn resolve_stage_name(&self, name: &str) -> String {
        let bytes = name.as_bytes();
        if bytes.len() < 2 {
            return "0".to_string();
        }
        let list = match bytes[0] {
            b'P' => &self.options.symbols.scenes.presentation,
            b'R' => &self.options.symbols.scenes.regular,
            b'S' => &self.options.symbols.scenes.special,
            b'B' => &self.options.symbols.scenes.bonus,
            _ => return "0".to_string(),
        };
        let target: String = name[2..].chars().filter(|c| *c != ' ').collect();
        for (index, candidate) in list.iter().enumerate() {
            let stripped: String = candidate.chars().filter(|c| *c != ' ').collect();
            if str_comp(&target, &stripped) {
                return index.to_string();
            }
        }
        "0".to_string()
    }

    fn store_operand(
        &mut self,
        func_name: &str,
        array_str: &mut String,
    ) -> Result<(), CompileError> {
        if let Some(constant) = parse_int(func_name) {
            self.emit(SCRIPTVAR_INTCONST)?;
            self.emit(constant)?;
            return Ok(());
        }
        if func_name.starts_with('"') {
            if func_name.len() < 2 || !func_name.ends_with('"') {
                return Err(self.error("unterminated string literal"));
            }
            self.emit(SCRIPTVAR_STRCONST)?;
            self.emit(func_name.len() as i32 - 2)?;
            let bytes = func_name.as_bytes();
            let mut word = 0i32;
            let mut slot = 0usize;
            let mut pos = 1usize;
            loop {
                let c = i32::from(bytes[pos]);
                match slot {
                    0 => word = c << 24,
                    1 => word += c << 16,
                    2 => word += c << 8,
                    _ => {
                        word += c;
                        self.emit(word)?;
                    }
                }
                slot = (slot + 1) % 4;
                if bytes[pos] == b'"' {
                    if slot != 0 {
                        self.emit(word)?;
                    }
                    break;
                }
                pos += 1;
            }
            return Ok(());
        }

        self.emit(SCRIPTVAR_VAR)?;
        if !array_str.is_empty() {
            let kind = match array_str.as_bytes()[0] {
                b'+' => VARARR_ENTNOPLUS1,
                b'-' => VARARR_ENTNOMINUS1,
                _ => VARARR_ARRAY,
            };
            self.emit(kind)?;
            if kind != VARARR_ARRAY {
                array_str.remove(0);
            }
            if let Some(constant) = parse_int(array_str) {
                self.emit(0)?;
                self.emit(constant)?;
            } else {
                let mut constant = 0;
                for (index, name) in [
                    "arrayPos0",
                    "arrayPos1",
                    "arrayPos2",
                    "arrayPos3",
                    "arrayPos4",
                    "arrayPos5",
                    "arrayPos6",
                    "arrayPos7",
                    "tempObjectPos",
                ]
                .iter()
                .enumerate()
                {
                    if str_comp(array_str, name) {
                        constant = index as i32;
                    }
                }
                self.emit(1)?;
                self.emit(constant)?;
            }
        } else {
            self.emit(VARARR_NONE)?;
        }
        let id = self
            .variable_id_ci(func_name)
            .ok_or_else(|| self.error(format!("operand not found: {func_name}")))?;
        self.emit(id)?;
        Ok(())
    }

    fn variable_id_ci(&self, name: &str) -> Option<i32> {
        let mut index = 0i32;
        while let Some(candidate) = crate::vars::variable_name(self.options.revision, index) {
            if candidate.len() == name.len() && str_comp(name, candidate) {
                return Some(index);
            }
            index += 1;
        }
        None
    }
}

fn function_to_script(function: &FunctionState) -> ScriptFunction {
    ScriptFunction {
        name: function.name.clone(),
        access: match function.access {
            AccessState::Public => Access::Public,
            AccessState::None | AccessState::Private => Access::Private,
        },
        code_pos: function.code_pos,
        jump_pos: function.jump_pos,
        is_native: false,
    }
}

fn empty_ptr() -> ScriptPtr {
    ScriptPtr {
        code_pos: EMPTY_EVENT,
        jump_pos: EMPTY_JUMP_POS,
    }
}

fn common_value_count(revision: V4Revision) -> usize {
    if revision == V4Revision::Rev00 {
        33
    } else {
        34
    }
}

fn common_aliases(revision: V4Revision) -> Vec<(&'static str, &'static str)> {
    let mut aliases = vec![
        ("true", "1"),
        ("false", "0"),
        ("FX_SCALE", "0"),
        ("FX_ROTATE", "1"),
        ("FX_ROTOZOOM", "2"),
        ("FX_INK", "3"),
        ("PRESENTATION_STAGE", "0"),
        ("REGULAR_STAGE", "1"),
        ("BONUS_STAGE", "2"),
        ("SPECIAL_STAGE", "3"),
        ("MENU_1", "0"),
        ("MENU_2", "1"),
        ("C_TOUCH", "0"),
        ("C_SOLID", "1"),
        ("C_SOLID2", "2"),
        ("C_PLATFORM", "3"),
        ("C_BOX", "65536"),
        ("MAT_WORLD", "0"),
        ("MAT_VIEW", "1"),
        ("MAT_TEMP", "2"),
        ("FX_FLIP", "5"),
        ("FACING_LEFT", "1"),
        ("FACING_RIGHT", "0"),
    ];
    if revision != V4Revision::Rev00 {
        aliases.push(("STAGE_2P_MODE", "4"));
    }
    aliases.extend_from_slice(&[
        ("STAGE_FROZEN", "3"),
        ("STAGE_PAUSED", "2"),
        ("STAGE_RUNNING", "1"),
        ("RESET_GAME", "2"),
        ("STANDARD", "0"),
        ("MOBILE", "1"),
        ("DEVICE_XBOX", "2"),
        ("DEVICE_PSN", "3"),
        ("DEVICE_IOS", "4"),
        ("DEVICE_ANDROID", "5"),
    ]);
    aliases
}

// -------------------------------------------------------------------------------------------
// Tokenizer
// -------------------------------------------------------------------------------------------

const READ_NORMAL: u8 = 0;
const READ_STRING: u8 = 1;
const READ_COMMENTLINE: u8 = 2;
const READ_ENDLINE: u8 = 3;
const READ_EOF: u8 = 4;

/// Splits a source into tokenized lines, mirroring upstream's character loop.
///
/// Spaces, tabs, carriage returns and semicolons are separators; `;` terminates a logical line
/// without advancing the line counter; `//` comments run to end of line. Quotes toggle string
/// mode, inside which spaces are preserved. `/* ... */` block comments are stripped first
/// (extension; upstream only handles `//`).
#[allow(unused_assignments)]
fn tokenize(source: &str) -> Vec<SourceLine> {
    let cleaned = strip_block_comments(source);
    let bytes = cleaned.as_bytes();
    let mut lines = Vec::new();
    let mut pos = 0usize;
    let mut read_mode = READ_NORMAL;
    let mut cur_char: u8 = 0;
    let mut prev_char: u8 = 0;
    let mut line_no = 0usize;
    let mut text = Vec::<u8>::new();

    while read_mode < READ_EOF {
        text.clear();
        read_mode = READ_NORMAL;
        let mut disable_increment = false;
        while read_mode < READ_ENDLINE {
            prev_char = cur_char;
            if pos < bytes.len() {
                cur_char = bytes[pos];
                pos += 1;
            }
            if read_mode == READ_STRING {
                if cur_char == b'\t'
                    || cur_char == b'\r'
                    || cur_char == b'\n'
                    || cur_char == b';'
                    || read_mode >= READ_COMMENTLINE
                {
                    if cur_char == b'\n' {
                        read_mode = READ_ENDLINE;
                    }
                } else if cur_char != b'/' || text.is_empty() {
                    text.push(cur_char);
                    if cur_char == b'"' {
                        read_mode = READ_NORMAL;
                    }
                } else if cur_char == b'/' && prev_char == b'/' {
                    read_mode = READ_COMMENTLINE;
                    text.pop();
                } else {
                    text.push(cur_char);
                }
            } else if cur_char == b' '
                || cur_char == b'\t'
                || cur_char == b'\r'
                || cur_char == b'\n'
                || cur_char == b';'
                || read_mode >= READ_COMMENTLINE
            {
                if cur_char == b'\n' || cur_char == b';' {
                    read_mode = READ_ENDLINE;
                    if cur_char == b';' {
                        disable_increment = true;
                    }
                }
            } else if cur_char != b'/' || text.is_empty() {
                text.push(cur_char);
                if cur_char == b'"' && read_mode == READ_NORMAL {
                    read_mode = READ_STRING;
                }
            } else if cur_char == b'/' && prev_char == b'/' {
                read_mode = READ_COMMENTLINE;
                text.pop();
            } else {
                text.push(cur_char);
            }
            if pos >= bytes.len() {
                read_mode = READ_EOF;
            }
        }
        if !disable_increment {
            line_no += 1;
        }
        let end = text
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(text.len());
        lines.push(SourceLine {
            text: String::from_utf8_lossy(&text[..end]).into_owned(),
            number: line_no,
        });
    }
    lines
}

/// Replaces `/* ... */` contents with spaces while preserving newlines and byte positions.
fn strip_block_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut pos = 0usize;
    let mut in_string = false;
    while pos < bytes.len() {
        let c = bytes[pos];
        if in_string {
            if c == b'"' {
                in_string = false;
            }
            pos += 1;
        } else if c == b'"' {
            in_string = true;
            pos += 1;
        } else if c == b'/' && bytes.get(pos + 1) == Some(&b'/') {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
        } else if c == b'/' && bytes.get(pos + 1) == Some(&b'*') {
            out[pos] = b' ';
            out[pos + 1] = b' ';
            pos += 2;
            while pos < bytes.len() && !(bytes[pos] == b'*' && bytes.get(pos + 1) == Some(&b'/')) {
                if bytes[pos] != b'\n' {
                    out[pos] = b' ';
                }
                pos += 1;
            }
            if pos < bytes.len() {
                out[pos] = b' ';
                out[pos + 1] = b' ';
                pos += 2;
            }
        } else {
            pos += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// -------------------------------------------------------------------------------------------
// String helpers (ports of String.cpp)
// -------------------------------------------------------------------------------------------

/// Port of upstream `StrComp`: byte equality or a one-case difference counts as a match.
fn str_comp(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut index = 0usize;
    loop {
        let ca = a.get(index).copied().unwrap_or(0);
        let cb = b.get(index).copied().unwrap_or(0);
        let matches = ca == cb || ca == cb.wrapping_add(b' ') || ca == cb.wrapping_sub(b' ');
        if !matches {
            return false;
        }
        if ca == 0 {
            return true;
        }
        index += 1;
    }
}

/// Port of upstream `FindStringToken(string, token, stopID)`; returns the byte offset.
fn find_token_n(haystack: &str, token: &str, stop_id: usize) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let token = token.as_bytes();
    let mut found = 0usize;
    let mut start = 0usize;
    while start < haystack.len() {
        if haystack.len() - start < token.len() {
            return None;
        }
        let mut matched = true;
        for (offset, expected) in token.iter().enumerate() {
            if haystack[start + offset] != *expected {
                matched = false;
            }
        }
        if matched {
            found += 1;
            if found == stop_id {
                return Some(start);
            }
        }
        start += 1;
    }
    None
}

/// Port of upstream `FindStringToken(string, token, 1)`.
fn find_token(haystack: &str, token: &str) -> Option<usize> {
    find_token_n(haystack, token, 1)
}

/// True when `token` occurs at offset zero.
fn starts_with_token(text: &str, token: &str) -> bool {
    find_token(text, token) == Some(0)
}

/// Port of upstream `ConvertStringToInteger`: parses an optional sign, `0x`/`0b`/`0o` base
/// prefixes and hex digit values exactly like the engine's `ParseScriptFile` helper.
#[must_use]
pub fn parse_int(text: &str) -> Option<i32> {
    let bytes = text.as_bytes();
    let first = *bytes.first()?;
    if first != b'+' && !first.is_ascii_digit() && first != b'-' {
        return None;
    }
    let mut negative = false;
    let mut base: u32 = 10;
    let mut value: i32 = 0;
    let mut char_id = 0usize;
    let mut str_length = bytes.len() as i64 - 1;
    if first == b'-' {
        negative = true;
        char_id = 1;
        str_length -= 1;
    } else if first == b'+' {
        char_id = 1;
        str_length -= 1;
    }

    if bytes.get(char_id) == Some(&b'0') {
        match bytes.get(char_id + 1) {
            Some(b'x') | Some(b'X') => base = 16,
            Some(b'b') | Some(b'B') => base = 2,
            Some(b'o') | Some(b'O') => base = 8,
            _ => {}
        }
        if base != 10 {
            char_id += 2;
            str_length -= 2;
        }
    }

    while str_length > -1 {
        let c = bytes.get(char_id).copied().unwrap_or(0);
        let mut flag = c < b'0';
        if !flag {
            if base == 16 && c > b'f' {
                flag = true;
            }
            if base == 8 && c > b'7' {
                flag = true;
            }
            if base == 2 && c > b'1' {
                flag = true;
            }
        }
        if flag {
            return None;
        }
        if str_length <= 0 {
            if c.is_ascii_digit() {
                value = value.wrapping_add(i32::from(c - b'0'));
            } else if (b'a'..=b'f').contains(&c) {
                value = value.wrapping_add(i32::from(c - b'a' + 10));
            } else if (b'A'..=b'F').contains(&c) {
                value = value.wrapping_add(i32::from(c - b'A' + 10));
            }
        } else {
            let mut strlen = str_length + 1;
            let mut char_val: u32 = 0;
            if c.is_ascii_digit() {
                char_val = u32::from(c - b'0');
            } else if (b'a'..=b'f').contains(&c) {
                char_val = u32::from(c - b'a' + 10);
            } else if (b'A'..=b'F').contains(&c) {
                char_val = u32::from(c - b'A' + 10);
            }
            loop {
                strlen -= 1;
                if strlen == 0 {
                    break;
                }
                char_val = char_val.wrapping_mul(base);
            }
            value = value.wrapping_add(char_val as i32);
        }
        str_length -= 1;
        char_id += 1;
    }

    if negative {
        value = value.wrapping_neg();
    }
    Some(value)
}

/// Port of upstream `CopyAliasStr`: `array_index` selects the bracketed part.
fn copy_alias_str(text: &str, array_index: bool) -> String {
    let mut dest = String::new();
    let mut array_value = false;
    for c in text.chars() {
        if array_index {
            if array_value {
                if c == ']' {
                    array_value = false;
                } else {
                    dest.push(c);
                }
            } else if c == '[' {
                array_value = true;
            }
        } else if array_value {
            if c == ']' {
                array_value = false;
            }
        } else if c == '[' {
            array_value = true;
        } else {
            dest.push(c);
        }
    }
    dest
}

/// Port of upstream `ConvertArithmaticSyntax`.
fn convert_arithmetic(text: &str, revision: V4Revision) -> String {
    const TOKENS: [&str; 13] = [
        "=", "+=", "-=", "++", "--", "*=", "/=", ">>=", "<<=", "&=", "|=", "^=", "%=",
    ];
    const OPS: [Op; 13] = [
        Op::Equal,
        Op::Add,
        Op::Sub,
        Op::Inc,
        Op::Dec,
        Op::Mul,
        Op::Div,
        Op::ShR,
        Op::ShL,
        Op::And,
        Op::Or,
        Op::Xor,
        Op::Mod,
    ];
    let mut token = 0usize;
    let mut offset = 0usize;
    for (index, candidate) in TOKENS.iter().enumerate() {
        if let Some(found) = find_token(text, candidate) {
            offset = found;
            token = index + 1;
        }
    }
    if token == 0 {
        return text.to_string();
    }
    let op = OPS[token - 1];
    let table = opcode_table(ScriptVersion::V4, revision);
    let opcode_size = crate::opcodes::encoded_opcode(ScriptVersion::V4, revision, op)
        .and_then(|encoded| table.get(encoded as usize))
        .map_or(2, |info| info.operands.len());
    let name = table
        .iter()
        .find(|info| info.op == op)
        .map_or("Equal", |info| info.name);
    let mut dest = format!("{name}(");
    dest.push_str(&text[..offset]);
    if opcode_size > 1 {
        dest.push(',');
        let len = TOKENS[token - 1].len();
        offset += len;
        dest.push_str(&text[offset..]);
    }
    dest.push(')');
    dest
}

/// Port of the comparison-operator scan in upstream `ConvertConditionalStatement`.
fn find_compare(text: &str) -> Option<(usize, usize)> {
    const TOKENS: [&str; 6] = ["==", ">", ">=", "<", "<=", "!="];
    let mut result = None;
    for (index, token) in TOKENS.iter().enumerate() {
        if let Some(found) = find_token(text, token) {
            result = Some((index, found));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> CompileOptions {
        CompileOptions {
            platform: PlatformMode::Origins,
            revision: V4Revision::Rev03,
            ..CompileOptions::default()
        }
    }

    fn op(op: Op) -> i32 {
        crate::opcodes::encoded_opcode(ScriptVersion::V4, V4Revision::Rev03, op).unwrap() as i32
    }

    fn compile(source: &str) -> ScriptFile {
        compile_object_script(source, &options()).unwrap()
    }

    fn compile_error(source: &str) -> CompileError {
        compile_object_script(source, &options()).unwrap_err()
    }

    fn compile_strict(source: &str) -> Result<ScriptFile, CompileError> {
        let mut options = options();
        options.strict = true;
        compile_object_script(source, &options)
    }

    #[test]
    fn empty_script_has_one_empty_object() {
        let file = compile("");
        assert!(file.code.is_empty());
        assert!(file.functions.is_empty());
        assert_eq!(file.object_scripts.len(), 1);
        assert_eq!(file.object_scripts[0].update.code_pos, EMPTY_EVENT);
        assert_eq!(file.object_scripts[0].update.jump_pos, EMPTY_JUMP_POS);
    }

    #[test]
    fn assignment_and_arithmetic_emit_expected_opcodes() {
        let file = compile(
            "event ObjectUpdate\n\
             temp0 = 5\n\
             temp0 += 2\n\
             temp0 -= 1\n\
             temp0 *= 3\n\
             temp0 /= 4\n\
             temp0 %= 5\n\
             temp0 <<= 1\n\
             temp0 >>= 1\n\
             temp0 &= 6\n\
             temp0 |= 7\n\
             temp0 ^= 8\n\
             temp0++\n\
             temp0--\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::Equal),
                1,
                0,
                0,
                2,
                5,
                op(Op::Add),
                1,
                0,
                0,
                2,
                2,
                op(Op::Sub),
                1,
                0,
                0,
                2,
                1,
                op(Op::Mul),
                1,
                0,
                0,
                2,
                3,
                op(Op::Div),
                1,
                0,
                0,
                2,
                4,
                op(Op::Mod),
                1,
                0,
                0,
                2,
                5,
                op(Op::ShL),
                1,
                0,
                0,
                2,
                1,
                op(Op::ShR),
                1,
                0,
                0,
                2,
                1,
                op(Op::And),
                1,
                0,
                0,
                2,
                6,
                op(Op::Or),
                1,
                0,
                0,
                2,
                7,
                op(Op::Xor),
                1,
                0,
                0,
                2,
                8,
                op(Op::Inc),
                1,
                0,
                0,
                op(Op::Dec),
                1,
                0,
                0,
                op(Op::End),
            ]
        );
        assert_eq!(file.object_scripts[0].update.code_pos, 0);
        assert_eq!(file.object_scripts[0].update.jump_pos, 0);
    }

    #[test]
    fn negative_and_hex_literals() {
        let file = compile(
            "event ObjectUpdate\n\
             temp0 = -1\n\
             temp1 = 0x100\n\
             temp2 = 0b101\n\
             temp3 = 0o17\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::Equal),
                1,
                0,
                0,
                2,
                -1,
                op(Op::Equal),
                1,
                0,
                1,
                2,
                0x100,
                op(Op::Equal),
                1,
                0,
                2,
                2,
                5,
                op(Op::Equal),
                1,
                0,
                3,
                2,
                15,
                op(Op::End),
            ]
        );
    }

    #[test]
    fn if_else_if_else_patches_jump_table() {
        let file = compile(
            "event ObjectUpdate\n\
             if temp0 == 5\n\
             temp1 = 1\n\
             else\n\
             temp1 = 2\n\
             end if\n\
             end event\n",
        );
        // IfEqual(jpos=0, temp0, 5), Equal(temp1, 1), else, Equal(temp1, 2), endif, End
        assert_eq!(
            file.code,
            vec![
                op(Op::IfEqual),
                2,
                0,
                1,
                0,
                0,
                2,
                5,
                op(Op::Equal),
                1,
                0,
                1,
                2,
                1,
                op(Op::Else),
                op(Op::Equal),
                1,
                0,
                1,
                2,
                2,
                op(Op::EndIf),
                op(Op::End),
            ]
        );
        // [else position, endif position]
        assert_eq!(file.jump_table, vec![15, 22]);
    }

    #[test]
    fn if_without_else_points_one_before_endif() {
        let file = compile(
            "event ObjectUpdate\n\
             if temp0 > 5\n\
             temp1 = 1\n\
             end if\n\
             end event\n",
        );
        // [endif - 1, endif]
        assert_eq!(file.jump_table, vec![14, 15]);
    }

    #[test]
    fn if_comparison_operators() {
        for (source, expected) in [
            ("if temp0 == 1", Op::IfEqual),
            ("if temp0 > 1", Op::IfGreater),
            ("if temp0 >= 1", Op::IfGreaterOrEqual),
            ("if temp0 < 1", Op::IfLower),
            ("if temp0 <= 1", Op::IfLowerOrEqual),
            ("if temp0 != 1", Op::IfNotEqual),
        ] {
            let file = compile(&format!(
                "event ObjectUpdate\n{source}\nend if\nend event\n"
            ));
            assert_eq!(file.code[0], op(expected), "{source}");
        }
    }

    #[test]
    fn while_loop_uses_loop_marker() {
        let file = compile(
            "event ObjectUpdate\n\
             while temp0 < 5\n\
             temp0++\n\
             loop\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::WLower),
                2,
                0,
                1,
                0,
                0,
                2,
                5,
                op(Op::Inc),
                1,
                0,
                0,
                op(Op::Loop),
                op(Op::End),
            ]
        );
        // [loop start, after loop]
        assert_eq!(file.jump_table, vec![0, 13]);
    }

    #[test]
    fn switch_with_cases_and_default() {
        let file = compile(
            "event ObjectUpdate\n\
             switch temp0\n\
             case 1\n\
             temp1 = 1\n\
             case 2\n\
             temp1 = 2\n\
             default\n\
             temp1 = 3\n\
             end switch\n\
             end event\n",
        );
        // switch(0, temp0), Equal(temp1,1), Equal(temp1,2), Equal(temp1,3), endswitch, End
        assert_eq!(
            file.code,
            vec![
                op(Op::Switch),
                2,
                0,
                1,
                0,
                0,
                op(Op::Equal),
                1,
                0,
                1,
                2,
                1,
                op(Op::Equal),
                1,
                0,
                1,
                2,
                2,
                op(Op::Equal),
                1,
                0,
                1,
                2,
                3,
                op(Op::EndSwitch),
                op(Op::End),
            ]
        );
        // [min, max, default, end, case1, case2]
        assert_eq!(file.jump_table.len(), 6);
        assert_eq!(file.jump_table[0], 1);
        assert_eq!(file.jump_table[1], 2);
        // default is at code offset 18 (before Equal(temp1,3))
        assert_eq!(file.jump_table[2], 18);
        // endswitch is after the EndSwitch opcode at 24
        assert_eq!(file.jump_table[3], 25);
        assert_eq!(file.jump_table[4], 6);
        assert_eq!(file.jump_table[5], 12);
    }

    #[test]
    fn switch_without_default_falls_through_to_end() {
        let file = compile(
            "event ObjectUpdate\n\
             switch temp0\n\
             case 0\n\
             temp1 = 1\n\
             end switch\n\
             end event\n",
        );
        // No default: the default slot and unset cases fall through to endswitch - 1.
        assert_eq!(file.jump_table.len(), 5);
        assert_eq!(file.jump_table[2], file.jump_table[3] - 1);
        assert_eq!(file.jump_table[4], 6);
    }

    #[test]
    fn foreach_active_and_all() {
        for (suffix, expected) in [
            ("ACTIVE_ENTITIES", Op::ForEachActive),
            ("ALL_ENTITIES", Op::ForEachAll),
        ] {
            let file = compile(&format!(
                "event ObjectUpdate\n\
                 foreach (TypeName[Ring], arrayPos0, {suffix})\n\
                 temp0 = 1\n\
                 next\n\
                 end event\n"
            ));
            assert_eq!(file.code[0], op(expected), "{suffix}");
        }
    }

    #[test]
    fn strings_are_packed_with_closing_quote() {
        let file = compile(
            "event ObjectUpdate\n\
             LoadSpriteSheet(\"a.gif\")\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::LoadSpriteSheet),
                3,
                5,
                0x612E_6769,
                0x6622_0000,
                op(Op::End),
            ]
        );
    }

    #[test]
    fn string_length_multiple_of_four() {
        let file = compile(
            "event ObjectUpdate\n\
             LoadSpriteSheet(\"abcd\")\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::LoadSpriteSheet),
                3,
                4,
                0x6162_6364,
                0x2200_0000,
                op(Op::End),
            ]
        );
    }

    #[test]
    fn reserve_and_public_function_share_index() {
        let file = compile(
            "reserve function Foo\n\
             event ObjectUpdate\n\
             CallFunction(Foo)\n\
             end event\n\
             public function Foo\n\
             temp0 = 1\n\
             end function\n",
        );
        assert_eq!(file.functions.len(), 1);
        assert_eq!(file.functions[0].name, "Foo");
        assert_eq!(file.functions[0].access, Access::Public);
        assert_eq!(file.functions[0].code_pos, 4);
        assert_eq!(
            file.code,
            vec![
                op(Op::CallFunction),
                2,
                0,
                op(Op::End),
                op(Op::Equal),
                1,
                0,
                0,
                2,
                1,
                op(Op::Return),
            ]
        );
    }

    #[test]
    fn private_functions_do_not_leak_to_next_file() {
        let mut compiler = Compiler::new(&options());
        compiler
            .compile_file(Some("a.txt"), "private function Hidden\nend function\n")
            .unwrap();
        compiler
            .compile_file(Some("b.txt"), "public function Shown\nend function\n")
            .unwrap();
        let file = compiler.finish();
        assert_eq!(file.functions.len(), 2);
        assert!(file.functions[0].name.is_empty());
        assert_eq!(file.functions[1].name, "Shown");
    }

    #[test]
    fn aliases_values_and_tables_emit_data_words() {
        let file = compile(
            "private alias 5 : FIVE\n\
             public value COUNTER = 0x10\n\
             public table SIZES[2]\n\
             event ObjectUpdate\n\
             temp0 = FIVE\n\
             temp1 = COUNTER\n\
             temp2 = SIZES\n\
             end event\n",
        );
        // Scope-level data: COUNTER=0x10, SIZES=[2,0,0]
        assert_eq!(&file.code[..4], &[0x10, 2, 0, 0]);
        // Equal(temp0, 5), Equal(temp1, local[0]), Equal(temp2, table index 1)
        assert_eq!(
            file.code[4..],
            [
                op(Op::Equal),
                1,
                0,
                0,
                2,
                5,
                op(Op::Equal),
                1,
                0,
                1,
                1,
                1,
                0,
                0,
                18,
                op(Op::Equal),
                1,
                0,
                2,
                2,
                1,
                op(Op::End),
            ]
        );
    }

    #[test]
    fn thirty_two_character_alias_names_resolve() {
        // The decomp reference stores alias names in `char name[0x20]`; a 32-character name has
        // no room for a NUL terminator, so the reference's `StrComp` reads into the following
        // `value` field and the alias never matches (see the asset-gated
        // `mission_zone02_long_alias_divergence` test). This port uses `String` and resolves it,
        // matching the shipped `_Bytecode`.
        let file = compile(
            "private alias 5 : EGGMANSIGNPOST_SPAWNFALLSIGNPOST\n\
             event ObjectUpdate\n\
             temp0 = EGGMANSIGNPOST_SPAWNFALLSIGNPOST\n\
             end event\n",
        );
        assert_eq!(file.code, vec![op(Op::Equal), 1, 0, 0, 2, 5, op(Op::End)]);
    }

    #[test]
    fn table_with_default_values_reads_lines() {
        let file = compile(
            "private table VALUES\n\
             1, 2, 3\n\
             4, 5\n\
             end table\n\
             event ObjectUpdate\n\
             temp0 = VALUES\n\
             end event\n",
        );
        // VALUES = [5, 1, 2, 3, 4, 5] then Equal(temp0, table index 0) and End
        assert_eq!(&file.code[..6], &[5, 1, 2, 3, 4, 5]);
        assert_eq!(file.code[6..], [op(Op::Equal), 1, 0, 0, 2, 0, op(Op::End)]);
    }

    #[test]
    fn array_pos_aliases_and_entity_offsets() {
        let file = compile(
            "private alias arrayPos6 : currentPlayer\n\
             event ObjectUpdate\n\
             temp0 = object.value0[arrayPos6]\n\
             temp1 = object.value1[+2]\n\
             temp2 = object.value2[-currentPlayer]\n\
             temp3 = global[7]\n\
             end event\n",
        );
        assert_eq!(
            file.code,
            vec![
                op(Op::Equal),
                1,
                0,
                0,
                1,
                1,
                1,
                6,
                73, // object.value0[arrayPos6]
                op(Op::Equal),
                1,
                0,
                1,
                1,
                2,
                0,
                2,
                74, // object.value1[+2]
                op(Op::Equal),
                1,
                0,
                2,
                1,
                3,
                1,
                6,
                75, // object.value2[-arrayPos6]
                op(Op::Equal),
                1,
                0,
                3,
                1,
                1,
                0,
                7,
                17, // global[7]
                op(Op::End),
            ]
        );
    }

    #[test]
    fn platform_blocks_select_by_mode() {
        let source = "event ObjectUpdate\n\
                      #platform: USE_STANDALONE\n\
                      temp0 = 1\n\
                      #endplatform\n\
                      #platform: USE_ORIGINS\n\
                      temp0 = 2\n\
                      #endplatform\n\
                      end event\n";
        let origins = compile_object_script(source, &options()).unwrap();
        assert_eq!(
            origins.code,
            vec![op(Op::Equal), 1, 0, 0, 2, 2, op(Op::End)]
        );
        let mut standalone_options = options();
        standalone_options.platform = PlatformMode::Standalone;
        let standalone = compile_object_script(source, &standalone_options).unwrap();
        assert_eq!(
            standalone.code,
            vec![op(Op::Equal), 1, 0, 0, 2, 1, op(Op::End)]
        );
    }

    #[test]
    fn nested_platform_blocks_are_skipped_as_a_unit() {
        let source = "event ObjectUpdate\n\
                      #platform: USE_STANDALONE\n\
                      temp0 = 1\n\
                      #platform: USE_ORIGINS\n\
                      temp0 = 2\n\
                      #endplatform\n\
                      temp0 = 3\n\
                      #endplatform\n\
                      temp0 = 4\n\
                      end event\n";
        let file = compile(source);
        assert_eq!(file.code, vec![op(Op::Equal), 1, 0, 0, 2, 4, op(Op::End)]);
    }

    #[test]
    fn block_comments_are_stripped_without_shifting_lines() {
        let file = compile(
            "event ObjectUpdate\n\
             /* temp0 = 1\n\
             still a comment */ temp0 = 2\n\
             end event\n",
        );
        assert_eq!(file.code, vec![op(Op::Equal), 1, 0, 0, 2, 2, op(Op::End)]);
    }

    #[test]
    fn symbol_tables_resolve_type_and_sfx_names() {
        let mut options = options();
        options.symbols.object_types = vec![
            "BlankObject".to_string(),
            "PlayerObject".to_string(),
            "Ring".to_string(),
        ];
        options.symbols.sfx_names = vec!["Jump".to_string(), "Ring".to_string()];
        options.symbols.global_variables = vec!["player.lives".to_string()];
        let file = compile_object_script(
            "event ObjectUpdate\n\
             CreateTempObject(TypeName[Ring], 0, temp0, temp1)\n\
             PlaySfx(SfxName[Ring], false)\n\
             temp0 = VarName[player.lives]\n\
             end event\n",
            &options,
        )
        .unwrap();
        assert_eq!(file.code[0], op(Op::CreateTempObject));
        assert_eq!(&file.code[1..4], &[2, 2, 2]); // type index 2
        assert_eq!(file.code[11], op(Op::PlaySfx));
        assert_eq!(&file.code[12..14], &[2, 1]); // sfx index 1
        assert_eq!(
            &file.code[16..],
            &[op(Op::Equal), 1, 0, 0, 2, 0, op(Op::End)]
        ); // VarName -> int const 0
    }

    #[test]
    fn base_positions_offset_every_position() {
        let mut options = options();
        options.base_code_pos = 1000;
        options.base_jump_pos = 200;
        let file = compile_object_script(
            "event ObjectUpdate\n\
             if temp0 == 1\n\
             temp0 = 2\n\
             end if\n\
             end event\n",
            &options,
        )
        .unwrap();
        assert_eq!(file.code.len(), 16);
        assert_eq!(file.object_scripts[0].update.code_pos, 1000);
        assert_eq!(file.object_scripts[0].update.jump_pos, 200);
        // Relative jump entries are unaffected by the base positions.
        assert_eq!(file.jump_table, vec![14, 15]);
    }

    #[test]
    fn compile_group_links_files_and_merges_functions() {
        let files = [
            ("a.txt", "public function Shared\ntemp0 = 1\nend function\n"),
            (
                "b.txt",
                "event ObjectUpdate\nCallFunction(Shared)\nend event\n",
            ),
        ];
        let file = compile_group(&files, &options()).unwrap();
        assert_eq!(file.functions.len(), 1);
        assert_eq!(file.functions[0].code_pos, 0);
        // b.txt's event starts after a.txt's function (6 words + return)
        assert_eq!(file.object_scripts.len(), 2);
        assert_eq!(file.object_scripts[0].update.code_pos, EMPTY_EVENT);
        assert_eq!(file.object_scripts[1].update.code_pos, 7);
    }

    #[test]
    fn finish_group_keeps_global_functions() {
        let mut compiler = Compiler::new(&options());
        compiler
            .compile_file(
                Some("global.txt"),
                "public function Global\ntemp0 = 1\nend function\n",
            )
            .unwrap();
        let mark = compiler.mark();
        compiler
            .compile_file(
                Some("stage.txt"),
                "event ObjectUpdate\nCallFunction(Global)\nend event\n",
            )
            .unwrap();
        let stage = compiler.finish_group(mark);
        assert_eq!(stage.functions.len(), 1);
        assert_eq!(stage.object_scripts.len(), 1);
        assert_eq!(stage.code.len(), 4);
        assert_eq!(stage.code, vec![op(Op::CallFunction), 2, 0, op(Op::End)]);
    }

    #[test]
    fn return_statement_emits_return_opcode() {
        let file = compile("event ObjectUpdate\nreturn\nend event\n");
        assert_eq!(file.code, vec![op(Op::Return), op(Op::End)]);
    }

    #[test]
    fn not_operator_is_a_function_call() {
        let file = compile("event ObjectUpdate\nNot(temp0)\nend event\n");
        assert_eq!(file.code, vec![op(Op::Not), 1, 0, 0, op(Op::End)]);
    }

    #[test]
    fn editor_events_are_ignored() {
        // `event RSDKDraw`/`event RSDKLoad` are editor-only; upstream leaves them in scope mode
        // and silently ignores the body lines.
        let file = compile(
            "event RSDKDraw\nDrawSprite(0)\nend event\nevent RSDKLoad\nLoadSpriteSheet(\"a.gif\")\nend event\n",
        );
        assert!(file.code.is_empty());
        assert_eq!(file.object_scripts[0].update.code_pos, EMPTY_EVENT);
    }

    #[test]
    fn platform_blocks_inside_a_switch_case() {
        let source = "event ObjectUpdate\n\
                      switch temp0\n\
                      case 0\n\
                      #platform: USE_STANDALONE\n\
                      temp1 = 1\n\
                      #endplatform\n\
                      #platform: USE_ORIGINS\n\
                      temp1 = 2\n\
                      #endplatform\n\
                      end switch\n\
                      end event\n";
        let origins = compile(source);
        assert!(
            origins
                .code
                .windows(6)
                .any(|window| window == [op(Op::Equal), 1, 0, 1, 2, 2])
        );
        let mut standalone_options = options();
        standalone_options.platform = PlatformMode::Standalone;
        let standalone = compile_object_script(source, &standalone_options).unwrap();
        assert!(
            standalone
                .code
                .windows(6)
                .any(|window| window == [op(Op::Equal), 1, 0, 1, 2, 1])
        );
    }

    #[test]
    fn unknown_opcode_reports_line() {
        let error = compile_error("event ObjectUpdate\nNotAThing(1)\nend event\n");
        assert_eq!(error.line, 2);
        assert!(
            error.message.contains("opcode not found"),
            "{}",
            error.message
        );
    }

    #[test]
    fn unknown_operand_reports_line() {
        let error = compile_error("event ObjectUpdate\ntemp0 = NotAnAlias\nend event\n");
        assert_eq!(error.line, 2);
        assert!(
            error.message.contains("operand not found"),
            "{}",
            error.message
        );
    }

    #[test]
    fn unterminated_string_reports_line() {
        let error = compile_error("event ObjectUpdate\nLoadSpriteSheet(\"abc\nend event\n");
        assert_eq!(error.line, 2);
        assert!(
            error.message.contains("unterminated string"),
            "{}",
            error.message
        );
    }

    #[test]
    fn missing_end_if_is_a_strict_error_at_the_opening_line() {
        let source = "event ObjectUpdate\nif temp0 == 1\ntemp0 = 2\nend event\n";
        let _ = compile(source);
        let error = compile_strict(source).unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("end if"), "{}", error.message);
    }

    #[test]
    fn platform_without_end_is_a_strict_error_at_the_opening_line() {
        let source = "event ObjectUpdate\n#platform: USE_STANDALONE\ntemp0 = 1\n";
        let _ = compile(source);
        let error = compile_strict(source).unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("#endplatform"), "{}", error.message);
    }

    #[test]
    fn bad_alias_is_a_strict_error_at_the_declaration_line() {
        let source = "private alias 5 FIVE\nevent ObjectUpdate\nend event\n";
        let _ = compile(source);
        let error = compile_strict(source).unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.message.contains("alias"), "{}", error.message);
    }

    #[test]
    fn line_numbers_follow_logical_lines() {
        let error = compile_error(
            "// comment\n\
             event ObjectUpdate\n\
             temp0 = 1; temp0 = NotAnAlias\n\
             end event\n",
        );
        // The `;` splits the line without incrementing the counter, so both parts are line 3.
        assert_eq!(error.line, 3);
    }

    #[test]
    fn missing_end_event_is_a_strict_error() {
        let source = "event ObjectUpdate\ntemp0 = 1\n";
        let _ = compile(source);
        let error = compile_strict(source).unwrap_err();
        assert!(error.message.contains("end event"), "{}", error.message);
    }
}

#[cfg(test)]
mod reserved_tests {
    use super::*;

    #[test]
    fn reserved_functions_default_to_the_empty_event_pointers() {
        let file = compile_object_script(
            "reserve function NeverDefined\nevent ObjectUpdate\nend event\n",
            &CompileOptions::default(),
        )
        .unwrap();
        assert_eq!(file.functions.len(), 1);
        assert_eq!(file.functions[0].code_pos, EMPTY_EVENT);
        assert_eq!(file.functions[0].jump_pos, EMPTY_JUMP_POS);
    }

    #[test]
    fn defined_reserved_function_gets_real_positions() {
        let file = compile_object_script(
            "reserve function Later\npublic function Later\ntemp0 = 1\nend function\n",
            &CompileOptions::default(),
        )
        .unwrap();
        assert_eq!(file.functions.len(), 1);
        assert_eq!(file.functions[0].code_pos, 0);
        assert_eq!(file.functions[0].jump_pos, 0);
    }
}
