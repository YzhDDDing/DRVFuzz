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

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{ElfError, LogParseError, ParseError, ProcessError, RocketError},
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
use riscv_instruction::separated_instructions::RV32Extensions;

const ROCKET_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_ROCKET_RV32_BIN";
const ROCKET_RV32_NO_D_BINARY_ENV: &str = "RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN";
const ROCKET_RV64_BINARY_ENV: &str = "RISCV_WRAPPER_ROCKET_RV64_BIN";

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

/// Register write entry extracted from a Rocket log.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
}

/// Memory write entry extracted from a Rocket log.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub size: u8,
}

/// Exception entry extracted from a Rocket log.
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub cause: String,
    pub pc: u64,
}

/// Parsed Rocket log result.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RocketTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>,
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

/// Rocket compile-and-run configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub rocket_binary: Option<PathBuf>,
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
            rocket_binary: None,
            emulator_args: vec!["--cycle-count".to_string(), "+verbose".to_string()],
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

    fn rocket_path(&self) -> Result<PathBuf, RocketError> {
        if let Some(path) = &self.rocket_binary {
            return Ok(path.clone());
        }

        let extension_map = self.extension_map();
        let env_name = match self.isa_base {
            ISABase::Rv32 => {
                if extension_map.rv32.contains(&RV32Extensions::D) {
                    ROCKET_RV32_BINARY_ENV
                } else {
                    ROCKET_RV32_NO_D_BINARY_ENV
                }
            }
            ISABase::Rv64 => ROCKET_RV64_BINARY_ENV,
        };
        let binary = env::var(env_name).map_err(|_| RocketError::EnvVarNotSet {
            var: env_name.to_string(),
        })?;
        Ok(PathBuf::from(binary))
    }
}

/// Rocket executor.
#[derive(Debug)]
pub struct RocketExecutor {
    config: ExecutorConfig,
}

impl RocketExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Compile user instructions and run the Rocket simulator.
    pub fn compile_and_run_with_dir(
        &self,
        run_dir: &Path,
        user_insts: &[String],
    ) -> Result<RocketRunResult, RocketError> {
        info!(
            "Starting Rocket compile-and-run with {} user instructions in {}",
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

        let output = self.run_rocket(run_dir, &build)?;
        info!("Rocket finished with status {:?}", output.status.code());

        Ok(output)
    }

    /// Compile user instructions and run the Rocket simulator.
    pub fn compile_and_run(&self, user_insts: &[String]) -> Result<RocketRunResult, RocketError> {
        self.compile_and_run_with_dir(&self.config.work_dir, user_insts)
    }

    /// Only perform compilation and return the build artifacts.
    pub fn compile(&self, user_insts: &[String]) -> Result<ElfBuildResult, RocketError> {
        info!(
            "Compiling Rocket program with {} user instructions",
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
            "Rocket compilation finished: {}",
            build.executable_file.display()
        );
        Ok(build)
    }

    fn ensure_work_dir(&self, dir: &Path) -> Result<(), RocketError> {
        if dir.exists() {
            return Ok(());
        }
        fs::create_dir_all(dir).map_err(|e| RocketError::from(e))
    }

    fn write_program(&self, dir: &Path, program: &str) -> Result<PathBuf, RocketError> {
        let asm_path = dir.join("program.S");
        fs::write(&asm_path, program)?;
        debug!("Assembly written to {}", asm_path.display());
        Ok(asm_path)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<String, RocketError> {
        Ok(self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?)
    }

    fn run_rocket(
        &self,
        run_dir: &Path,
        build: &ElfBuildResult,
    ) -> Result<RocketRunResult, RocketError> {
        let rocket_path = self.config.rocket_path()?;
        if !is_command_available(&rocket_path) {
            return Err(RocketError::BinaryNotFound {
                path: rocket_path.display().to_string(),
            });
        }

        info!("Launching Rocket emulator: {}", rocket_path.display());

        let mut cmd = Command::new(&rocket_path);
        cmd.args(&self.config.emulator_args);
        cmd.arg(&build.executable_file);
        cmd.current_dir(run_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let command_display = format!("{:?}", cmd);
        debug!("Rocket command line: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            RocketError::from(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let stdout_reader = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture Rocket stdout")
        })?;
        let stderr_reader = child.stderr.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture Rocket stderr")
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

        let stdout_path = run_dir.join("rocket_stdout.log");
        let stderr_path = run_dir.join("rocket_stderr.log");
        fs::write(&stdout_path, &stdout_bytes).map_err(|e| {
            RocketError::from(ProcessError::FileWriteFailed {
                path: stdout_path.clone(),
                source: e,
            })
        })?;
        fs::write(&stderr_path, &stderr_bytes).map_err(|e| {
            RocketError::from(ProcessError::FileWriteFailed {
                path: stderr_path.clone(),
                source: e,
            })
        })?;

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if timed_out {
            if let Some(limit) = timeout {
                error!("Rocket execution timed out after {:?}", limit);
                return Err(RocketError::from(ProcessError::TimedOut {
                    command: command_display,
                    timeout: limit,
                }));
            } else {
                unreachable!("timed_out implies timeout was set");
            }
        }

        info!("Rocket finished with status {:?}", status.code());

        Ok(RocketRunResult {
            build: build.clone(),
            status,
            stdout,
            stderr,
            stdout_path,
            stderr_path,
        })
    }
}

