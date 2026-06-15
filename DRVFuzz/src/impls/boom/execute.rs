use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
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

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{BoomError, ElfError, LogParseError, ParseError, ProcessError},
    exception_cause::{format_cause_code, parse_cause_hex},
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

// const BOOM_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_BOOM_RV32_BIN";
// const BOOM_RV32_NO_D_BINARY_ENV: &str = "RISCV_WRAPPER_BOOM_RV32_NO_D_BIN";
const BOOM_V3_BINARY_ENV: &str = "RISCV_WRAPPER_BOOM_V3_BIN";
const BOOM_V4_BINARY_ENV: &str = "RISCV_WRAPPER_BOOM_V4_BIN";

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

/// Register write entry extracted from a Boom log.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
}

/// Memory write entry extracted from a Boom log.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub size: u8,
}

/// Exception entry extracted from a Boom log.
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub cause: String,
    pub pc: u64,
}

/// Parsed Boom log result.
#[derive(Debug, Clone, Serialize, Default)]
pub struct BoomTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>,
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

/// Boom compile-and-run configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub boom_binary: Option<PathBuf>,
    pub emulator_args: Vec<String>,
    pub timeout: Option<Duration>,
    extension_override: Option<ExtensionMap>,
}

impl ExecutorConfig {
    pub fn new(work_dir: PathBuf, isa_base: ISABase, riscv_impl: RiscVImpl) -> Self {
        Self {
            work_dir,
            isa_base,
            riscv_impl,
            boom_binary: None,
            emulator_args: vec!["+verbose".to_string()],
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

    fn boom_path(&self) -> Result<PathBuf, BoomError> {
        if let Some(path) = &self.boom_binary {
            return Ok(path.clone());
        }

        let (env_name, fallback_name) = match self.riscv_impl {
            RiscVImpl::BoomV3 => (BOOM_V3_BINARY_ENV, "boom_v3_medium_rv64"),
            RiscVImpl::BoomV4 => (BOOM_V4_BINARY_ENV, "boom_v4_medium_rv64"),
            other => {
                warn!(
                    "Boom executor invoked with non-BOOM impl {}; defaulting to BOOM v3 binary lookup",
                    other
                );
                (BOOM_V3_BINARY_ENV, "boom_v3_medium_rv64")
            }
        };

        if let Ok(binary) = env::var(env_name) {
            return Ok(PathBuf::from(binary));
        }

        // Fallback to repo-relative binary (converted to an absolute path) so it still works
        // even though we run the wrapper with `current_dir(run_dir)`.
        let fallback = env::current_dir()?
            .join("riscv_impls_bins")
            .join(fallback_name);
        if fallback.exists() {
            return Ok(fallback);
        }

        Err(BoomError::EnvVarNotSet {
            var: env_name.to_string(),
        })
    }
}

/// Boom executor.
#[derive(Debug)]
pub struct BoomExecutor {
    config: ExecutorConfig,
}

impl BoomExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Compile user instructions and run the Boom simulator.
    pub fn compile_and_run_with_dir(
        &self,
        run_dir: &Path,
        user_insts: &[String],
    ) -> Result<BoomRunResult, BoomError> {
        info!(
            "Starting Boom compile-and-run with {} user instructions in {}",
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
        )?;

        info!("ELF build completed at {}", build.executable_file.display());

        let output = self.run_boom(run_dir, &build)?;
        info!("Boom finished with status {:?}", output.status.code());

        Ok(output)
    }

    /// Compile user instructions and run the Boom simulator.
    pub fn compile_and_run(&self, user_insts: &[String]) -> Result<BoomRunResult, BoomError> {
        self.compile_and_run_with_dir(&self.config.work_dir, user_insts)
    }

    /// Only perform compilation and return the build artifacts.
    pub fn compile(&self, user_insts: &[String]) -> Result<ElfBuildResult, BoomError> {
        info!(
            "Compiling Boom program with {} user instructions",
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
        )?;

        info!(
            "Boom compilation finished: {}",
            build.executable_file.display()
        );
        Ok(build)
    }

    fn ensure_work_dir(&self, dir: &Path) -> Result<(), BoomError> {
        if dir.exists() {
            return Ok(());
        }
        fs::create_dir_all(dir).map_err(|e| BoomError::from(e))
    }

    fn write_program(&self, dir: &Path, program: &str) -> Result<PathBuf, BoomError> {
        let asm_path = dir.join("program.S");
        fs::write(&asm_path, program)?;
        debug!("Assembly written to {}", asm_path.display());
        Ok(asm_path)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<String, BoomError> {
        Ok(self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?)
    }

    fn run_boom(&self, run_dir: &Path, build: &ElfBuildResult) -> Result<BoomRunResult, BoomError> {
        let boom_path = self.config.boom_path()?;
        if !is_command_available(&boom_path) {
            return Err(BoomError::BinaryNotFound {
                path: boom_path.display().to_string(),
            });
        }

        info!("Launching Boom emulator: {}", boom_path.display());

        let mut cmd = Command::new(&boom_path);
        // Use relative path to avoid Boom path issues
        let elf_filename = build
            .executable_file
            .file_name()
            .ok_or_else(|| BoomError::BuildElfError("Invalid ELF file path".to_string()))?;
        let elf_arg = format!("./{}", elf_filename.to_string_lossy());
        let has_max_cycles = self
            .config
            .emulator_args
            .iter()
            .any(|arg| arg.starts_with("+max-cycles="));
        let has_loadmem = self
            .config
            .emulator_args
            .iter()
            .any(|arg| arg.starts_with("+loadmem="));
        let max_cycles =
            env::var("RISCV_WRAPPER_BOOM_MAX_CYCLES").unwrap_or_else(|_| "2000000".to_string());
        cmd.arg("+permissive");
        cmd.args(&self.config.emulator_args);
        if !has_max_cycles {
            cmd.arg(format!("+max-cycles={max_cycles}"));
        }
        if !has_loadmem {
            cmd.arg(format!("+loadmem={elf_arg}"));
        }
        cmd.arg("+permissive-off");
        cmd.arg(&elf_arg);
        cmd.current_dir(run_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let command_display = format!("{:?}", cmd);
        debug!("Boom command line: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            BoomError::from(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let stdout_reader = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture Boom stdout"))?;
        let stderr_reader = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture Boom stderr"))?;

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

        let stdout_path = run_dir.join("boom_stdout.log");
        let stderr_path = run_dir.join("boom_stderr.log");
        fs::write(&stdout_path, &stdout_bytes).map_err(|e| {
            BoomError::from(ProcessError::FileWriteFailed {
                path: stdout_path.clone(),
                source: e,
            })
        })?;
        fs::write(&stderr_path, &stderr_bytes).map_err(|e| {
            BoomError::from(ProcessError::FileWriteFailed {
                path: stderr_path.clone(),
                source: e,
            })
        })?;

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if timed_out {
            if let Some(limit) = timeout {
                error!("Boom execution timed out after {:?}", limit);
                return Err(BoomError::from(ProcessError::TimedOut {
                    command: command_display,
                    timeout: limit,
                }));
            } else {
                unreachable!("timed_out implies timeout was set");
            }
        }

        info!("Boom finished with status {:?}", status.code());

        Ok(BoomRunResult {
            build: build.clone(),
            status,
            stdout,
            stderr,
            stdout_path,
            stderr_path,
        })
    }
}

/// Boom execution result.
#[derive(Debug)]
pub struct BoomRunResult {
    pub build: ElfBuildResult,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

// Parse Boom logs
pub fn parse_boom_log<R: BufRead>(reader: R) -> Result<BoomTrace, BoomError> {
    const XREG_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+x\s*(?P<reg>\d+)\s+(?P<val>0x[0-9a-fA-F]+)";
    static XREG_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(XREG_PATTERN));

    const FREG_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+f\s*(?P<reg>\d+)\s+(?P<val>0x[0-9a-fA-F]+)";
    static FREG_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(FREG_PATTERN));

    const STORE_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\(STORE\)\s+addr=(?P<addr>0x[0-9a-fA-F]+)\s+data=(?P<data>0x[0-9a-fA-F]+)\s+size=(?P<size>\d+)";
    static STORE_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(STORE_PATTERN));

