use std::cmp::Ordering;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{CVA6Error, ElfError, LogParseError, ParseError, ProcessError},
    exception_cause::{canonical_cause_code_from_name, format_cause_code},
    execution_output::{
        ExceptionInfo as TraitExceptionInfo, ExecutionOutput, MemValue as TraitMemValue,
        RegisterValue as TraitRegisterValue,
    },
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
    tracer::Tracer,
    user_instruction::UserInstructionInfo,
};

const CVA6_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_CVA6_RV32_BIN";
const CVA6_RV64_BINARY_ENV: &str = "RISCV_WRAPPER_CVA6_RV64_BIN";
const CVA6_BINARY_ENV: &str = "RISCV_WRAPPER_CVA6_BIN";
const RVFI_TRACE_CANDIDATES: [&str; 2] =
    ["trace_rvfi_hart_00000000.dasm", "trace_rvfi_hart_00.dasm"];

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

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub source: ExceptionSource,
    pub kind: String,
    pub pc: u64,
    pub code: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ExceptionSource {
    Rvfi,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
    pub instruction: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWriteByte {
    pub lane: u8,
    pub address: u64,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub width: usize,
    pub mask: u64,
    pub instruction: Option<u32>,
    pub byte_writes: Vec<MemoryWriteByte>,
}

/// CVA6 RVFI trace organized by PC (matching Rocket's structure)
#[derive(Debug, Clone, Serialize, Default)]
pub struct RvfiTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>,
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub rvfi_path: PathBuf,
}

impl TraceConfig {
    pub fn new(rvfi_path: impl Into<PathBuf>) -> Self {
        Self {
            rvfi_path: rvfi_path.into(),
        }
    }

    pub fn from_run_directory(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            rvfi_path: RVFI_TRACE_CANDIDATES
                .iter()
                .map(|name| dir.join(name))
                .find(|path| path.exists())
                .unwrap_or_else(|| dir.join(RVFI_TRACE_CANDIDATES[0])),
        }
    }
}

pub fn analyze_traces(config: &TraceConfig) -> Result<RvfiTrace, CVA6Error> {
    info!("Analyzing RVFI trace from {:?}", config.rvfi_path);
    let rvfi = parse_rvfi_file(&config.rvfi_path)?;

    let total_reg_writes: usize = rvfi.register_writes.values().map(|v| v.len()).sum();
    let total_mem_writes: usize = rvfi.memory_writes.values().map(|v| v.len()).sum();
    debug!(
        "RVFI trace parsed: {} PCs with register writes (total {}), {} PCs with memory writes (total {}), {} PCs with exceptions",
        rvfi.register_writes.len(),
        total_reg_writes,
        rvfi.memory_writes.len(),
        total_mem_writes,
        rvfi.exceptions.len()
    );

    info!("Trace analysis complete");
    Ok(rvfi)
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub cva6_binary: Option<PathBuf>,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub timeout: Option<Duration>,
}

impl ExecutorConfig {
    pub fn new(
        work_dir: PathBuf,
        isa_base: ISABase,
        riscv_impl: RiscVImpl,
    ) -> Result<Self, CVA6Error> {
        let cva6_binary = match isa_base {
            ISABase::Rv32 => env::var(CVA6_RV32_BINARY_ENV)
                .or_else(|_| env::var(CVA6_BINARY_ENV))
                .ok()
                .map(PathBuf::from),
            ISABase::Rv64 => env::var(CVA6_RV64_BINARY_ENV)
                .or_else(|_| env::var(CVA6_BINARY_ENV))
                .ok()
                .map(PathBuf::from),
        };

        Ok(Self {
            work_dir,
            cva6_binary,
            isa_base,
            riscv_impl,
            timeout: None,
        })
    }
}

pub struct Cva6Executor {
    config: ExecutorConfig,
}

impl Cva6Executor {
    pub fn new(config: ExecutorConfig) -> Self {
        debug!("Instantiating CVA6 executor with config: {:?}", config);
        Self { config }
    }

