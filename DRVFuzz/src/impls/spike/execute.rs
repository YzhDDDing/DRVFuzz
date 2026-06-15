use std::cmp::Ordering;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use super::march;

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{ElfError, LogParseError, ParseError, ProcessError, SpikeError},
    exception_cause::{canonical_cause_code_from_name, format_cause_code},
    execution_output::{
        ExceptionInfo as TraitExceptionInfo, ExecutionOutput, MemValue as TraitMemValue,
        RegisterValue as TraitRegisterValue,
    },
    extension_map::ExtensionMap,
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
    tracer::Tracer,
    user_instruction::UserInstructionInfo,
};

pub(crate) const SPIKE_BINARY_ENV: &str = "RISCV_WRAPPER_SPIKE_BIN";

/// Check if a command is available, either as a file path or in PATH
fn is_command_available(path_or_command: &Path) -> bool {
    // If it looks like a file path (contains directory separator or is absolute), check if file exists
    if path_or_command.components().count() > 1 || path_or_command.is_absolute() {
        return path_or_command.exists();
    }

    // Otherwise, search in PATH
    if let Some(path_env) = env::var_os("PATH") {
        for dir in env::split_paths(&path_env) {
            let candidate = dir.join(path_or_command);
            if candidate.exists() {
                return true;
            }
        }
    }

    false
}

/// Register write entry extracted from a Spike log.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
}

/// Memory write entry extracted from a Spike log.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub width: usize,
}

/// Exception entry extracted from a Spike log.
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub cause: String,
    pub pc: u64,
    pub mcause: Option<u64>,
}

/// Parsed Spike log result.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SpikeTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>,
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

/// Basic configuration describing the environment needed to run Spike.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub spike_args: Vec<String>,
    pub timeout: Option<Duration>,
    extension_override: Option<ExtensionMap>,
}

impl ExecutorConfig {
    pub fn new(work_dir: PathBuf, isa_base: ISABase, riscv_impl: RiscVImpl) -> Self {
        Self {
            work_dir,
            isa_base,
            riscv_impl,
            spike_args: Vec::new(),
            timeout: None,
            extension_override: None,
        }
    }

    pub fn set_extension_override(&mut self, extension_map: ExtensionMap) {
        self.extension_override = Some(extension_map);
    }

    fn extension_map(&self) -> ExtensionMap {
        if let Some(ref override_map) = self.extension_override {
            override_map.clone()
        } else {
            self.riscv_impl.extension_map()
        }
    }
}

/// Spike executor that only handles compile-and-run (parsing handled separately).
#[derive(Debug)]
pub struct SpikeExecutor {
    config: ExecutorConfig,
}