    const EXC_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+EXCEPTION\s+cause=(?P<cause>0x[0-9a-fA-F]+)\s+tval=(?P<tval>0x[0-9a-fA-F]+)";
    static EXC_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(EXC_PATTERN));

    // Track bare PC lines (e.g., stores) to associate following MT entries.
    const PC_ONLY_PATTERN: &str =
        r"^\s*(?P<hart>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s*$";
    static PC_ONLY_RE: Lazy<Result<Regex, regex::Error>> =
        Lazy::new(|| Regex::new(PC_ONLY_PATTERN));

    let xreg_re = compiled_regex(&XREG_RE, XREG_PATTERN)?;
    let freg_re = compiled_regex(&FREG_RE, FREG_PATTERN)?;
    let store_re = compiled_regex(&STORE_RE, STORE_PATTERN)?;
    let exc_re = compiled_regex(&EXC_RE, EXC_PATTERN)?;
    let pc_only_re = compiled_regex(&PC_ONLY_RE, PC_ONLY_PATTERN)?;

    // Use intermediate HashMaps to ensure uniqueness (last write wins)
    let mut temp_reg_writes: HashMap<u64, HashMap<String, RegisterWrite>> = HashMap::new();
    let mut temp_mem_writes: HashMap<u64, HashMap<u64, MemoryWrite>> = HashMap::new();
    let mut exceptions: HashMap<u64, ExceptionEvent> = HashMap::new();
    // Track the most recently observed PC to associate with following MT entries.
    let mut current_pc: Option<u64> = None;
    // Track committed *memory-write* instructions (store/amo/sc) so that MT lines can be
    // attributed correctly even if other module printfs interleave between the instruction
    // commit line and the LSU MT line.
    let mut pending_mem_writes: VecDeque<PendingMemWrite> = VecDeque::new();
    // BOOM does not always print explicit `EXCEPTION cause=...` lines. In many cases the
    // exception is only observable by control-flow entering the trap handler, which reads
    // `mepc`/`mcause`/`mtval` CSRs. We reconstruct exceptions by watching CSR-read
    // instructions (SYSTEM opcode with CSR funct3) that write the CSR value into a GPR.
    let mut pending_trap_mepc: Option<u64> = None;
    let mut pending_trap_mcause: Option<u64> = None;
    let mut _pending_trap_mtval: Option<u64> = None;

    // Track the last decoded memory-write instruction so we can prefer it when an MT line
    // follows shortly after. This mitigates stale pending entries (e.g., from killed/trapping
    // stores) stealing later MT writebacks.
    let mut last_enqueued_mem_write: Option<(PendingMemWrite, usize)> = None;

    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        let line_no = line_idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("MT ") {
            // BOOM LSU prints (v3):
            //   MT <tsc> <uopc> <mem_cmd> <mem_size> <addr> <stdata> <wbdata>
            // and optionally (patched BOOM):
            //   MT <tsc> <uopc> <mem_cmd> <mem_size> <pc> <addr> <stdata> <wbdata>
            //
            // BOOM v4 uses a placeholder uopc (0) but the rest matches.
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            if fields.len() != 8 && fields.len() != 9 {
                continue;
            }
            let mem_cmd_str = fields[3];
            let mem_size_str = fields[4];
            let (pc_str_opt, addr_str, stdata_str, wbdata_str) = if fields.len() == 9 {
                (Some(fields[5]), fields[6], fields[7], fields[8])
            } else {
                (None, fields[5], fields[6], fields[7])
            };

            let mem_cmd = parse_hex_u8(mem_cmd_str)?;
            let mem_size = parse_hex_u8(mem_size_str)?;
            let address = parse_hex_u64(addr_str)?;
            let stdata = parse_hex_u64(stdata_str)?;
            let wbdata = parse_hex_u64(wbdata_str)?;

            if !is_mem_cmd_write(mem_cmd) {
                continue;
            }

            let pc_from_mt = pc_str_opt.and_then(|s| parse_hex_u64(s).ok());
            let preferred_pc = pc_from_mt.or_else(|| {
                last_enqueued_mem_write.and_then(|(entry, entry_line)| {
                    // Only prefer very recent decoded stores/AMOs; otherwise fall back to the
                    // pending queue order to handle delayed MT lines.
                    if line_no.saturating_sub(entry_line) <= 2
                        && matches_pending_mem_write(entry, mem_cmd, mem_size)
                    {
                        let pc = pop_pending_mem_write_exact(entry, &mut pending_mem_writes);
                        if pc.is_some() {
                            return pc;
                        }
                    }
                    None
                })
            });

            let pc = preferred_pc
                .or_else(|| pop_pending_mem_write_pc(mem_cmd, mem_size, &mut pending_mem_writes))
                .or(current_pc)
                .ok_or_else(|| BoomError::MissingPcForMemoryTrace {
                    line: trimmed.to_string(),
                })?;

            // SC failures do not actually write memory. Still consume the pending entry above.
            if mem_cmd == M_XSC && wbdata != 0 {
                continue;
            }

            let write_value = if is_amo_cmd(mem_cmd) {
                match compute_amo_write_value(mem_cmd, mem_size, stdata, wbdata) {
                    Some(v) => v,
                    None => {
                        warn!(
                            "Boom: unsupported AMO mem_cmd=0x{:x} in MT line: {}",
                            mem_cmd, trimmed
                        );
                        continue;
                    }
                }
            } else {
                stdata
            };

            temp_mem_writes
                .entry(pc)
                .or_insert_with(HashMap::new)
                .insert(
                    address,
                    MemoryWrite {
                        pc,
                        address,
                        value: write_value,
                        size: mem_size,
                    },
                );
            continue;
        }

        if let Some(cap) = xreg_re.captures(trimmed) {
            let instr_str = cap
                .name("instr")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "instr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let instr = u32::from_str_radix(instr_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: instr_str.to_string(),
                    source: e,
                })
            })?;

            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let reg_str = cap
                .name("reg")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "reg".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg: usize = reg_str.parse().map_err(|e| {
                BoomError::from(ParseError::IntParseError {
                    value: reg_str.to_string(),
                    source: e,
                })
            })?;

            let val_str = cap
                .name("val")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: val_str.to_string(),
                    source: e,
                })
            })?;

            // Reconstruct exception events from trap handler CSR reads:
            // - `mepc`  CSR=0x341 (faulting PC)
            // - `mcause` CSR=0x342 (trap cause code)
            // - `mtval` CSR=0x343 (trap value, optional)
            //
            // These reads commonly appear consecutively at the beginning of the trap handler.
            // Once both `mepc` and `mcause` have been observed, emit an ExceptionEvent keyed
            // by the faulting PC (mepc), not the trap handler PC.
            if (instr & 0x7f) == 0x73 {
                let funct3 = (instr >> 12) & 0x7;
                // CSR instructions have funct3 != 0.
                if funct3 != 0 {
                    let csr = ((instr >> 20) & 0xfff) as u16;
                    let rd = ((instr >> 7) & 0x1f) as usize;
                    if rd == reg {
                        match csr {
                            // When a new mepc is observed, clear any stale mcause/mtval
                            // so we only pair mcause reads that happen *after* this trap.
                            0x341 => {
                                pending_trap_mepc = Some(value);
                                pending_trap_mcause = None;
                                _pending_trap_mtval = None;
                            }
                            0x342 => pending_trap_mcause = Some(value),
                            0x343 => _pending_trap_mtval = Some(value),
                            _ => {}
                        }
                        if let (Some(mepc), Some(mcause)) = (pending_trap_mepc, pending_trap_mcause)
                        {
                            exceptions.insert(
                                mepc,
                                ExceptionEvent {
                                    cause: format!("{mcause:#x}"),
                                    pc: mepc,
                                },
                            );
                            // The faulting instruction will not generate a memory-write MT line.
                            // Remove any pending mem-write entry for it to avoid mis-association.
                            drop_pending_mem_writes_at_pc(&mut pending_mem_writes, mepc);
                            pending_trap_mepc = None;
                            pending_trap_mcause = None;
                            _pending_trap_mtval = None;
                        }
                    }
                }
            }

            let register_name = format!("x{reg}");
            temp_reg_writes
                .entry(pc)
                .or_insert_with(HashMap::new)
                .insert(
                    register_name.clone(),
                    RegisterWrite {
                        index: reg as u8,
                        pc,
                        register: register_name,
                        value,
                    },
                );
            current_pc = Some(pc);
            if let Some(entry) =
                enqueue_pending_mem_write_from_instr(pc, instr, &mut pending_mem_writes)
            {
                last_enqueued_mem_write = Some((entry, line_no));
            }
            continue;
        }

        if let Some(cap) = freg_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let instr_str = cap
                .name("instr")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "instr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let instr = u32::from_str_radix(instr_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: instr_str.to_string(),
                    source: e,
                })
            })?;
            let reg_str = cap
                .name("reg")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "reg".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg: usize = reg_str.parse().map_err(|e| {
                BoomError::from(ParseError::IntParseError {
                    value: reg_str.to_string(),
                    source: e,
                })
            })?;
            let val_str = cap
                .name("val")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: val_str.to_string(),
                    source: e,
                })
            })?;

            let register_name = format!("f{reg}");
            temp_reg_writes
                .entry(pc)
                .or_insert_with(HashMap::new)
                .insert(
                    register_name.clone(),
                    RegisterWrite {
                        index: reg as u8,
                        pc,
                        register: register_name,
                        value,
                    },
                );
            current_pc = Some(pc);
            if let Some(entry) =
                enqueue_pending_mem_write_from_instr(pc, instr, &mut pending_mem_writes)
            {
                last_enqueued_mem_write = Some((entry, line_no));
            }
            continue;
        }

        if let Some(cap) = store_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let addr_str = cap
                .name("addr")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "addr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let addr = u64::from_str_radix(addr_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: addr_str.to_string(),
                    source: e,
                })
            })?;

            let data_str = cap
                .name("data")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "data".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(data_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: data_str.to_string(),
                    source: e,
                })
            })?;

            // Parse size field (log2 encoding: 0=1byte, 1=2bytes, 2=4bytes, 3=8bytes)
            let size_str = cap
                .name("size")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "size".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let size: u8 = size_str.parse().map_err(|e| {
                BoomError::from(ParseError::IntParseError {
                    value: size_str.to_string(),
                    source: e,
                })
            })?;

            temp_mem_writes
                .entry(pc)
                .or_insert_with(HashMap::new)
                .insert(
                    addr,
                    MemoryWrite {
                        pc,
                        address: addr,
                        value,
                        size,
                    },
                );
            current_pc = Some(pc);
            continue;
        }

        if let Some(cap) = exc_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let cause_str = cap
                .name("cause")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "cause".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();

            // Still match and validate the tval field in the log, but no longer store it
            let _ = cap.name("tval").ok_or_else(|| {
                BoomError::from(LogParseError::MissingCaptureGroup {
                    group: "tval".to_string(),
                    line: trimmed.to_string(),
                })
            })?;

            exceptions.insert(
                pc,
                ExceptionEvent {
                    cause: cause_str.to_string(),
                    pc,
                },
            );
            // The faulting instruction will not produce a memory-write MT line.
            drop_pending_mem_writes_at_pc(&mut pending_mem_writes, pc);
            current_pc = Some(pc);
            continue;
        }

        if let Some(cap) = pc_only_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let instr_str = cap
                .name("instr")
                .ok_or_else(|| {
                    BoomError::from(LogParseError::MissingCaptureGroup {
                        group: "instr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let instr = u32::from_str_radix(instr_str, 16).map_err(|e| {
                BoomError::from(ParseError::HexParseError {
                    value: instr_str.to_string(),
                    source: e,
                })
            })?;
            current_pc = Some(pc);
            if let Some(entry) =
                enqueue_pending_mem_write_from_instr(pc, instr, &mut pending_mem_writes)
            {
                last_enqueued_mem_write = Some((entry, line_no));
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
        "Boom trace parsed: {} PCs with register writes (total {}), {} PCs with memory writes (total {}), {} PCs with exceptions",
        register_writes.len(),
        total_reg_writes,
        memory_writes.len(),
        total_mem_writes,
        total_exceptions
    );

    Ok(BoomTrace {
        register_writes,
        memory_writes,
        exceptions,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_boom_log;

    #[test]
    fn parses_exception_from_trap_handler_csr_reads() {
        // Trap handler sequence observed in `temp/boom_test/boom_stderr.log`:
        // - 0x341022f3: csrrs x5,  mepc, x0  => x5 := mepc (faulting PC)
        // - 0x34202373: csrrs x6,  mcause, x0 => x6 := mcause
        let log = concat!(
            "3 0x0000000080000b24 (0x341022f3) x 5 0x00000000800004bc\n",
            "3 0x0000000080000b28 (0x34202373) x 6 0x0000000000000002\n",
        );
        let trace = parse_boom_log(std::io::Cursor::new(log)).expect("parse should succeed");
        let event = trace
            .exceptions
            .get(&0x00000000800004bc)
            .expect("should reconstruct exception at mepc");
        assert_eq!(event.pc, 0x00000000800004bc);
        assert_eq!(event.cause, "0x2");
    }

    #[test]
    fn parses_amo_write_from_mt_line() {
        // BOOM v3 LSU prints:
        // MT <tsc> <uopc> <mem_cmd> <mem_size> <addr> <stdata(rs2)> <wbdata(old)>
        //
        // For AMO.OR.W:
        // - mem_cmd = 0x0a
        // - write value = old | rs2 (truncated to mem_size width)
        let log = concat!(
            "3 0x0000000080000000 (0x472426af) x 13 0x0000000000000000\n",
            "MT 0000000000000001 43 0a 2 0000000000000100 00000000ff00ff00 0000000000ff00ff\n",
        );
        let trace = parse_boom_log(std::io::Cursor::new(log)).expect("parse should succeed");
        let writes = trace
            .memory_writes
            .get(&0x0000000080000000)
            .expect("should have mem writes at PC");
        assert_eq!(writes.len(), 1);
        let write = &writes[0];
        assert_eq!(write.address, 0x100);
        assert_eq!(write.size, 2);
        assert_eq!(write.value, 0x00000000ffffffff);
    }

    #[test]
    fn associates_mt_with_store_pc_even_with_intervening_lines() {
        // In BOOM/Verilator output, `printf`s from different modules can interleave, so an LSU
        // `MT ...` line is not guaranteed to appear immediately after the corresponding
        // instruction commit line. Ensure we still attribute MT to the correct store PC.
        let log = concat!(
            "3 0x0000000000001000 (0x00d62023)\n", // sw x13, 0(x12) (store)
            "3 0x0000000000001004 (0x00100513) x10 0x0000000000000001\n", // li a0,1 (intervening)
            "MT 0000000000000001 02 01 2 0000000000002000 0000000011223344 0000000000000000\n",
        );
        let trace = parse_boom_log(std::io::Cursor::new(log)).expect("parse should succeed");
        assert!(trace.memory_writes.contains_key(&0x1000));
        assert!(!trace.memory_writes.contains_key(&0x1004));
    }

    #[test]
    fn prefers_recent_pending_store_over_stale_pending_entry() {
        // If a store-like instruction is enqueued but never produces an MT write line (e.g.,
        // killed/trapped), it can remain in the pending queue and steal the next MT write that
        // matches its mem_cmd/mem_size. Prefer the most recently decoded store when the MT line
        // follows immediately.
        let log = concat!(
            "3 0x0000000080002000 (0x00c62023)\n", // sw x12, 0(x12) (stale pending, no MT)
            "3 0x0000000080002004 (0x00100513) x10 0x0000000000000001\n", // intervening
            "3 0x0000000080001000 (0x00e4a827)\n", // fsw f14, 16(x9) (recent pending)
            "MT 0000000000000001 02 01 2 0000000000000100 0000000000000000 0000000000000000\n",
        );
        let trace = parse_boom_log(std::io::Cursor::new(log)).expect("parse should succeed");
        assert!(trace.memory_writes.contains_key(&0x0000000080001000));
        assert!(!trace.memory_writes.contains_key(&0x0000000080002000));
    }

    #[test]
    fn parses_mt_with_explicit_pc_field() {
        // New (patched) MT format includes a PC field, enabling exact attribution.
        let log = concat!(
            "3 0x0000000080000706 (0x00e4a827)\n",
            "MT 0000000000000001 02 01 2 0080000706 008ffffff0 0000000000000000 574834c0b6b6bff9\n",
        );
        let trace = parse_boom_log(std::io::Cursor::new(log)).expect("parse should succeed");
        let writes = trace
            .memory_writes
            .get(&0x0000000080000706)
            .expect("should attribute MT to the explicit PC");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].address, 0x000000008ffffff0);
        assert_eq!(writes[0].size, 2);
        assert_eq!(writes[0].value, 0);
    }
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
) -> Result<&'a Regex, BoomError> {
    lazy.as_ref().map_err(|err| {
        BoomError::from(LogParseError::RegexCompilationFailed {
            pattern: pattern.to_string(),
            source: err.clone(),
        })
    })
}