    fn get_cva6_binary(&self) -> Result<PathBuf, CVA6Error> {
        let env_name = match self.config.isa_base {
            ISABase::Rv32 => CVA6_RV32_BINARY_ENV,
            ISABase::Rv64 => CVA6_RV64_BINARY_ENV,
        };

        if let Some(overridden) = &self.config.cva6_binary {
            return Ok(overridden.clone());
        }

        // Try ISA-specific env var first
        if let Ok(binary) = env::var(env_name) {
            return Ok(PathBuf::from(binary));
        }

        // Fall back to generic CVA6_BIN for backward compatibility (assumes RV64)
        if self.config.isa_base == ISABase::Rv64 {
            if let Ok(binary) = env::var(CVA6_BINARY_ENV) {
                return Ok(PathBuf::from(binary));
            }
        }

        Err(CVA6Error::EnvVarNotSet {
            var: env_name.to_string(),
        })
    }

    fn ensure_work_dir(&self, dir: &Path) -> Result<(), CVA6Error> {
        debug!("Ensuring work directory exists at {}", dir.display());
        fs::create_dir_all(dir)?;
        debug!("Work directory ready at {}", dir.display());
        Ok(())
    }

    fn cleanup_trace_files(&self, trace_dir: &Path) {
        for name in ["trace_rvfi_hart_00000000.dasm"] {
            let path = trace_dir.join(name);
            if path.exists() {
                match fs::remove_file(&path) {
                    Ok(_) => debug!("Removed stale trace file {}", path.display()),
                    Err(err) => warn!(
                        "Failed to remove stale trace file {}: {}",
                        path.display(),
                        err
                    ),
                }
            }
        }
    }

    fn build_program(&self, user_insts: &[String]) -> Result<String, CVA6Error> {
        debug!(
            "Building assembly program for {} user instructions",
            user_insts.len()
        );
        Ok(self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?)
    }

    fn compile_and_run_with_dir(
        &self,
        run_dir: &Path,
        user_insts: &[String],
    ) -> Result<RunArtifacts, CVA6Error> {
        info!(
            "Starting CVA6 compile-and-run flow with {} user instructions in {}",
            user_insts.len(),
            run_dir.display()
        );
        self.ensure_work_dir(run_dir)?;
        self.cleanup_trace_files(run_dir);

        let asm_path = run_dir.join("program.S");
        let program = self.build_program(user_insts)?;
        fs::write(&asm_path, &program)?;
        debug!("Assembly written to {}", asm_path.display());

        let extension_map = self.config.riscv_impl.extension_map();
        let build_result = build_elf_with_extensions(
            &asm_path,
            &extension_map,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|err| {
            error!(
                "Failed to build ELF for CVA6 execution (asm={}, isa={:?}, impl={:?}): {:#}",
                asm_path.display(),
                self.config.isa_base,
                self.config.riscv_impl,
                err
            );
            err
        })?;
        info!(
            "ELF build complete: exe={}, disasm={}",
            build_result.executable_file.display(),
            build_result.disassembly_file.display()
        );

        self.invoke_cva6(run_dir, &build_result)?;
        info!("CVA6 execution finished successfully");

        let trace_config = TraceConfig::from_run_directory(run_dir);
        debug!("Collecting traces using {:?}", trace_config);
        let summary = analyze_traces(&trace_config)?;
        info!("Trace parsing succeeded");

        Ok(RunArtifacts {
            rvfi: summary,
            disassembly_file: build_result.disassembly_file,
        })
    }