/// Rocket execution result.
#[derive(Debug)]
pub struct RocketRunResult {
    pub build: ElfBuildResult,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

pub fn parse_rocket_log<R: BufRead>(reader: R) -> Result<RocketTrace, RocketError> {
    const XREG_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+x\s*(?P<reg>\d+)\s+(?P<val>0x[0-9a-fA-F]+)";
    static XREG_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(XREG_PATTERN));

    const FREG_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+f\s*(?P<reg>\d+)\s+(?P<val>0x[0-9a-fA-F]+)";
    static FREG_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(FREG_PATTERN));

    const STORE_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\(STORE\)\s+addr=(?P<addr>0x[0-9a-fA-F]+)\s+data=(?P<data>0x[0-9a-fA-F]+)\s+size=(?P<size>\d+)";
    static STORE_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(STORE_PATTERN));

    const EXC_PATTERN: &str = r"^\s*(?P<cycle>\d+)\s+(?P<pc>0x[0-9a-fA-F]+)\s+\((?P<instr>0x[0-9a-fA-F]+)\)\s+EXCEPTION\s+cause=(?P<cause>0x[0-9a-fA-F]+)\s+tval=(?P<tval>0x[0-9a-fA-F]+)";
    static EXC_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(EXC_PATTERN));

    let xreg_re = compiled_regex(&XREG_RE, XREG_PATTERN)?;
    let freg_re = compiled_regex(&FREG_RE, FREG_PATTERN)?;
    let store_re = compiled_regex(&STORE_RE, STORE_PATTERN)?;
    let exc_re = compiled_regex(&EXC_RE, EXC_PATTERN)?;

    // Use intermediate HashMaps to ensure uniqueness (last write wins)
    let mut temp_reg_writes: HashMap<u64, HashMap<String, RegisterWrite>> = HashMap::new();
    let mut temp_mem_writes: HashMap<u64, HashMap<u64, MemoryWrite>> = HashMap::new();
    let mut exceptions: HashMap<u64, ExceptionEvent> = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(cap) = xreg_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let reg_str = cap
                .name("reg")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "reg".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg: usize = reg_str.parse().map_err(|e| {
                RocketError::from(ParseError::IntParseError {
                    value: reg_str.to_string(),
                    source: e,
                })
            })?;

            let val_str = cap
                .name("val")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: val_str.to_string(),
                    source: e,
                })
            })?;

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
            continue;
        }

        if let Some(cap) = freg_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;
            let reg_str = cap
                .name("reg")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "reg".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let reg: usize = reg_str.parse().map_err(|e| {
                RocketError::from(ParseError::IntParseError {
                    value: reg_str.to_string(),
                    source: e,
                })
            })?;
            let val_str = cap
                .name("val")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "val".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(val_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
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
            continue;
        }

        if let Some(cap) = store_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let addr_str = cap
                .name("addr")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "addr".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let addr = u64::from_str_radix(addr_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: addr_str.to_string(),
                    source: e,
                })
            })?;

            let data_str = cap
                .name("data")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "data".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let value = u64::from_str_radix(data_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: data_str.to_string(),
                    source: e,
                })
            })?;

            // Parse size field (log2 encoding: 0=1byte, 1=2bytes, 2=4bytes, 3=8bytes)
            let size_str = cap
                .name("size")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "size".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();
            let size: u8 = size_str.parse().map_err(|e| {
                RocketError::from(ParseError::IntParseError {
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
            continue;
        }

        if let Some(cap) = exc_re.captures(trimmed) {
            let pc_str = cap
                .name("pc")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "pc".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str()
                .trim_start_matches("0x");
            let pc = u64::from_str_radix(pc_str, 16).map_err(|e| {
                RocketError::from(ParseError::HexParseError {
                    value: pc_str.to_string(),
                    source: e,
                })
            })?;

            let cause_str = cap
                .name("cause")
                .ok_or_else(|| {
                    RocketError::from(LogParseError::MissingCaptureGroup {
                        group: "cause".to_string(),
                        line: trimmed.to_string(),
                    })
                })?
                .as_str();

            // Still match and validate the tval field in the log, but no longer store it
            let _ = cap.name("tval").ok_or_else(|| {
                RocketError::from(LogParseError::MissingCaptureGroup {
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
        "Rocket trace parsed: {} PCs with register writes (total {}), {} PCs with memory writes (total {}), {} PCs with exceptions",
        register_writes.len(),
        total_reg_writes,
        memory_writes.len(),
        total_mem_writes,
        total_exceptions
    );

    Ok(RocketTrace {
        register_writes,
        memory_writes,
        exceptions,
    })
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
) -> Result<&'a Regex, RocketError> {
    lazy.as_ref().map_err(|err| {
        RocketError::from(LogParseError::RegexCompilationFailed {
            pattern: pattern.to_string(),
            source: err.clone(),
        })
    })
}

fn parse_trace_from_result(run_result: &RocketRunResult) -> Result<RocketTrace, RocketError> {
    let trace = parse_trace_from_path(&run_result.stderr_path)?;

    if !trace_has_content(&trace) {
        return Err(RocketError::BuildElfError(
            "Rocket log contained no trace data".to_string(),
        ));
    }

    Ok(trace)
}

fn parse_trace_from_path(path: &Path) -> Result<RocketTrace, RocketError> {
    let file = fs::File::open(path).map_err(|e| {
        RocketError::from(ProcessError::LogFileOpenFailed {
            path: path.to_path_buf(),
            source: e,
        })
    })?;
    let reader = BufReader::new(file);
    parse_rocket_log(reader)
}

fn trace_has_content(trace: &RocketTrace) -> bool {
    !(trace.register_writes.is_empty()
        && trace.memory_writes.is_empty()
        && trace.exceptions.is_empty())
}

impl RocketExecutor {
    pub fn execute<T: AsRef<Path>>(
        &self,
        run_folder: T,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, RocketError> {
        info!(
            "Executing Rocket run for {} user instructions",
            user_insts.len()
        );

        let run_dir = run_folder.as_ref().to_owned();
        info!("Using Rocket run directory {}", run_dir.display());

        let run_result = self.compile_and_run_with_dir(&run_dir, user_insts)?;

        let trace = parse_trace_from_result(&run_result)?;

        let tracer = Tracer::new(user_insts, &run_result.build.disassembly_file).map_err(|e| {
            RocketError::from(ElfError::DumpLoadFailed {
                path: run_result.build.disassembly_file.clone(),
                source: e,
            })
        })?;

        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        info!(
            "Rocket run complete: {} register writes, {} memory writes, {} exceptions",
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

/// Build ExecutionOutput from Rocket trace
fn build_execution_output(
    trace: RocketTrace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    use std::collections::HashMap;

    let total_pcs: usize = user_pc_map.iter().map(|pcs| pcs.len()).sum();
    debug!(
        "Building Rocket execution output: {} instructions, {} total PCs mapped",
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
                        "Rocket: unknown exception cause '{}' at PC 0x{:x}",
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

/// Extract bytes from Rocket STORE based on size field (log2 encoding)
/// Rocket's data field is always 64-bit, size indicates actual write width:
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

fn build_user_pc_map(
    tracer: &Tracer,
    user_inst_count: usize,
) -> Result<Vec<Vec<u64>>, RocketError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(RocketError::NoPcMapping { index: idx })?;

        if pcs.is_empty() {
            return Err(RocketError::NoPcMapping { index: idx });
        }

        result.push(pcs.to_vec());
    }

    Ok(result)
}