impl SpikeExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Compile assembly and run Spike, returning raw output and build artifacts.
    pub fn compile_and_run_with_dir(
        &self,
        run_dir: &Path,
        user_insts: &[String],
    ) -> Result<SpikeRunResult, SpikeError> {
        info!(
            "Starting Spike compile-and-run with {} user instructions in {}",
            user_insts.len(),
            run_dir.display()
        );
        self.ensure_work_dir(run_dir)?;

        let extension_map = self.config.extension_map();
        let program = self.build_program(user_insts)?;
        let asm_path = self.write_program(run_dir, &program)?;
        let build = build_elf_with_extensions(
            &asm_path,
            &extension_map,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|err| {
            error!(
                "Failed to build ELF for Spike execution (asm={}, isa={:?}, impl={:?}): {:#}",
                asm_path.display(),
                self.config.isa_base,
                self.config.riscv_impl,
                err
            );
            err
        })?;
        info!("ELF build completed at {}", build.executable_file.display());

        let isa_string = match self.config.isa_base {
            ISABase::Rv32 => march::isa_from_rv32_extensions(&extension_map.rv32),
            ISABase::Rv64 => march::isa_from_rv64_extensions(&extension_map.rv64),
        }
        .map_err(|err| {
            error!(
                "Failed to derive ISA string for Spike (isa={:?}): {:#}",
                self.config.isa_base, err
            );
            SpikeError::BuildElfError(format!("Failed to derive ISA string for Spike: {}", err))
        })?;

        // Append the Zicntr extension to isa_string (required by Spike)
        let isa_string = format!("{}_Zicntr", isa_string);
        let output = self.run_spike(run_dir, &build, &isa_string)?;
        info!("Spike finished with status {:?}", output.status.code());

        Ok(output)
    }

    /// Compile assembly and run Spike, returning raw output and build artifacts.
    pub fn compile_and_run(&self, user_insts: &[String]) -> Result<SpikeRunResult, SpikeError> {
        self.compile_and_run_with_dir(&self.config.work_dir, user_insts)
    }

    /// Compile only and return the built ELF artifacts.
    pub fn compile(&self, user_insts: &[String]) -> Result<ElfBuildResult, SpikeError> {
        info!(
            "Compiling Spike program with {} user instructions",
            user_insts.len()
        );
        self.ensure_work_dir(&self.config.work_dir)?;
        let extension_map = self.config.extension_map();
        let program = self.build_program(user_insts)?;
        let asm_path = self.write_program(&self.config.work_dir, &program)?;
        let build = build_elf_with_extensions(
            &asm_path,
            &extension_map,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|err| {
            error!(
                "Failed to build ELF for Spike (asm={}, isa={:?}, impl={:?}): {:#}",
                asm_path.display(),
                self.config.isa_base,
                self.config.riscv_impl,
                err
            );
            err
        })?;
        info!(
            "Spike compilation finished: {}",
            build.executable_file.display()
        );
        Ok(build)
    }

    fn ensure_work_dir(&self, dir: &Path) -> Result<(), SpikeError> {
        if dir.exists() {
            return Ok(());
        }

        fs::create_dir_all(dir).map_err(|err| {
            error!("Failed to create work directory {}: {}", dir.display(), err);
            SpikeError::from(err)
        })
    }

    fn write_program(&self, dir: &Path, program: &str) -> Result<PathBuf, SpikeError> {
        let asm_path = dir.join("program.S");
        fs::write(&asm_path, program).map_err(|err| {
            error!(
                "Failed to write assembly file {}: {}",
                asm_path.display(),
                err
            );
            err
        })?;
        debug!("Assembly written to {}", asm_path.display());
        Ok(asm_path)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<String, SpikeError> {
        // Use build_asm_content from DRVFuzz
        Ok(self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?)
    }

    fn run_spike(
        &self,
        run_dir: &Path,
        build: &ElfBuildResult,
        isa_string: &str,
    ) -> Result<SpikeRunResult, SpikeError> {
        let spike_binary = env::var(SPIKE_BINARY_ENV).map_err(|_| SpikeError::EnvVarNotSet {
            var: SPIKE_BINARY_ENV.to_string(),
        })?;
        let spike_binary = PathBuf::from(&spike_binary);

        // Check if executable is available (either as file path or in PATH)
        if !is_command_available(&spike_binary) {
            return Err(SpikeError::BinaryNotFound {
                path: spike_binary.display().to_string(),
            });
        }

        let mut cmd = Command::new(&spike_binary);
        cmd.arg(format!("--isa={}", isa_string.to_uppercase()));
        cmd.arg("--log-commits");
        cmd.arg("-l");
        cmd.args(&self.config.spike_args);
        cmd.arg(&build.executable_file);
        cmd.current_dir(run_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let command_display = format!("{:?}", cmd);
        info!("Launching Spike command: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            error!("Failed to execute spike command: {}", err);
            SpikeError::from(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let stdout_reader = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture Spike stdout")
        })?;
        let stderr_reader = child.stderr.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture Spike stderr")
        })?;

        let stdout_handle = thread::spawn(move || -> Result<Vec<u8>, io::Error> {
            let mut buffer = Vec::new();
            let mut reader = BufReader::new(stdout_reader);
            reader.read_to_end(&mut buffer)?;
            Ok(buffer)
        });
        let stderr_handle = thread::spawn(move || -> Result<Vec<u8>, io::Error> {
            let mut buffer = Vec::new();
            let mut reader = BufReader::new(stderr_reader);
            reader.read_to_end(&mut buffer)?;
            Ok(buffer)
        });

        let timeout = self.config.timeout;
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if let Some(limit) = timeout {
                if start.elapsed() >= limit {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait()?;
                }
            }
            thread::sleep(Duration::from_millis(50));
        };

        let stdout_bytes = stdout_handle
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "stdout reader panicked"))??;
        let stderr_bytes = stderr_handle
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "stderr reader panicked"))??;

        let stdout_path = run_dir.join("spike_stdout.log");
        let stderr_path = run_dir.join("spike_stderr.log");
        fs::write(&stdout_path, &stdout_bytes).map_err(|err| {
            error!(
                "Failed to persist Spike stdout to {}: {}",
                stdout_path.display(),
                err
            );
            SpikeError::from(ProcessError::FileWriteFailed {
                path: stdout_path.clone(),
                source: err,
            })
        })?;
        fs::write(&stderr_path, &stderr_bytes).map_err(|err| {
            error!(
                "Failed to persist Spike stderr to {}: {}",
                stderr_path.display(),
                err
            );
            SpikeError::from(ProcessError::FileWriteFailed {
                path: stderr_path.clone(),
                source: err,
            })
        })?;

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if timed_out {
            if let Some(limit) = timeout {
                error!("Spike execution timed out after {:?}", limit);
                return Err(SpikeError::from(ProcessError::TimedOut {
                    command: command_display,
                    timeout: limit,
                }));
            } else {
                unreachable!("timed_out implies timeout was set");
            }
        }

        Ok(SpikeRunResult {
            build: build.clone(),
            isa: isa_string.to_string(),
            status,
            stdout,
            stderr,
            stdout_path,
            stderr_path,
        })
    }
}

