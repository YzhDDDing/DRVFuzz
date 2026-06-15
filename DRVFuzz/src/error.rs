use std::path::PathBuf;
use std::time::Duration;

use riscv_instruction_types::RandomGenerationError;
use thiserror::Error;

/// Error types that may occur while building an ELF.
#[derive(Debug, Error)]
pub enum BuildElfError {
    #[error("failed to write linker script at {path}: {source}")]
    LinkerScriptWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to execute `{command}` during {stage}: {source}")]
    CommandSpawn {
        stage: &'static str,
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`{command}` failed during {stage}: {stderr}")]
    CommandFailure {
        stage: &'static str,
        command: String,
        stderr: String,
    },
    #[error("failed to write disassembly output at {path}: {source}")]
    DisassemblyWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to build -march string: {source}")]
    MarchBuild {
        #[source]
        source: BuildMarchError,
    },
    #[error("failed to build -mabi string: {source}")]
    MabiBuild {
        #[source]
        source: BuildMabiError,
    },
}

/// Errors that can occur while constructing an `-mabi` string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BuildMabiError {
    /// Unsopported ISA base.
    #[error("unsupported ISA base")]
    UnsupportedIsaBase,
    /// No base ABI (e.g. `ilp32`, `lp64`) was supplied.
    #[error("missing base ABI option")]
    MissingBase,
    /// Two incompatible base ABIs were supplied simultaneously.
    #[error("conflicting base ABI options: `{existing}` vs `{requested}`")]
    ConflictingBase { existing: String, requested: String },
    /// A floating-point extension requires another extension to be present.
    #[error("extension `{ext}` requires `{required}` for ABI selection")]
    FloatRequires {
        ext: &'static str,
        required: &'static str,
    },
}

/// Errors that can occur while constructing a `-march` string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BuildMarchError {
    /// Unsopported ISA base.
    #[error("unsupported ISA base")]
    UnsupportedIsaBase,
    /// The input set does not contain a base ISA (`I` or `E`).
    #[error("missing base ISA extension (expected I or E)")]
    MissingBaseIsa,
    /// Both `I` and `E` were supplied, which is invalid.
    #[error("conflicting base ISA extensions (I and E)")]
    ConflictingBaseIsa,
    /// An extension requires another extension that was not supplied.
    #[error("extension `{ext}` requires `{required}`")]
    ExtensionRequires { ext: String, required: String },
    /// Two extensions cannot be enabled together.
    #[error("extensions `{left}` and `{right}` conflict")]
    ExtensionsConflict { left: String, right: String },
    /// An extension is only available for a specific XLEN.
    #[error("extension `{ext}` is only valid for rv{required_xlen}")]
    ExtensionOnlyForXlen { ext: String, required_xlen: u32 },
    /// The requested extension is not recognized by the GNU toolchain rules.
    #[error("extension `{ext}` is not supported by the GNU toolchain")]
    UnsupportedExtension { ext: String },
}

/// Errors encountered while normalizing execution output.
#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("memory address 0x{addr:x} is below expected start 0x{start:x}")]
    MemoryBelowStart { addr: u64, start: u64 },
}

/// Errors that can occur while constructing the execution context.
#[derive(Debug, Error)]
pub enum ContextBuildError {
    #[error("register and memory change lengths differ: registers={registers}, memory={memory}")]
    WriteVectorLengthMismatch { registers: usize, memory: usize },

    #[error("user instruction missing for index {index}")]
    MissingInstruction { index: usize },

    #[error("execution context missing for instruction index {index}")]
    MissingContext { index: usize },

    #[error(
        "instruction metadata length mismatch: instructions={instructions}, metadata={metadata}"
    )]
    InstructionMetadataLengthMismatch {
        instructions: usize,
        metadata: usize,
    },

    #[error(transparent)]
    ContextExtraction(#[from] ContextExtractionError),
}

/// Errors produced during random instruction generation.
#[derive(Debug, Error)]
pub enum GenerationError {
    #[error("need at least {required} temporary registers, but only {available} available")]
    InsufficientTempRegisters { required: usize, available: usize },

    #[error("memory range not found for RISC-V implementation: {impl_name}")]
    MemRangeNotFound { impl_name: String },

    #[error("memory range configuration failed: {source}")]
    MemRange {
        #[from]
        #[source]
        source: MemRangeError,
    },