    fn invoke_cva6(&self, run_dir: &Path, build_result: &ElfBuildResult) -> Result<(), CVA6Error> {
        let cva6_binary = self.get_cva6_binary()?;
        info!(
            "Invoking CVA6 binary {} with executable {}",
            cva6_binary.display(),
            build_result.executable_file.display()
        );
        if !is_command_available(&cva6_binary) {
            error!(
                "CVA6 binary not available at {} (not found as file or in PATH)",
                cva6_binary.display()
            );
            return Err(CVA6Error::BinaryNotFound {
                path: cva6_binary.display().to_string(),
            });
        }

        let riscv_root = env::var("RISCV").map_err(|_| CVA6Error::EnvVarNotSet {
            var: "RISCV".to_string(),
        })?;
        debug!("Using RISCV toolchain at {}", riscv_root);
        let nm_path = Path::new(&riscv_root)
            .join("bin")
            .join("riscv64-unknown-elf-nm");
        if !is_command_available(&nm_path) {
            error!(
                "RISCV toolchain missing nm executable at {} (not found as file or in PATH)",
                nm_path.display()
            );
            return Err(CVA6Error::BuildElfError(format!(
                "RISC-V toolchain is missing the nm executable: {:?} (not found as file or in PATH)",
                nm_path
            )));
        }

        info!(
            "Locating tohost symbol using {} -B {}",
            nm_path.display(),
            build_result.executable_file.display()
        );
        let nm_output = Command::new(&nm_path)
            .arg("-B")
            .arg(&build_result.executable_file)
            .output()?;
        if !nm_output.status.success() {
            error!(
                "{} failed: {}",
                nm_path.display(),
                String::from_utf8_lossy(&nm_output.stderr)
            );
            return Err(CVA6Error::BuildElfError(format!(
                "`{}` failed to locate tohost symbol: {}",
                nm_path.display(),
                String::from_utf8_lossy(&nm_output.stderr)
            )));
        }

        let nm_stdout = String::from_utf8(nm_output.stdout).map_err(|e| {
            CVA6Error::BuildElfError(format!("Failed to parse nm output as UTF-8: {}", e))
        })?;
        let tohost_addr = nm_stdout
            .lines()
            .find_map(|line| {
                let mut parts = line.split_whitespace();
                let addr = parts.next()?;
                let _symbol_type = parts.next()?;
                let symbol = parts.next()?;
                if symbol == "tohost" {
                    Some(addr.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                CVA6Error::BuildElfError("failed to find tohost symbol in ELF".to_string())
            })?;
        debug!("Located tohost symbol at 0x{}", tohost_addr);

        let mut cmd = Command::new(&cva6_binary);
        cmd.arg(&build_result.executable_file)
            .arg(format!(
                "+elf_file={}",
                build_result.executable_file.display()
            ))
            .arg(format!("+tohost_addr={}", tohost_addr))
            .arg(format!("+trace_log_dir={}", run_dir.display()))
            .arg("+debug_disable");
        cmd.current_dir(run_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let command_display = format!("{:?}", cmd);
        info!("Launching CVA6: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            CVA6Error::from(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let stdout_reader = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture CVA6 stdout"))?;
        let stderr_reader = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture CVA6 stderr"))?;

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

        let stdout_bytes = stdout_handle.join().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "CVA6 stdout reader thread panicked")
        })??;
        let stderr_bytes = stderr_handle.join().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "CVA6 stderr reader thread panicked")
        })??;

        let stdout_path = run_dir.join("cva6_stdout.log");
        let stderr_path = run_dir.join("cva6_stderr.log");
        fs::write(&stdout_path, &stdout_bytes).map_err(|err| {
            error!(
                "Failed to persist CVA6 stdout to {}: {}",
                stdout_path.display(),
                err
            );
            CVA6Error::from(ProcessError::FileWriteFailed {
                path: stdout_path.clone(),
                source: err,
            })
        })?;
        fs::write(&stderr_path, &stderr_bytes).map_err(|err| {
            error!(
                "Failed to persist CVA6 stderr to {}: {}",
                stderr_path.display(),
                err
            );
            CVA6Error::from(ProcessError::FileWriteFailed {
                path: stderr_path.clone(),
                source: err,
            })
        })?;

        debug!(
            "CVA6 stdout saved to {} | stderr saved to {}",
            stdout_path.display(),
            stderr_path.display()
        );

        if timed_out {
            if let Some(limit) = timeout {
                error!(
                    "CVA6 execution timed out after {:?}. Logs saved to {} and {}",
                    limit,
                    stdout_path.display(),
                    stderr_path.display()
                );
                return Err(CVA6Error::from(ProcessError::TimedOut {
                    command: command_display,
                    timeout: limit,
                }));
            } else {
                unreachable!("timed_out implies timeout was set");
            }
        }

        if !status.success() {
            let stderr_text = String::from_utf8_lossy(&stderr_bytes).to_string();
            error!(
                "CVA6 execution failed with status {:?}. Logs saved to {} and {}",
                status.code(),
                stdout_path.display(),
                stderr_path.display()
            );
            return Err(CVA6Error::from(ProcessError::ProcessFailed {
                command: command_display,
                stderr: stderr_text,
            }));
        }

        debug!("CVA6 process exited successfully");
        Ok(())
    }
}