const M_XWR: u8 = 0x01;
const M_XA_SWAP: u8 = 0x04;
const M_XSC: u8 = 0x07;
const M_XA_ADD: u8 = 0x08;
const M_XA_XOR: u8 = 0x09;
const M_XA_OR: u8 = 0x0a;
const M_XA_AND: u8 = 0x0b;
const M_XA_MIN: u8 = 0x0c;
const M_XA_MAX: u8 = 0x0d;
const M_XA_MINU: u8 = 0x0e;
const M_XA_MAXU: u8 = 0x0f;
const M_PWR: u8 = 0x11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingMemWriteKind {
    StoreLike,
    Sc,
    Amo(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingMemWrite {
    pc: u64,
    kind: PendingMemWriteKind,
    mem_size: u8,
}

fn enqueue_pending_mem_write_from_instr(
    pc: u64,
    instr: u32,
    pending: &mut VecDeque<PendingMemWrite>,
) -> Option<PendingMemWrite> {
    if let Some(entry) = decode_pending_mem_write(pc, instr) {
        pending.push_back(entry);
        // Avoid unbounded growth if the log is truncated or MT is disabled.
        if pending.len() > 4096 {
            pending.pop_front();
        }
        return Some(entry);
    }
    None
}

fn decode_pending_mem_write(pc: u64, instr: u32) -> Option<PendingMemWrite> {
    // Check for compressed instruction encoding: bits[1:0] != 0b11.
    if (instr & 0x3) != 0x3 {
        let insn16 = instr as u16;
        let quadrant = insn16 & 0x3;
        let funct3 = (insn16 >> 13) & 0x7;
        let (is_store, mem_size) = match (quadrant, funct3) {
            // Quadrant 0 (0b00): c.fsw/c.fsd/c.sw/c.sd
            (0b00, 0b100) => (true, 2u8), // c.fsw
            (0b00, 0b101) => (true, 3u8), // c.fsd
            (0b00, 0b110) => (true, 2u8), // c.sw
            (0b00, 0b111) => (true, 3u8), // c.sd (RV64)
            // Quadrant 2 (0b10): c.fswsp/c.fsdsp/c.swsp/c.sdsp
            (0b10, 0b100) => (true, 2u8), // c.fswsp
            (0b10, 0b101) => (true, 3u8), // c.fsdsp
            (0b10, 0b110) => (true, 2u8), // c.swsp
            (0b10, 0b111) => (true, 3u8), // c.sdsp (RV64)
            _ => (false, 0u8),
        };
        if is_store {
            return Some(PendingMemWrite {
                pc,
                kind: PendingMemWriteKind::StoreLike,
                mem_size,
            });
        }
        return None;
    }

    // 32-bit instruction decoding.
    let opcode = (instr & 0x7f) as u8;
    let funct3 = ((instr >> 12) & 0x7) as u8;

    match opcode {
        0x23 => {
            // Integer store.
            Some(PendingMemWrite {
                pc,
                kind: PendingMemWriteKind::StoreLike,
                mem_size: decode_ls_mem_size(funct3)?,
            })
        }
        0x27 => {
            // FP store.
            Some(PendingMemWrite {
                pc,
                kind: PendingMemWriteKind::StoreLike,
                mem_size: decode_ls_mem_size(funct3)?,
            })
        }
        0x2f => {
            // AMO/LR/SC.
            let funct5 = ((instr >> 27) & 0x1f) as u8;
            let mem_size = decode_amo_mem_size(funct3)?;
            let kind = match funct5 {
                0b00011 => PendingMemWriteKind::Sc, // SC
                0b00001 => PendingMemWriteKind::Amo(M_XA_SWAP),
                0b00000 => PendingMemWriteKind::Amo(M_XA_ADD),
                0b00100 => PendingMemWriteKind::Amo(M_XA_XOR),
                0b01000 => PendingMemWriteKind::Amo(M_XA_OR),
                0b01100 => PendingMemWriteKind::Amo(M_XA_AND),
                0b10000 => PendingMemWriteKind::Amo(M_XA_MIN),
                0b10100 => PendingMemWriteKind::Amo(M_XA_MAX),
                0b11000 => PendingMemWriteKind::Amo(M_XA_MINU),
                0b11100 => PendingMemWriteKind::Amo(M_XA_MAXU),
                _ => return None, // LR or unsupported AMO encodings
            };
            Some(PendingMemWrite { pc, kind, mem_size })
        }
        _ => None,
    }
}

fn decode_ls_mem_size(funct3: u8) -> Option<u8> {
    match funct3 {
        0b000 => Some(0), // byte
        0b001 => Some(1), // half
        0b010 => Some(2), // word
        0b011 => Some(3), // dword
        _ => None,
    }
}

fn decode_amo_mem_size(funct3: u8) -> Option<u8> {
    match funct3 {
        0b010 => Some(2), // .w
        0b011 => Some(3), // .d
        _ => None,
    }
}

fn matches_pending_mem_write(entry: PendingMemWrite, mem_cmd: u8, mem_size: u8) -> bool {
    if entry.mem_size != mem_size {
        return false;
    }
    match entry.kind {
        PendingMemWriteKind::StoreLike => mem_cmd == M_XWR || mem_cmd == M_PWR,
        PendingMemWriteKind::Sc => mem_cmd == M_XSC,
        PendingMemWriteKind::Amo(cmd) => mem_cmd == cmd,
    }
}

fn pop_pending_mem_write_pc(
    mem_cmd: u8,
    mem_size: u8,
    pending: &mut VecDeque<PendingMemWrite>,
) -> Option<u64> {
    if pending
        .front()
        .copied()
        .is_some_and(|entry| matches_pending_mem_write(entry, mem_cmd, mem_size))
    {
        return pending.pop_front().map(|entry| entry.pc);
    }
    let idx = pending
        .iter()
        .position(|&entry| matches_pending_mem_write(entry, mem_cmd, mem_size))?;
    pending.remove(idx).map(|entry| entry.pc)
}

fn pop_pending_mem_write_exact(
    target: PendingMemWrite,
    pending: &mut VecDeque<PendingMemWrite>,
) -> Option<u64> {
    let idx = pending.iter().position(|&entry| entry == target)?;
    pending.remove(idx).map(|entry| entry.pc)
}

fn drop_pending_mem_writes_at_pc(pending: &mut VecDeque<PendingMemWrite>, pc: u64) {
    pending.retain(|entry| entry.pc != pc);
}

fn is_amo_cmd(cmd: u8) -> bool {
    matches!(
        cmd,
        M_XA_SWAP
            | M_XA_ADD
            | M_XA_XOR
            | M_XA_OR
            | M_XA_AND
            | M_XA_MIN
            | M_XA_MAX
            | M_XA_MINU
            | M_XA_MAXU
    )
}

fn is_mem_cmd_write(cmd: u8) -> bool {
    cmd == M_XWR || cmd == M_PWR || cmd == M_XSC || is_amo_cmd(cmd)
}

fn compute_amo_write_value(cmd: u8, mem_size: u8, stdata: u64, wbdata: u64) -> Option<u64> {
    let num_bytes: u32 = 1u32.checked_shl(mem_size as u32)?;
    let width_bits: u32 = num_bytes.checked_mul(8)?;
    let mask = if width_bits >= 64 {
        u64::MAX
    } else {
        (1u64 << width_bits) - 1
    };

    let old = wbdata & mask;
    let rs2 = stdata & mask;

    let result = match cmd {
        M_XA_SWAP => rs2,
        M_XA_ADD => old.wrapping_add(rs2) & mask,
        M_XA_XOR => (old ^ rs2) & mask,
        M_XA_OR => (old | rs2) & mask,
        M_XA_AND => (old & rs2) & mask,
        M_XA_MIN => {
            let old_s = sign_extend(old, width_bits);
            let rs2_s = sign_extend(rs2, width_bits);
            if old_s <= rs2_s { old } else { rs2 }
        }
        M_XA_MAX => {
            let old_s = sign_extend(old, width_bits);
            let rs2_s = sign_extend(rs2, width_bits);
            if old_s >= rs2_s { old } else { rs2 }
        }
        M_XA_MINU => {
            if old <= rs2 {
                old
            } else {
                rs2
            }
        }
        M_XA_MAXU => {
            if old >= rs2 {
                old
            } else {
                rs2
            }
        }
        _ => return None,
    };

    Some(result & mask)
}

fn sign_extend(value: u64, width_bits: u32) -> i64 {
    if width_bits == 0 || width_bits >= 64 {
        value as i64
    } else {
        let shift = 64 - width_bits;
        ((value << shift) as i64) >> shift
    }
}

fn parse_hex_u64(s: &str) -> Result<u64, ParseError> {
    let s = s.trim_start_matches("0x");
    u64::from_str_radix(s, 16).map_err(|e| ParseError::HexParseError {
        value: s.to_string(),
        source: e,
    })
}

fn parse_hex_u8(s: &str) -> Result<u8, ParseError> {
    let v = parse_hex_u64(s)?;
    u8::try_from(v).map_err(|_| ParseError::ValueParseError {
        text: s.to_string(),
    })
}

fn parse_trace_from_result(run_result: &BoomRunResult) -> Result<BoomTrace, BoomError> {
    let trace = parse_trace_from_path(&run_result.stderr_path)?;

    if !trace_has_content(&trace) {
        return Err(BoomError::BuildElfError(
            "BOOM log contained no trace data".to_string(),
        ));
    }

    Ok(trace)
}

fn parse_trace_from_path(path: &Path) -> Result<BoomTrace, BoomError> {
    let file = fs::File::open(path).map_err(|e| {
        BoomError::from(ProcessError::LogFileOpenFailed {
            path: path.to_path_buf(),
            source: e,
        })
    })?;
    let reader = BufReader::new(file);
    parse_boom_log(reader)
}

fn trace_has_content(trace: &BoomTrace) -> bool {
    !(trace.register_writes.is_empty()
        && trace.memory_writes.is_empty()
        && trace.exceptions.is_empty())
}

impl BoomExecutor {
    pub fn execute<T: AsRef<Path>>(
        &self,
        run_folder: T,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, BoomError> {
        info!(
            "Executing Boom run for {} user instructions",
            user_insts.len()
        );

        let run_dir = run_folder.as_ref().to_owned();
        info!("Using Boom run directory {}", run_dir.display());

        let run_result = self.compile_and_run_with_dir(&run_dir, user_insts)?;

        let trace = parse_trace_from_result(&run_result)?;

        let tracer = Tracer::new(user_insts, &run_result.build.disassembly_file).map_err(|e| {
            BoomError::from(ElfError::DumpLoadFailed {
                path: run_result.build.disassembly_file.clone(),
                source: e,
            })
        })?;

        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        info!(
            "Boom run complete: {} register writes, {} memory writes, {} exceptions",
            trace.register_writes.len(),
            trace.memory_writes.len(),
            trace.exceptions.len()
        );

        Ok(build_execution_output(
            trace,
            user_instruction_info,
            user_pc_map,
            self.config.riscv_impl,
            self.config.isa_base,
        ))
    }
}

/// Build ExecutionOutput from Boom trace
fn build_execution_output(
    trace: BoomTrace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    use std::collections::HashMap;

    let total_pcs: usize = user_pc_map.iter().map(|pcs| pcs.len()).sum();
    debug!(
        "Building Boom execution output: {} instructions, {} total PCs mapped",
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
                let is_interrupt = if let Some(cause_str) = event.cause.strip_prefix("0x") {
                    if let Ok(cause_val) = u64::from_str_radix(cause_str, 16) {
                        (cause_val & (1u64 << 63)) != 0
                    } else {
                        false
                    }
                } else {
                    false
                };

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
        user_regs.sort_by(|a, b| compare_register_names(&a.name, &b.name));
        register_writes.push(user_regs);

        // Convert collected memory writes to output format (already sorted by BTreeMap)
        let mut user_mem: Vec<_> = mem_map
            .into_iter()
            .map(|(addr, (_pc, value))| TraitMemValue { addr, value })
            .collect();
        user_mem.sort_by_key(|m| m.addr);
        memory_writes.push(user_mem);

        // Add exception if found for this user instruction
        if let Some((_pc, event)) = exception_opt {
            let cause_string = parse_cause_hex(&event.cause)
                .map(format_cause_code)
                .unwrap_or_else(|| {
                    warn!(
                        "Boom: unknown exception cause '{}' at PC 0x{:x}",
                        event.cause, event.pc
                    );
                    event.cause.clone()
                });

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

/// Extract bytes from Boom STORE based on size field (log2 encoding)
/// Boom's data field is always 64-bit, size indicates actual write width:
/// size=0: 1 byte (SB), size=1: 2 bytes (SH), size=2: 4 bytes (SW), size=3: 8 bytes (SD)
fn store_bytes_from_write(write: &MemoryWrite) -> Vec<(u64, u8)> {
    // Calculate actual byte count from log2-encoded size: num_bytes = 2^size
    let num_bytes = 1 << write.size;

    // Extract bytes from value (little-endian)
    let mut result = Vec::with_capacity(num_bytes);
    for i in 0..num_bytes {
        let byte = ((write.value >> (i * 8)) & 0xFF) as u8;
        result.push((write.address + i as u64, byte));
    }

    result
}

fn build_user_pc_map(tracer: &Tracer, user_inst_count: usize) -> Result<Vec<Vec<u64>>, BoomError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(BoomError::NoPcMapping { index: idx })?;

        if pcs.is_empty() {
            return Err(BoomError::NoPcMapping { index: idx });
        }

        result.push(pcs.to_vec());
    }

    Ok(result)
}