    #[error("failed to generate random instruction sequences: {source}")]
    RandomSequence {
        #[from]
        #[source]
        source: RandomGenerationError,
    },

    #[error(
        "memory access offset range [{min}, {max}] is incompatible with mem_size {mem_size} and max access width {width}"
    )]
    InvalidMemAccessOffset {
        min: i64,
        max: i64,
        mem_size: u64,
        width: u64,
    },

    #[error("memory start address {addr:#x} exceeds supported range")]
    MemoryAddressOutOfRange { addr: u64 },

    #[error("memory range map is empty")]
    EmptyMemRange,

    #[error("extension instruction scaling min {min} exceeds max {max}")]
    InvalidExtensionScaling { min: usize, max: usize },
}

/// Errors related to configuring memory ranges.
#[derive(Debug, Error)]
pub enum MemRangeError {
    #[error("no RISC-V implementations available to compute memory range")]
    NoImplementations,

    #[error("requested memory size {requested} exceeds minimum available memory size {available}")]
    MemorySizeExceedsAvailable { requested: u64, available: u64 },

    #[error("requested memory size {size} is not aligned to word size {word_size}")]
    MemoryNotWordAligned { size: u64, word_size: u64 },

    #[error(
        "requested memory size {size} is less than the maximum instruction access width {width}"
    )]
    MemorySizeTooSmall { size: u64, width: u64 },

    #[error("memory range size {actual} should equal mem_size {expected}")]
    MemoryRangeSizeMismatch { actual: u64, expected: u64 },
}

/// Execution errors for a specific RISC-V implementation.
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("implementation {impl_name} does not support ISA base {isa_base}")]
    UnsupportedIsaBase { impl_name: String, isa_base: String },

    #[error(
        "implementation {impl_name} does not support the requested unaligned access requirement ({allow_unaligned})"
    )]
    UnsupportedAlignmentMode {
        impl_name: String,
        allow_unaligned: bool,
    },

    #[error("execution failed for implementation {impl_name} with ISA base {isa_base}: {source}")]
    ImplementationFailed {
        impl_name: String,
        isa_base: String,
        #[source]
        source: ImplExecutionError,
    },
}

/// Regular expression errors.
#[derive(Debug, Error)]
pub enum RegexError {
    #[error("failed to compile regex pattern '{pattern}': {source}")]
    CompilationFailed {
        pattern: String,
        #[source]
        source: regex::Error,
    },
}

/// Reasons for failing to extract error context.
#[derive(Debug, Error)]
pub enum ContextExtractionError {
    #[error(transparent)]
    Regex(#[from] RegexError),

    #[error("failed to parse memory offset '{text}': {source}")]
    OffsetParseError {
        text: String,
        #[source]
        source: std::num::ParseIntError,
    },
}

/// String parsing errors.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("empty string provided where character expected")]
    EmptyString,

    #[error("invalid register name format: {name}")]
    InvalidRegisterName { name: String },

    #[error("failed to parse hex value '{value}': {source}")]
    HexParseError {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("failed to parse integer value '{value}': {source}")]
    IntParseError {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("failed to parse program counter from '{text}'")]
    PcParseError { text: String },

    #[error("failed to parse register number from '{text}'")]
    RegisterNumberParseError { text: String },

    #[error("failed to parse memory address from '{text}'")]
    AddressParseError { text: String },

    #[error("failed to parse value from '{text}'")]
    ValueParseError { text: String },
}

/// Log parsing errors.
#[derive(Debug, Error)]
pub enum LogParseError {
    #[error("failed to compile regex pattern '{pattern}': {source}")]
    RegexCompilationFailed {
        pattern: String,
        #[source]
        source: regex::Error,
    },

    #[error("failed to match pattern in line: {line}")]
    PatternMatchFailed { line: String },

    #[error("missing required capture group '{group}' in line: {line}")]
    MissingCaptureGroup { group: String, line: String },

    #[error("invalid line format: {line}")]
    InvalidLineFormat { line: String },

    #[error("failed to parse value from capture: {source}")]
    CaptureParseError {
        #[source]
        source: ParseError,
    },

    #[error("failed to read log file at {path}: {source}")]
    LogFileReadError {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no PC mapping found for user instruction index {index}")]
    NoPcMappingFound { index: usize },
}

/// Process execution errors.
#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to spawn process '{command}': {source}")]
    SpawnFailed {
        command: String,
        #[source]
        source: std::io::Error,
    },