struct RunArtifacts {
    rvfi: RvfiTrace,
    disassembly_file: PathBuf,
}

impl Cva6Executor {
    pub fn execute<T: AsRef<Path>>(
        &self,
        run_folder: T,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, CVA6Error> {
        info!(
            "Executing CVA6 run for {} user instructions",
            user_insts.len()
        );
        let run_dir = run_folder.as_ref().to_owned();
        info!("Using CVA6 run directory {}", run_dir.display());

        let artifacts = self.compile_and_run_with_dir(&run_dir, user_insts)?;

        let RunArtifacts {
            rvfi,
            disassembly_file,
        } = artifacts;

        let tracer = Tracer::new(user_insts, &disassembly_file).map_err(|e| {
            CVA6Error::from(ElfError::DumpLoadFailed {
                path: disassembly_file.clone(),
                source: e,
            })
        })?;

        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        let exception_count = rvfi.exceptions.len();
        info!("CVA6 run complete: {} exceptions (by PC)", exception_count);

        Ok(build_execution_output(
            rvfi,
            tracer,
            user_instruction_info,
            user_pc_map,
            self.config.riscv_impl,
            self.config.isa_base,
        ))
    }
}

/// Build ExecutionOutput from CVA6 trace (matching Rocket's approach)
fn build_execution_output(
    rvfi: RvfiTrace,
    _tracer: Tracer,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    use std::collections::HashMap;

    let total_pcs: usize = user_pc_map.iter().map(|pcs| pcs.len()).sum();
    debug!(
        "Building CVA6 execution output: {} instructions, {} total PCs mapped",
        user_instruction_info.len(),
        total_pcs
    );

    let num_user_insts = user_instruction_info.len();
    let mut register_writes: Vec<Vec<TraitRegisterValue>> = Vec::with_capacity(num_user_insts);
    let mut memory_writes: Vec<Vec<TraitMemValue>> = Vec::with_capacity(num_user_insts);
    let mut exceptions: Vec<TraitExceptionInfo> = Vec::new();

    // Iterate through each user instruction (matching Rocket's approach)
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
            if let Some(writes) = rvfi.register_writes.get(&pc) {
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
            if let Some(writes) = rvfi.memory_writes.get(&pc) {
                for write in writes {
                    // CVA6 RVFI stores full 64-bit value, need to extract bytes
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
            if let Some(event) = rvfi.exceptions.get(&pc) {
                // Filter out interrupts if needed (for now, include all exceptions)
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

        // Convert collected register writes to output format
        let mut user_regs: Vec<_> = reg_map
            .into_iter()
            .map(|(name, (_pc, value))| TraitRegisterValue { name, value })
            .collect();

        // Sort by register type and number
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
            let cause_string = if let Some(code) = event
                .code
                .or_else(|| canonical_cause_code_from_name(&event.kind))
            {
                format_cause_code(code)
            } else {
                warn!(
                    "CVA6: unknown exception kind '{}' at PC 0x{:x}",
                    event.kind, event.pc
                );
                event.kind.clone()
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

    ExecutionOutput {
        exceptions,
        register_write: register_writes,
        memory_write: memory_writes,
        riscv_impl,
        isa_base,
    }
}

/// Extract bytes from CVA6 memory write using mask
/// The mask indicates which bytes in an 8-byte aligned block are accessed.
/// Each bit in the mask corresponds to a byte offset within the aligned block.
fn store_bytes_from_write(write: &MemoryWrite) -> Vec<(u64, u8)> {
    if !write.byte_writes.is_empty() {
        return write
            .byte_writes
            .iter()
            .map(|entry| (entry.address, entry.value))
            .collect();
    }

    let mask = write.mask & 0xFF;
    if mask == 0 {
        return Vec::new();
    }

    // Determine the start of the block covered by the mask. The RVFI log records the effective
    // address (base + offset), so recover the block base by subtracting the lowest asserted bit.
    let lowest_bit = mask.trailing_zeros() as u64;
    let base_addr = write.address.saturating_sub(lowest_bit);

    let mut bytes = Vec::with_capacity(write.width.max(1));
    let mut remaining = mask;
    while remaining != 0 {
        let bit_idx = remaining.trailing_zeros() as u64;
        let addr = base_addr + bit_idx;
        let shift = (bit_idx * 8) as u32;
        let value = ((write.value >> shift) & 0xFF) as u8;
        bytes.push((addr, value));
        remaining &= remaining - 1;
    }

    debug_assert!(
        bytes.len() == write.width,
        "mask/width mismatch: mask=0x{mask:x}, width={}, bytes={}",
        write.width,
        bytes.len()
    );

    bytes
}

fn parse_rvfi_file<P: AsRef<Path>>(path: P) -> Result<RvfiTrace, CVA6Error> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_rvfi(reader)
}

/// Convert memory mask to byte count
/// The mask is a bitmap where each bit represents one byte in an 8-byte aligned block.
/// 0x01 (0b00000001) -> 1 byte at offset 0
/// 0x03 (0b00000011) -> 2 bytes at offset 0-1
/// 0x30 (0b00110000) -> 2 bytes at offset 4-5
/// 0x0f (0b00001111) -> 4 bytes at offset 0-3
/// 0xff (0b11111111) -> 8 bytes at offset 0-7
/// We count the total number of 1 bits to determine the access width.
fn mask_to_width(mask: u64) -> usize {
    if mask == 0 {
        return 0;
    }
    // Count total number of 1 bits in the mask
    mask.count_ones() as usize
}

fn parse_rvfi<R: std::io::BufRead>(reader: R) -> Result<RvfiTrace, CVA6Error> {
    // CVA6 RVFI log patterns (with mandatory masks):
    // Register write (no load): "3 0x<PC> (0x<instr>) x<reg> 0x<value>" or "f<reg>"
    // Load instruction: "3 0x<PC> (0x<instr>) x<reg> 0x<value> mem 0x<addr> 0x<rmask>"
    // Store instruction: "3 0x<PC> (0x<instr>) mem 0x<addr> 0x<value> 0x<wmask> [lane]0x<byte>@0x<addr> ..."
    // Atomic/complex instruction: "3 0x<PC> (0x<instr>) x<reg> 0x<value> mem 0x<addr> 0x<mem_val> 0x<wmask> [lane]0x<byte>@0x<addr> ..."
    // Exception: "<KIND> exception @ 0x<PC> (0x<instr>)"

    // Register write with optional load address and MANDATORY rmask, OR optional store with wmask and segments
    const REG_PATTERN: &str = r"^\s*3\s+0x(?P<pc>[0-9a-fA-F]+)\s+\(0x(?P<instr>[0-9a-fA-F]+)\)\s+(?P<reg_type>[xf])\s*(?P<reg>\d+)\s+0x(?P<val>[0-9a-fA-F]+)(?:\s+mem\s+0x(?P<addr>[0-9a-fA-F]+)\s+0x(?P<mem_val>[0-9a-fA-F]+)(?:\s+0x(?P<wmask>[0-9a-fA-F]+)(?P<segments>(?:\s+\[\s*\d+\]0x[0-9a-fA-F]+@0x[0-9a-fA-F]+)*))?)?";
    static REG_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(REG_PATTERN));

    // Store instruction with MANDATORY wmask (no register write at the beginning)
    const MEM_PATTERN: &str = r"^\s*3\s+0x(?P<pc>[0-9a-fA-F]+)\s+\(0x(?P<instr>[0-9a-fA-F]+)\)\s+mem\s+0x(?P<addr>[0-9a-fA-F]+)\s+0x(?P<val>[0-9a-fA-F]+)\s+0x(?P<wmask>[0-9a-fA-F]+)(?P<segments>(?:\s+\[\s*\d+\]0x[0-9a-fA-F]+@0x[0-9a-fA-F]+)*)\s*$";
    static MEM_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(MEM_PATTERN));
    const MEM_SEGMENT_PATTERN: &str =
        r"\[\s*(?P<lane>\d+)\]0x(?P<val>[0-9a-fA-F]+)@0x(?P<byte_addr>[0-9a-fA-F]+)";
    static MEM_SEG_RE: Lazy<Result<Regex, regex::Error>> =
        Lazy::new(|| Regex::new(MEM_SEGMENT_PATTERN));

    const EXC_PATTERN: &str =
        r"^(?P<kind>\w+)\s+exception\s+@\s+0x(?P<pc>[0-9a-fA-F]+)\s+\(0x(?P<instr>[0-9a-fA-F]+)\)";
    static EXC_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(EXC_PATTERN));

    let reg_re = compiled_regex(&REG_RE, REG_PATTERN)?;
    let mem_re = compiled_regex(&MEM_RE, MEM_PATTERN)?;
    let mem_segment_re = compiled_regex(&MEM_SEG_RE, MEM_SEGMENT_PATTERN)?;
    let exc_re = compiled_regex(&EXC_RE, EXC_PATTERN)?;

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

        // Try to match exception first
        if let Some(cap) = exc_re.captures(trimmed) {
            let kind = cap
                .name("kind")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "kind".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .to_string();
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let canonical_code = canonical_cause_code_from_name(&kind);
            exceptions.insert(
                pc,
                ExceptionEvent {
                    source: ExceptionSource::Rvfi,
                    pc,
                    kind,
                    code: canonical_code,
                },
            );
            continue;
        }

        // Try to match register write
        if let Some(cap) = reg_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let instr_str = cap
                .name("instr")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "instr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let instr = u32::from_str_radix(instr_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: instr_str.to_string(),
                    source: e,
                })
            })?;
            let reg_type = cap
                .name("reg_type")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "reg_type".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg_num_str = cap
                .name("reg")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "reg".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg_num: usize = reg_num_str.parse().map_err(|e| {
                CVA6Error::from(ParseError::IntParseError {
                    value: reg_num_str.to_string(),
                    source: e,
                })
            })?;
            let val_str = cap
                .name("val")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: val_str.to_string(),
                    source: e,
                })
            })?;

            if reg_num < 32 {
                let register_name = format!("{}{}", reg_type, reg_num);
                temp_reg_writes
                    .entry(pc)
                    .or_insert_with(HashMap::new)
                    .insert(
                        register_name.clone(),
                        RegisterWrite {
                            index: reg_num as u8,
                            pc,
                            register: register_name,
                            value,
                            instruction: Some(instr),
                        },
                    );
            }

            // Check if this line also contains a memory write (atomic/complex instruction)
            // Pattern: x<reg> 0x<val> mem 0x<addr> 0x<mem_val> 0x<wmask> [segments]
            if let Some(wmask_str) = cap.name("wmask") {
                // This is an atomic or complex instruction with both register and memory write
                let addr_str = cap
                    .name("addr")
                    .ok_or_else(|| {
                        CVA6Error::from(LogParseError::MissingCaptureGroup {
                            group: "addr".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let addr = u64::from_str_radix(addr_str, 16).map_err(|e| {
                    CVA6Error::from(ParseError::HexParseError {
                        value: addr_str.to_string(),
                        source: e,
                    })
                })?;

                let mem_val_str = cap
                    .name("mem_val")
                    .ok_or_else(|| {
                        CVA6Error::from(LogParseError::MissingCaptureGroup {
                            group: "mem_val".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let mem_value = u64::from_str_radix(mem_val_str, 16).map_err(|e| {
                    CVA6Error::from(ParseError::HexParseError {
                        value: mem_val_str.to_string(),
                        source: e,
                    })
                })?;

                let wmask = u64::from_str_radix(wmask_str.as_str(), 16).map_err(|e| {
                    CVA6Error::from(ParseError::HexParseError {
                        value: wmask_str.as_str().to_string(),
                        source: e,
                    })
                })?;

                // Parse optional lane/value annotations
                let segments_str = cap.name("segments").map(|m| m.as_str()).unwrap_or("");
                let mut byte_writes: Vec<MemoryWriteByte> = Vec::new();
                let mut remaining = segments_str.trim();
                while !remaining.is_empty() {
                    let captures = mem_segment_re.captures(remaining).ok_or_else(|| {
                        CVA6Error::from(LogParseError::InvalidLineFormat {
                            line: trimmed.to_string(),
                        })
                    })?;
                    let matched = captures.get(0).ok_or_else(|| {
                        CVA6Error::from(LogParseError::InvalidLineFormat {
                            line: trimmed.to_string(),
                        })
                    })?;
                    if matched.start() != 0 {
                        let prefix = &remaining[..matched.start()];
                        if !prefix.trim().is_empty() {
                            return Err(CVA6Error::from(LogParseError::InvalidLineFormat {
                                line: trimmed.to_string(),
                            }));
                        }
                    }

                    let lane_str = captures
                        .name("lane")
                        .ok_or_else(|| {
                            CVA6Error::from(LogParseError::MissingCaptureGroup {
                                group: "lane".to_string(),
                                line: trimmed.to_string(),
                            })
                        })?
                        .as_str();
                    let lane = lane_str.parse::<u8>().map_err(|e| {
                        CVA6Error::from(ParseError::IntParseError {
                            value: lane_str.to_string(),
                            source: e,
                        })
                    })?;

                    let byte_str = captures
                        .name("val")
                        .ok_or_else(|| {
                            CVA6Error::from(LogParseError::MissingCaptureGroup {
                                group: "val".to_string(),
                                line: trimmed.to_string(),
                            })
                        })?
                        .as_str();
                    let byte_value = u8::from_str_radix(byte_str, 16).map_err(|e| {
                        CVA6Error::from(ParseError::HexParseError {
                            value: format!("0x{}", byte_str),
                            source: e,
                        })
                    })?;

                    let byte_addr_str = captures
                        .name("byte_addr")
                        .ok_or_else(|| {
                            CVA6Error::from(LogParseError::MissingCaptureGroup {
                                group: "byte_addr".to_string(),
                                line: trimmed.to_string(),
                            })
                        })?
                        .as_str();
                    let byte_addr = u64::from_str_radix(byte_addr_str, 16).map_err(|e| {
                        CVA6Error::from(ParseError::HexParseError {
                            value: byte_addr_str.to_string(),
                            source: e,
                        })
                    })?;

                    byte_writes.push(MemoryWriteByte {
                        lane,
                        address: byte_addr,
                        value: byte_value,
                    });

                    remaining = remaining[matched.end()..].trim_start();
                }

                byte_writes.sort_by_key(|entry| entry.lane);

                let mask_width = mask_to_width(wmask);
                let width = if !byte_writes.is_empty() {
                    debug_assert_eq!(
                        byte_writes.len(),
                        mask_width,
                        "mask and byte write count differ for PC 0x{pc:x}"
                    );
                    byte_writes.len()
                } else {
                    mask_width
                };

                temp_mem_writes
                    .entry(pc)
                    .or_insert_with(HashMap::new)
                    .insert(
                        addr,
                        MemoryWrite {
                            pc,
                            address: addr,
                            value: mem_value,
                            width,
                            mask: wmask,
                            instruction: Some(instr),
                            byte_writes,
                        },
                    );
            }

            // If there's a "mem" field but no wmask, it's a load instruction
            // We don't record loads as memory writes
            continue;
        }

        // Try to match memory write (store)
        if let Some(cap) = mem_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let instr_str = cap
                .name("instr")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "instr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let instr = u32::from_str_radix(instr_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: instr_str.to_string(),
                    source: e,
                })
            })?;
            let addr_str = cap
                .name("addr")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "addr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let addr = u64::from_str_radix(addr_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: addr_str.to_string(),
                    source: e,
                })
            })?;
            let value_str = cap
                .name("val")
                .ok_or_else(|| {
                    CVA6Error::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let value = u64::from_str_radix(value_str, 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: value_str.to_string(),
                    source: e,
                })
            })?;

            // Calculate width from MANDATORY wmask field
            let wmask_match = cap.name("wmask").ok_or_else(|| {
                CVA6Error::from(LogParseError::MissingCaptureGroup {
                    group: "wmask".to_string(),
                    line: trimmed.to_string(),
                })
            })?;
            let wmask = u64::from_str_radix(wmask_match.as_str(), 16).map_err(|e| {
                CVA6Error::from(ParseError::HexParseError {
                    value: wmask_match.as_str().to_string(),
                    source: e,
                })
            })?;

            // Parse optional lane/value annotations (e.g. "[0]0x12@0x1000")
            let segments_str = cap.name("segments").map(|m| m.as_str()).unwrap_or("");
            let mut byte_writes: Vec<MemoryWriteByte> = Vec::new();
            let mut remaining = segments_str.trim();
            while !remaining.is_empty() {
                let captures = mem_segment_re.captures(remaining).ok_or_else(|| {
                    CVA6Error::from(LogParseError::InvalidLineFormat {
                        line: trimmed.to_string(),
                    })
                })?;
                let matched = captures.get(0).ok_or_else(|| {
                    CVA6Error::from(LogParseError::InvalidLineFormat {
                        line: trimmed.to_string(),
                    })
                })?;
                if matched.start() != 0 {
                    let prefix = &remaining[..matched.start()];
                    if !prefix.trim().is_empty() {
                        return Err(CVA6Error::from(LogParseError::InvalidLineFormat {
                            line: trimmed.to_string(),
                        }));
                    }
                }

                let lane_str = captures
                    .name("lane")
                    .ok_or_else(|| {
                        CVA6Error::from(LogParseError::MissingCaptureGroup {
                            group: "lane".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let lane = lane_str.parse::<u8>().map_err(|e| {
                    CVA6Error::from(ParseError::IntParseError {
                        value: lane_str.to_string(),
                        source: e,
                    })
                })?;

                let byte_str = captures
                    .name("val")
                    .ok_or_else(|| {
                        CVA6Error::from(LogParseError::MissingCaptureGroup {
                            group: "val".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let byte_value = u8::from_str_radix(byte_str, 16).map_err(|e| {
                    CVA6Error::from(ParseError::HexParseError {
                        value: format!("0x{}", byte_str),
                        source: e,
                    })
                })?;

                let byte_addr_str = captures
                    .name("byte_addr")
                    .ok_or_else(|| {
                        CVA6Error::from(LogParseError::MissingCaptureGroup {
                            group: "byte_addr".to_string(),
                            line: trimmed.to_string(),
                        })
                    })?
                    .as_str();
                let byte_addr = u64::from_str_radix(byte_addr_str, 16).map_err(|e| {
                    CVA6Error::from(ParseError::HexParseError {
                        value: byte_addr_str.to_string(),
                        source: e,
                    })
                })?;

                byte_writes.push(MemoryWriteByte {
                    lane,
                    address: byte_addr,
                    value: byte_value,
                });

                remaining = remaining[matched.end()..].trim_start();
            }

            byte_writes.sort_by_key(|entry| entry.lane);

            let mask_width = mask_to_width(wmask);
            let width = if !byte_writes.is_empty() {
                debug_assert_eq!(
                    byte_writes.len(),
                    mask_width,
                    "mask and byte write count differ for PC 0x{pc:x}"
                );
                byte_writes.len()
            } else {
                mask_width
            };

            temp_mem_writes
                .entry(pc)
                .or_insert_with(HashMap::new)
                .insert(
                    addr,
                    MemoryWrite {
                        pc,
                        address: addr,
                        value,
                        width,
                        mask: wmask,
                        instruction: Some(instr),
                        byte_writes,
                    },
                );
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
        "CVA6 RVFI trace parsed: {} PCs with register writes (total {}), {} PCs with memory writes (total {}), {} PCs with exceptions",
        register_writes.len(),
        total_reg_writes,
        memory_writes.len(),
        total_mem_writes,
        total_exceptions
    );

    Ok(RvfiTrace {
        register_writes: register_writes,
        memory_writes: memory_writes,
        exceptions,
    })
}

fn build_user_pc_map(tracer: &Tracer, user_inst_count: usize) -> Result<Vec<Vec<u64>>, CVA6Error> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(LogParseError::NoPcMappingFound { index: idx })
            .map_err(CVA6Error::from)?;

        if pcs.is_empty() {
            return Err(CVA6Error::from(LogParseError::NoPcMappingFound {
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
    lazy: &'a Lazy<Result<Regex, regex::Error>>,
    pattern: &'static str,
) -> Result<&'a Regex, CVA6Error> {
    lazy.as_ref().map_err(|err| {
        CVA6Error::from(LogParseError::RegexCompilationFailed {
            pattern: pattern.to_string(),
            source: err.clone(),
        })
    })
}