/// Spike execution result, keeping the raw outputs for later inspection.
#[derive(Debug)]
pub struct SpikeRunResult {
    pub build: ElfBuildResult,
    pub isa: String,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

/// Helper function to compile regex patterns with error handling
fn compile_regex(pattern: &str) -> Result<Regex, LogParseError> {
    Regex::new(pattern).map_err(|e| LogParseError::RegexCompilationFailed {
        pattern: pattern.to_string(),
        source: e,
    })
}

/// Parse Spike logs
fn parse_spike_log<R: BufRead>(reader: R) -> Result<SpikeTrace, SpikeError> {
    // Example Spike log format:
    // core   0: 0x0000000000001000 (0x00000297) auipc   t0, 0x0
    // core   0: 3 0x0000000000001000 (0x00000297) x5  0x0000000000001000
    // core   0: 3 0x000000000000100c (0x0182b283) x5  0x0000000080000000 mem 0x0000000000001018
    // core   0: exception trap_load_address_misaligned, epc 0x000000008000005c
    // core   0:           tval 0x0000000080000027

    // Match lines that start with "core <n>: 3 <pc> <instr>"
    static REG_CHANGE_LINE_RE: Lazy<Result<Regex, LogParseError>> = Lazy::new(|| {
        compile_regex(r"^core\s+\d+:\s+3\s+0x(?P<pc>[0-9a-fA-F]+)\s+\(0x(?P<instr>[0-9a-fA-F]+)\)")
    });

    // Match individual register writes within the line
    // Use word boundary to avoid matching hex addresses like "mem 0x8f869180"
    static REG_MATCH_RE: Lazy<Result<Regex, LogParseError>> =
        Lazy::new(|| compile_regex(r"\b(?P<prefix>[xf])(?P<reg>\d+)\s+0x(?P<val>[0-9a-fA-F]+)"));

    static MEM_RE: Lazy<Result<Regex, LogParseError>> = Lazy::new(|| {
        compile_regex(r"mem\s+0x(?P<addr>[0-9a-fA-F]+)(?:\s+0x(?P<data>[0-9a-fA-F]+))?")
    });

    static EXCEPTION_RE: Lazy<Result<Regex, LogParseError>> = Lazy::new(|| {
        compile_regex(r"^core\s+\d+:\s+exception\s+(?P<kind>\w+),\s+epc\s+0x(?P<pc>[0-9a-fA-F]+)")
    });

    let reg_write_line_re = compiled_regex(&REG_CHANGE_LINE_RE)?;
    let reg_match_re = compiled_regex(&REG_MATCH_RE)?;
    let mem_re = compiled_regex(&MEM_RE)?;
    let exception_re = compiled_regex(&EXCEPTION_RE)?;

    // Use intermediate HashMaps to ensure uniqueness (last write wins)
    let mut temp_reg_writes: HashMap<u64, HashMap<String, RegisterWrite>> = HashMap::new();
    let mut temp_mem_writes: HashMap<u64, HashMap<u64, MemoryWrite>> = HashMap::new();
    let mut exceptions: HashMap<u64, ExceptionEvent> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check for exceptions
        if let Some(cap) = exception_re.captures(trimmed) {
            let kind = cap
                .name("kind")
                .ok_or_else(|| {
                    SpikeError::from(LogParseError::MissingCaptureGroup {
                        group: "kind".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .to_string();
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    SpikeError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                SpikeError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let mcause = canonical_cause_code_from_name(&kind);
            exceptions.insert(
                pc,
                ExceptionEvent {
                    cause: kind,
                    pc,
                    mcause,
                },
            );
            continue;
        }

        // Check register/memory change lines
        if let Some(line_cap) = reg_write_line_re.captures(trimmed) {
            let pc_str = line_cap
                .name("pc")
                .ok_or_else(|| {
                    SpikeError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();

            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                SpikeError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            // Extract all register writes from this line (there may be multiple)
            for reg_cap in reg_match_re.captures_iter(trimmed) {
                let prefix = reg_cap
                    .name("prefix")
                    .ok_or_else(|| {
                        SpikeError::from(LogParseError::MissingCaptureGroup {
                            group: "prefix".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let reg_str = reg_cap
                    .name("reg")
                    .ok_or_else(|| {
                        SpikeError::from(LogParseError::MissingCaptureGroup {
                            group: "reg".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let val_str = reg_cap
                    .name("val")
                    .ok_or_else(|| {
                        SpikeError::from(LogParseError::MissingCaptureGroup {
                            group: "val".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();

                let reg_idx = reg_str.parse::<u8>().map_err(|e| {
                    SpikeError::from(ParseError::IntParseError {
                        value: format!("{}{}", prefix, reg_str),
                        source: e,
                    })
                })?;

                let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                    SpikeError::from(ParseError::HexParseError {
                        value: val_str.to_string(),
                        source: e,
                    })
                })?;

                let register_name = format!("{}{}", prefix, reg_idx);
                temp_reg_writes
                    .entry(pc)
                    .or_insert_with(HashMap::new)
                    .insert(
                        register_name.clone(),
                        RegisterWrite {
                            index: reg_idx,
                            pc,
                            register: register_name,
                            value,
                        },
                    );
            }

            // Check memory accesses—a line may contain multiple mem entries (e.g., cbo.zero emits 64 single-byte entries)
            for mem_cap in mem_re.captures_iter(trimmed) {
                let addr_str = mem_cap
                    .name("addr")
                    .ok_or_else(|| {
                        SpikeError::from(LogParseError::MissingCaptureGroup {
                            group: "addr".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let address = u64::from_str_radix(addr_str, 16).map_err(|e| {
                    SpikeError::from(ParseError::HexParseError {
                        value: addr_str.to_string(),
                        source: e,
                    })
                })?;

                if let Some(data_match) = mem_cap.name("data") {
                    // data_match contains only hexadecimal digits without the "0x" prefix
                    let digits = data_match.as_str();

                    if digits.is_empty() {
                        return Err(SpikeError::LogParseError(
                            LogParseError::InvalidLineFormat {
                                line: format!(
                                    "Empty data for memory store at PC 0x{:016x}, address 0x{:016x}: {}",
                                    pc, address, trimmed
                                ),
                            },
                        ));
                    }

                    // Compute byte width: every 2 hexadecimal characters equal 1 byte
                    let byte_width = (digits.len() + 1) / 2;

                    if byte_width > 8 {
                        return Err(SpikeError::LogParseError(
                            LogParseError::InvalidLineFormat {
                                line: format!(
                                    "Store width {} exceeds 8 bytes at PC 0x{:016x}: {}",
                                    byte_width, pc, trimmed
                                ),
                            },
                        ));
                    }

                    let store_value = u64::from_str_radix(digits, 16).map_err(|e| {
                        SpikeError::from(ParseError::HexParseError {
                            value: digits.to_string(),
                            source: e,
                        })
                    })?;

                    temp_mem_writes
                        .entry(pc)
                        .or_insert_with(HashMap::new)
                        .insert(
                            address,
                            MemoryWrite {
                                pc,
                                address,
                                value: store_value,
                                width: byte_width,
                            },
                        );
                }
            }

            continue;
        }
    }

    // Convert intermediate HashMaps to final Vec format
    let register_writes: HashMap<u64, Vec<RegisterWrite>> = temp_reg_writes
        .into_iter()
        .map(|(pc, reg_map)| (pc, reg_map.into_values().collect()))
        .collect();

    let memory_writes: HashMap<u64, Vec<MemoryWrite>> = temp_mem_writes
        .into_iter()
        .map(|(pc, mem_map)| (pc, mem_map.into_values().collect()))
        .collect();

    let total_reg_writes: usize = register_writes.values().map(|v| v.len()).sum();
    let total_mem_writes: usize = memory_writes.values().map(|v| v.len()).sum();
    let total_exceptions: usize = exceptions.len();

    debug!(
        "Spike trace parsed: {} PCs with register writes (total {}), {} PCs with memory writes (total {}), {} PCs with exceptions",
        register_writes.len(),
        total_reg_writes,
        memory_writes.len(),
        total_mem_writes,
        total_exceptions
    );

    Ok(SpikeTrace {
        register_writes,
        memory_writes,
        exceptions,
    })
}

impl SpikeExecutor {
    pub fn execute<T: AsRef<Path>>(
        &self,
        run_folder: T,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, SpikeError> {
        info!(
            "Executing Spike run for {} user instructions",
            user_insts.len()
        );

        let run_dir = run_folder.as_ref().to_owned();
        info!("Using Spike run directory {}", run_dir.display());

        let run_result = self.compile_and_run_with_dir(&run_dir, user_insts)?;

        // Parse the Spike log (Spike writes all trace data to stderr)
        let trace = {
            let file = std::fs::File::open(&run_result.stderr_path).map_err(|e| {
                SpikeError::from(ProcessError::LogFileOpenFailed {
                    path: run_result.stderr_path.clone(),
                    source: e,
                })
            })?;
            let reader = BufReader::new(file);
            parse_spike_log(reader)?
        };

        // Load the ELF tracer
        let tracer = Tracer::new(user_insts, &run_result.build.disassembly_file).map_err(|e| {
            SpikeError::from(ElfError::DumpLoadFailed {
                path: run_result.build.disassembly_file.clone(),
                source: e,
            })
        })?;

        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        info!(
            "Spike run complete: {} register writes, {} memory writes, {} exceptions",
            trace.register_writes.len(),
            trace.memory_writes.len(),
            trace.exceptions.len()
        );

        build_execution_output(
            trace,
            user_instruction_info,
            user_pc_map,
            self.config.riscv_impl,
            self.config.isa_base,
        )
    }
}

/// Build ExecutionOutput from Spike trace
fn build_execution_output(
    trace: SpikeTrace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> Result<ExecutionOutput, SpikeError> {
    use std::collections::HashMap;

    let total_pcs: usize = user_pc_map.iter().map(|pcs| pcs.len()).sum();
    debug!(
        "Building Spike execution output: {} instructions, {} total PCs mapped",
        user_instruction_info.len(),
        total_pcs
    );

    let num_user_insts = user_instruction_info.len();
    let mut register_writes: Vec<Vec<TraitRegisterValue>> = Vec::with_capacity(num_user_insts);
    let mut memory_writes: Vec<Vec<TraitMemValue>> = Vec::with_capacity(num_user_insts);
    let mut exceptions: Vec<TraitExceptionInfo> = Vec::new();

    // Iterate through each user instruction
    for (user_idx, pcs) in user_pc_map.iter().enumerate() {
        // For each user instruction, collect all writes from all associated PCs
        // Keep track of the highest PC for each register/memory/exception

        // Register: map register_name -> (pc, value)
        let mut reg_map: HashMap<String, (u64, u64)> = HashMap::new();

        // Memory: map address -> (pc, value)
        let mut mem_map: HashMap<u64, (u64, u8)> = HashMap::new();

        // Exception: keep the one with the highest PC
        let mut exception_opt: Option<(u64, &ExceptionEvent)> = None;

        // Iterate through all PCs for this user instruction
        for &pc in pcs {
            // Collect register writes at this PC
            if let Some(writes) = trace.register_writes.get(&pc) {
                for write in writes {
                    reg_map
                        .entry(write.register.clone())
                        .and_modify(|entry| {
                            if pc > entry.0 {
                                *entry = (pc, write.value);
                            }
                        })
                        .or_insert((pc, write.value));
                }
            }

            // Collect memory writes at this PC
            if let Some(writes) = trace.memory_writes.get(&pc) {
                for write in writes {
                    let bytes = store_bytes_from_write(write);
                    for (addr, value) in bytes {
                        mem_map
                            .entry(addr)
                            .and_modify(|entry| {
                                if pc > entry.0 {
                                    *entry = (pc, value);
                                }
                            })
                            .or_insert((pc, value));
                    }
                }
            }

            // Collect exception at this PC
            if let Some(event) = trace.exceptions.get(&pc) {
                // Filter out interrupts (asynchronous) - only include synchronous exceptions
                // Spike uses exception names, not numeric codes, so we check for interrupt patterns
                let is_interrupt = event.cause.contains("interrupt");

                if !is_interrupt {
                    match exception_opt {
                        Some((existing_pc, _)) if pc > existing_pc => {
                            exception_opt = Some((pc, event));
                        }
                        None => {
                            exception_opt = Some((pc, event));
                        }
                        _ => {}
                    }
                }
            }
        }

        // Convert collected register writes to output format
        let mut user_regs: Vec<_> = reg_map
            .into_iter()
            .map(|(name, (_pc, value))| TraitRegisterValue { name, value })
            .collect();

        // Sort by register type and number
        // Safety: register names are always non-empty (format: prefix + number)
        user_regs.sort_by(|a, b| compare_register_names(&a.name, &b.name));
        register_writes.push(user_regs);

        // Convert collected memory writes to output format
        let mut user_mem: Vec<_> = mem_map
            .into_iter()
            .map(|(addr, (_pc, value))| TraitMemValue { addr, value })
            .collect();
        user_mem.sort_by_key(|m| m.addr);
        memory_writes.push(user_mem);

        // Add exception if found for this user instruction
        if let Some((_pc, event)) = exception_opt {
            let cause_string = if let Some(code) = event.mcause {
                format_cause_code(code)
            } else {
                warn!(
                    "Spike: unknown exception cause '{}' at PC 0x{:x}",
                    event.cause, event.pc
                );
                event.cause.clone()
            };

            exceptions.push(TraitExceptionInfo {
                user_instruction_index: user_idx,
                cause: cause_string,
            });
        }
    }

    debug!(
        "Built execution output: {} register write sets, {} memory write sets, {} exceptions",
        register_writes.len(),
        memory_writes.len(),
        exceptions.len()
    );

    Ok(ExecutionOutput {
        exceptions,
        register_write: register_writes,
        memory_write: memory_writes,
        riscv_impl,
        isa_base,
    })
}

fn store_bytes_from_write(write: &MemoryWrite) -> Vec<(u64, u8)> {
    if write.width == 0 {
        return Vec::new();
    }

    let width = write.width.min(8);
    let data = write.value.to_le_bytes();
    (0..width)
        .map(|offset| (write.address + offset as u64, data[offset]))
        .collect()
}

// Helper functions

fn build_user_pc_map(tracer: &Tracer, user_inst_count: usize) -> Result<Vec<Vec<u64>>, SpikeError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(LogParseError::NoPcMappingFound { index: idx })
            .map_err(SpikeError::from)?;

        if pcs.is_empty() {
            return Err(SpikeError::from(LogParseError::NoPcMappingFound {
                index: idx,
            }));
        }

        result.push(pcs.to_vec());
    }

    Ok(result)
}

fn register_sort_key(name: &str) -> Option<(char, u32)> {
    let mut chars = name.chars();
    let prefix = chars.next()?;
    let number: String = chars.collect();
    if number.is_empty() {
        return None;
    }
    let value = number.parse().ok()?;
    Some((prefix, value))
}

fn compare_register_names(left: &str, right: &str) -> Ordering {
    match (register_sort_key(left), register_sort_key(right)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.cmp(right),
    }
}

fn compiled_regex<'a>(
    lazy: &'a Lazy<Result<Regex, LogParseError>>,
) -> Result<&'a Regex, SpikeError> {
    match lazy.as_ref() {
        Ok(regex) => Ok(regex),
        Err(LogParseError::RegexCompilationFailed { pattern, source }) => {
            Err(SpikeError::from(LogParseError::RegexCompilationFailed {
                pattern: pattern.clone(),
                source: source.clone(),
            }))
        }
        Err(other) => Err(SpikeError::from(LogParseError::PatternMatchFailed {
            line: other.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_map::ExtensionMap;
    use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};

    #[test]
    fn executor_config_uses_extension_override() {
        let mut config =
            ExecutorConfig::new(PathBuf::from("/tmp"), ISABase::Rv32, RiscVImpl::Spike);
        let override_map = ExtensionMap {
            rv32: vec![RV32Extensions::I, RV32Extensions::F],
            rv64: vec![RV64Extensions::I],
        };
        config.set_extension_override(override_map.clone());
        let resolved = config.extension_map();

        assert_eq!(resolved.rv32, override_map.rv32);
        assert_eq!(resolved.rv64, override_map.rv64);
    }
}