    #[error("process '{command}' failed with stderr: {stderr}")]
    ProcessFailed { command: String, stderr: String },

    #[error("process '{command}' timed out after {timeout:?}")]
    TimedOut { command: String, timeout: Duration },

    #[error("failed to create directory at {path}: {source}")]
    DirectoryCreationFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write file at {path}: {source}")]
    FileWriteFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to open log file at {path}: {source}")]
    LogFileOpenFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// ELF/disassembly related errors.
#[derive(Debug, Error)]
pub enum ElfError {
    #[error("failed to load ELF dump from {path}: {source}")]
    DumpLoadFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to build ELF: {source}")]
    BuildFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Configuration errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("ISA base {isa_base} is not supported by {impl_name}")]
    UnsupportedIsaBase { impl_name: String, isa_base: String },

    #[error("invalid configuration: {message}")]
    InvalidConfiguration { message: String },
}

/// Spike execution errors (aggregating all sub-errors).
#[derive(Debug, Error)]
pub enum SpikeError {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("Spike binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),
}

/// Rocket execution errors.
#[derive(Debug, Error)]
pub enum RocketError {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("Rocket binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),

    #[error("no PC mapping found for user instruction index {index}")]
    NoPcMapping { index: usize },
}

/// Boom execution errors.
#[derive(Debug, Error)]
pub enum BoomError {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("Boom binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),

    #[error("no PC mapping found for user instruction index {index}")]
    NoPcMapping { index: usize },

    #[error("memory trace encountered without a current PC: {line}")]
    MissingPcForMemoryTrace { line: String },
}

/// PicoRV32 execution errors.
#[derive(Debug, Error)]
pub enum PicoRV32Error {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("PicoRV32 binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error("PicoRV32 only supports RV32, got {isa_base}")]
    UnsupportedIsaBase { isa_base: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),
}

/// Srv32 execution errors.
#[derive(Debug, Error)]
pub enum Srv32Error {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("Srv32 binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error("Srv32 only supports RV32, got {isa_base}")]
    UnsupportedIsaBase { isa_base: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),
}

/// CVA6 execution errors.
#[derive(Debug, Error)]
pub enum CVA6Error {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("CVA6 binary not found: {path}")]
    BinaryNotFound { path: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),
}

/// XiangShan execution errors.
#[derive(Debug, Error)]
pub enum XiangShanError {
    #[error("environment variable {var} is not set")]
    EnvVarNotSet { var: String },

    #[error("XiangShan emulator not found: {path}")]
    BinaryNotFound { path: String },

    #[error("DiffTest reference library not found: {path}")]
    DiffSoNotFound { path: String },

    #[error(transparent)]
    ProcessError(#[from] ProcessError),

    #[error(transparent)]
    LogParseError(#[from] LogParseError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error(transparent)]
    ElfError(#[from] ElfError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    BuildElf(#[from] BuildElfError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("failed to build ELF: {0}")]
    BuildElfError(String),

    #[error("XiangShan commit trace is empty")]
    EmptyTrace,

    #[error("user PC mapping missing for instruction index {index}")]
    NoPcMapping { index: usize },

    #[error("XiangShan emulator exited with status {status:?}. See logs for details.")]
    NonZeroExit {
        status: Option<i32>,
        stdout: String,
        stderr: String,
    },
}

/// Unified execution error type used across all RISC-V implementations.
#[derive(Debug, Error)]
pub enum ImplExecutionError {
    #[error(transparent)]
    Spike(#[from] SpikeError),

    #[error(transparent)]
    Rocket(#[from] RocketError),

    #[error(transparent)]
    Boom(#[from] BoomError),

    #[error(transparent)]
    PicoRV32(#[from] PicoRV32Error),

    #[error(transparent)]
    Srv32(#[from] Srv32Error),

    #[error(transparent)]
    CVA6(#[from] CVA6Error),

    #[error(transparent)]
    XiangShan(#[from] XiangShanError),

    #[error(transparent)]
    Process(#[from] ProcessError),

    #[error(transparent)]
    LogParse(#[from] LogParseError),

    #[error(transparent)]
    Elf(#[from] ElfError),

    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("generic error: {0}")]
    Generic(String),
}
