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
use regex::{Captures, Regex};
use serde::Serialize;

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{ElfError, LogParseError, ParseError, PicoRV32Error, ProcessError},
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

const PICORV32_BINARY_ENV: &str = "RISCV_WRAPPER_PICORV32_BIN";

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

/// PicoRV32 log register write record
#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
}

/// PicoRV32 log memory write record
#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub size: u8,
}

/// PicoRV32 log exception record
#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub cause: String,
    pub pc: u64,
    pub mcause: Option<u64>,
}

/// PicoRV32 log parsing result
/// Uses HashMap structure similar to Spike for automatic deduplication
#[derive(Debug, Clone, Serialize)]
pub struct PicoRV32Trace {
    // HashMap<PC, HashMap<register_name, RegisterWrite>> - deduplicates by PC + register
    pub register_writes: HashMap<u64, HashMap<String, RegisterWrite>>,
    // HashMap<PC, HashMap<address, MemoryWrite>> - deduplicates by PC + address
    pub memory_writes: HashMap<u64, HashMap<u64, MemoryWrite>>,
    // HashMap<PC, ExceptionEvent> - deduplicates by PC
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

impl Default for PicoRV32Trace {
    fn default() -> Self {
        Self {
            register_writes: HashMap::new(),
            memory_writes: HashMap::new(),
            exceptions: HashMap::new(),
        }
    }
}

/// PicoRV32 compile and execute configuration
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub picorv32_binary: Option<PathBuf>,
    pub emulator_args: Vec<String>,
    pub timeout: Option<Duration>,
}

impl ExecutorConfig {
    pub fn new(work_dir: PathBuf, isa_base: ISABase, riscv_impl: RiscVImpl) -> Self {
        Self {
            work_dir,
            isa_base,
            riscv_impl,
            picorv32_binary: None,
            emulator_args: vec!["+verbose".to_string()],
            timeout: None,
        }
    }

    fn extension_map(&self) -> ExtensionMap {
        self.riscv_impl.extension_map()
    }

    fn picorv32_path(&self) -> Result<PathBuf, PicoRV32Error> {
        if let Some(path) = &self.picorv32_binary {
            return Ok(path.clone());
        }

        let binary = env::var(PICORV32_BINARY_ENV).map_err(|_| PicoRV32Error::EnvVarNotSet {
            var: PICORV32_BINARY_ENV.to_string(),
        })?;
        Ok(PathBuf::from(binary))
    }
}

/// PicoRV32 executor
#[derive(Debug)]
pub struct PicoRV32Executor {
    config: ExecutorConfig,
}

impl PicoRV32Executor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Compile user instructions and execute PicoRV32 simulator
    pub fn compile_and_run_with_dir(
        &self,
        run_dir: &Path,
        user_insts: &[String],
    ) -> Result<PicoRV32RunResult, PicoRV32Error> {
        info!(
            "Starting PicoRV32 compile-and-run with {} user instructions in {}",
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

        let output = self.run_picorv32(run_dir, &build)?;
        info!("PicoRV32 finished with status {:?}", output.status.code());

        Ok(output)
    }

    /// Compile user instructions and execute PicoRV32 simulator
    pub fn compile_and_run(
        &self,
        user_insts: &[String],
    ) -> Result<PicoRV32RunResult, PicoRV32Error> {
        self.compile_and_run_with_dir(&self.config.work_dir, user_insts)
    }

    /// Only execute compilation, return build artifact
    pub fn compile(&self, user_insts: &[String]) -> Result<ElfBuildResult, PicoRV32Error> {
        info!(
            "Compiling PicoRV32 program with {} user instructions",
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
            "PicoRV32 compilation finished: {}",
            build.executable_file.display()
        );
        Ok(build)
    }

    fn ensure_work_dir(&self, dir: &Path) -> Result<(), PicoRV32Error> {
        if dir.exists() {
            return Ok(());
        }
        fs::create_dir_all(dir).map_err(|e| PicoRV32Error::from(e))
    }

    fn write_program(&self, dir: &Path, program: &str) -> Result<PathBuf, PicoRV32Error> {
        let asm_path = dir.join("program.S");
        fs::write(&asm_path, program)?;
        debug!("Assembly written to {}", asm_path.display());
        Ok(asm_path)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<String, PicoRV32Error> {
        Ok(self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?)
    }

    fn run_picorv32(
        &self,
        run_dir: &Path,
        build: &ElfBuildResult,
    ) -> Result<PicoRV32RunResult, PicoRV32Error> {
        let picorv32_path = self.config.picorv32_path()?;
        if !is_command_available(&picorv32_path) {
            return Err(PicoRV32Error::BinaryNotFound {
                path: picorv32_path.display().to_string(),
            });
        }

        info!("Launching PicoRV32 emulator: {}", picorv32_path.display());

        let mut cmd = Command::new(&picorv32_path);
        cmd.args(&self.config.emulator_args);
        cmd.arg(&build.executable_file);
        cmd.current_dir(run_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let command_display = format!("{:?}", cmd);
        debug!("PicoRV32 command line: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            PicoRV32Error::from(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let stdout_reader = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture PicoRV32 stdout")
        })?;
        let stderr_reader = child.stderr.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "failed to capture PicoRV32 stderr")
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

        let stdout_path = run_dir.join("picorv32_stdout.log");
        let stderr_path = run_dir.join("picorv32_stderr.log");
        fs::write(&stdout_path, &stdout_bytes).map_err(PicoRV32Error::from)?;
        fs::write(&stderr_path, &stderr_bytes).map_err(PicoRV32Error::from)?;

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if timed_out {
            if let Some(limit) = timeout {
                error!("PicoRV32 execution timed out after {:?}", limit);
                return Err(PicoRV32Error::from(ProcessError::TimedOut {
                    command: command_display,
                    timeout: limit,
                }));
            } else {
                unreachable!("timed_out implies timeout was set");
            }
        }

        info!("PicoRV32 finished with status {:?}", status.code());

        Ok(PicoRV32RunResult {
            build: build.clone(),
            status,
            stdout,
            stderr,
            stdout_path,
            stderr_path,
        })
    }
}

fn decode_picorv32_mcause(raw: &str, instr: Option<u32>) -> Option<u64> {
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "MISALIGNED_WORD" | "MISALIGNED_HALFWORD" | "MISALIGNED_BYTE" => {
            classify_data_misaligned(instr)
                .or_else(|| canonical_cause_code_from_name("LOAD_ADDR_MISALIGNED"))
        }
        "MISALIGNED_INSTRUCTION" => canonical_cause_code_from_name("INSTR_ADDR_MISALIGNED"),
        "EBREAK/UNSUPPORTED" => {
            if let Some(insn) = instr {
                if is_ebreak(insn) {
                    canonical_cause_code_from_name("BREAKPOINT")
                } else {
                    canonical_cause_code_from_name("ILLEGAL_INSTR")
                }
            } else {
                canonical_cause_code_from_name("ILLEGAL_INSTR")
            }
        }
        _ => canonical_cause_code_from_name(&upper),
    }
}

fn classify_data_misaligned(instr: Option<u32>) -> Option<u64> {
    let insn = instr?;
    if (insn & 0b11) != 0b11 {
        let quadrant = (insn & 0b11) as u8;
        let funct3 = ((insn >> 13) & 0x7) as u8;
        return match (quadrant, funct3) {
            (0b00, 0b010) | (0b00, 0b011) | (0b01, 0b010) | (0b01, 0b011) => {
                canonical_cause_code_from_name("LOAD_ADDR_MISALIGNED")
            }
            (0b00, 0b110) | (0b00, 0b111) | (0b10, 0b110) | (0b10, 0b111) => {
                canonical_cause_code_from_name("STORE_ADDR_MISALIGNED")
            }
            _ => None,
        };
    }

    let opcode = (insn & 0x7f) as u8;
    match opcode {
        0b0000011 | 0b0000111 | 0b0001111 => canonical_cause_code_from_name("LOAD_ADDR_MISALIGNED"),
        0b0100011 | 0b0100111 | 0b0101111 => {
            canonical_cause_code_from_name("STORE_ADDR_MISALIGNED")
        }
        _ => None,
    }
}

fn is_ebreak(instr: u32) -> bool {
    instr == 0x0010_0073 || instr == 0x9002
}

/// PicoRV32 execution result
#[derive(Debug)]
pub struct PicoRV32RunResult {
    pub build: ElfBuildResult,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

/// Parse PicoRV32 log from buffered reader
/// Uses HashMap-based intermediate storage similar to Spike for automatic deduplication
fn parse_picorv32_log<R: BufRead>(reader: R) -> Result<PicoRV32Trace, PicoRV32Error> {
    let reg_write_re = compiled_regex(&REG_WRITE_RE, REG_WRITE_PATTERN)?;
    let mem_write_re = compiled_regex(&MEM_WRITE_RE, MEM_WRITE_PATTERN)?;
    let exc_addr_re = compiled_regex(&EXC_ADDR_RE, EXC_ADDR_PATTERN)?;
    let exc_re = compiled_regex(&EXC_RE, EXC_PATTERN)?;
    let trap_re = compiled_regex(&TRAP_RE, TRAP_PATTERN)?;
    use HashMap;

    // REG_WRITE: x8  <= 0x12345678  (PC=0x70 INSN=0x67840413)
    const REG_WRITE_PATTERN: &str = r"^REG_WRITE:\s+x(?P<reg>\d+)\s+<=\s+0x(?P<val>[0-9a-fA-F]+)\s+\(PC=0x(?P<pc>[0-9a-fA-F]+)\s+INSN=0x(?P<instr>[0-9a-fA-F]+)\s*\)";
    static REG_WRITE_RE: Lazy<Result<Regex, regex::Error>> =
        Lazy::new(|| Regex::new(REG_WRITE_PATTERN));

    // MEM_WRITE: ADDR=0x20000000 DATA=0x075bcd15 SIZE=4 (PC=0x000000ae INSN=0x00532023)
    const MEM_WRITE_PATTERN: &str = r"^MEM_WRITE:\s+ADDR=0x(?P<addr>[0-9a-fA-F]+)\s+DATA=0x(?P<data>[0-9a-fA-F]+)\s+SIZE=(?P<size>\d+)\s+\(PC=0x(?P<pc>[0-9a-fA-F]+)\s+INSN=0x(?P<instr>[0-9a-fA-F]+)\s*\)";
    static MEM_WRITE_RE: Lazy<Result<Regex, regex::Error>> =
        Lazy::new(|| Regex::new(MEM_WRITE_PATTERN));

    // EXCEPTION: MISALIGNED_WORD ADDR=0x00000001 (PC=0x00000082 INSN=0x00092983)
    const EXC_ADDR_PATTERN: &str = r"^EXCEPTION:\s+(?P<cause>\w+)\s+ADDR=0x(?P<addr>[0-9a-fA-F]+)\s+\(PC=0x(?P<pc>[0-9a-fA-F]+)(?:\s+INSN=0x(?P<instr>[0-9a-fA-F]+))?\)";
    static EXC_ADDR_RE: Lazy<Result<Regex, regex::Error>> =
        Lazy::new(|| Regex::new(EXC_ADDR_PATTERN));

    // EXCEPTION: MISALIGNED_INSTRUCTION (PC=0x00000001)
    // EXCEPTION: EBREAK/UNSUPPORTED (PC=0x00000070 INSN=0x00009002)
    const EXC_PATTERN: &str = r"^EXCEPTION:\s+(?P<cause>[\w/]+)\s+\(PC=0x(?P<pc>[0-9a-fA-F]+)(?:\s+INSN=0x(?P<instr>[0-9a-fA-F]+))?\)";
    static EXC_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(EXC_PATTERN));

    // TRAP: Entering trap state (PC=0x000000b8 INSN=0x00009002)
    const TRAP_PATTERN: &str = r"^TRAP:\s+Entering trap state\s+\(PC=0x(?P<pc>[0-9a-fA-F]+)\s+INSN=0x(?P<instr>[0-9a-fA-F]+)\)";
    static TRAP_RE: Lazy<Result<Regex, regex::Error>> = Lazy::new(|| Regex::new(TRAP_PATTERN));

    // Use HashMap structure similar to Spike - automatic deduplication by PC + register/address
    let mut register_writes: HashMap<u64, HashMap<String, RegisterWrite>> = HashMap::new();
    let mut memory_writes: HashMap<u64, HashMap<u64, MemoryWrite>> = HashMap::new();
    let mut exceptions: HashMap<u64, ExceptionEvent> = HashMap::new();

    for line in reader.lines() {
        let line = line?;

        for segment in line.split('\r') {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse REG_WRITE
            if let Some(cap) = reg_write_re.captures(trimmed) {
                let pc = parse_hex_u64(capture_group(&cap, "pc", trimmed)?)?;
                let reg = parse_usize(capture_group(&cap, "reg", trimmed)?)?;
                let value = parse_hex_u64(capture_group(&cap, "val", trimmed)?)?;
                let register_name = format!("x{reg}");

                // Store in HashMap - last write wins for same PC + register
                register_writes
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

            // Parse MEM_WRITE
            if let Some(cap) = mem_write_re.captures(trimmed) {
                let pc = parse_hex_u64(capture_group(&cap, "pc", trimmed)?)?;
                let addr = parse_hex_u64(capture_group(&cap, "addr", trimmed)?)?;
                let data = parse_hex_u64(capture_group(&cap, "data", trimmed)?)?;
                let size = parse_u8(capture_group(&cap, "size", trimmed)?)?;

                // Store in HashMap - last write wins for same PC + address
                memory_writes.entry(pc).or_insert_with(HashMap::new).insert(
                    addr,
                    MemoryWrite {
                        pc,
                        address: addr,
                        value: data,
                        size,
                    },
                );
                continue;
            }

            // Parse EXCEPTION with ADDR
            if let Some(cap) = exc_addr_re.captures(trimmed) {
                let pc = parse_hex_u64(capture_group(&cap, "pc", trimmed)?)?;
                let cause = capture_group(&cap, "cause", trimmed)?.to_string();
                let _ = parse_hex_u64(capture_group(&cap, "addr", trimmed)?)?;
                let instr = cap
                    .name("instr")
                    .and_then(|m| u32::from_str_radix(m.as_str(), 16).ok());
                let mcause = decode_picorv32_mcause(&cause, instr);

                exceptions.insert(pc, ExceptionEvent { cause, pc, mcause });
                continue;
            }

            // Parse EXCEPTION without ADDR
            if let Some(cap) = exc_re.captures(trimmed) {
                let pc = parse_hex_u64(capture_group(&cap, "pc", trimmed)?)?;
                let cause = capture_group(&cap, "cause", trimmed)?.to_string();
                let instr = cap
                    .name("instr")
                    .and_then(|m| u32::from_str_radix(m.as_str(), 16).ok());
                let mcause = decode_picorv32_mcause(&cause, instr);

                exceptions.insert(pc, ExceptionEvent { cause, pc, mcause });
                continue;
            }

            // Parse TRAP
            if let Some(cap) = trap_re.captures(trimmed) {
                let pc = parse_hex_u64(capture_group(&cap, "pc", trimmed)?)?;

                exceptions.insert(
                    pc,
                    ExceptionEvent {
                        cause: "TRAP".to_string(),
                        pc,
                        mcause: None,
                    },
                );
                continue;
            }
        }
    }

    debug!(
        "PicoRV32 trace parsed: {} PCs with register writes, {} PCs with memory writes, {} exceptions",
        register_writes.len(),
        memory_writes.len(),
        exceptions.len()
    );

    Ok(PicoRV32Trace {
        register_writes,
        memory_writes,
        exceptions,
    })
}

fn compiled_regex<'a>(
    lazy: &'a Lazy<Result<Regex, regex::Error>>,
    pattern: &'static str,
) -> Result<&'a Regex, PicoRV32Error> {
    lazy.as_ref().map_err(|err| {
        PicoRV32Error::from(LogParseError::RegexCompilationFailed {
            pattern: pattern.to_string(),
            source: err.clone(),
        })
    })
}

fn capture_group<'a>(
    caps: &'a Captures<'a>,
    group: &str,
    line: &str,
) -> Result<&'a str, PicoRV32Error> {
    caps.name(group).map(|m| m.as_str()).ok_or_else(|| {
        PicoRV32Error::from(LogParseError::MissingCaptureGroup {
            group: group.to_string(),
            line: line.to_string(),
        })
    })
}

fn parse_hex_u64(value: &str) -> Result<u64, PicoRV32Error> {
    u64::from_str_radix(value, 16).map_err(|source| {
        PicoRV32Error::from(ParseError::HexParseError {
            value: value.to_string(),
            source,
        })
    })
}

fn parse_usize(value: &str) -> Result<usize, PicoRV32Error> {
    value.parse().map_err(|source| {
        PicoRV32Error::from(ParseError::IntParseError {
            value: value.to_string(),
            source,
        })
    })
}

fn parse_u8(value: &str) -> Result<u8, PicoRV32Error> {
    value.parse().map_err(|source| {
        PicoRV32Error::from(ParseError::IntParseError {
            value: value.to_string(),
            source,
        })
    })
}

fn parse_trace_with_fallback(
    run_result: &PicoRV32RunResult,
) -> Result<PicoRV32Trace, PicoRV32Error> {
    match parse_trace_from_path(&run_result.stdout_path) {
        Ok(trace) if trace_has_content(&trace) => return Ok(trace),
        Ok(_) => {
            debug!(
                "PicoRV32 stdout log {} contained no trace information, trying stderr",
                run_result.stdout_path.display()
            );
        }
        Err(err) => {
            debug!(
                "Unable to parse PicoRV32 stdout log {}: {:#}",
                run_result.stdout_path.display(),
                err
            );
        }
    }

    match parse_trace_from_path(&run_result.stderr_path) {
        Ok(trace) if trace_has_content(&trace) => Ok(trace),
        Ok(_) => Err(PicoRV32Error::BuildElfError(
            "PicoRV32 logs contained no trace data (checked stdout and stderr)".to_string(),
        )),
        Err(err) => Err(err),
    }
}

fn parse_trace_from_path(path: &Path) -> Result<PicoRV32Trace, PicoRV32Error> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    parse_picorv32_log(reader)
}

fn trace_has_content(trace: &PicoRV32Trace) -> bool {
    !trace.register_writes.is_empty()
        || !trace.memory_writes.is_empty()
        || !trace.exceptions.is_empty()
}

impl PicoRV32Executor {
    pub fn execute<T: AsRef<Path>>(
        &self,
        run_folder: T,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, PicoRV32Error> {
        info!(
            "Executing PicoRV32 run for {} user instructions",
            user_insts.len()
        );

        let run_dir = run_folder.as_ref().to_owned();
        info!("Using PicoRV32 run directory {}", run_dir.display());

        let run_result = self.compile_and_run_with_dir(&run_dir, user_insts)?;

        let trace = parse_trace_with_fallback(&run_result)?;

        let tracer = Tracer::new(user_insts, &run_result.build.disassembly_file).map_err(|e| {
            PicoRV32Error::from(ElfError::DumpLoadFailed {
                path: run_result.build.disassembly_file.clone(),
                source: e,
            })
        })?;

        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        info!(
            "PicoRV32 run complete: {} exceptions, {} PCs with writes",
            trace.exceptions.len(),
            trace.register_writes.len() + trace.memory_writes.len()
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

/// Build ExecutionOutput from PicoRV32 trace
/// Similar to Spike's approach: iterate through user instructions and collect writes by PC
fn build_execution_output(
    trace: PicoRV32Trace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    use HashMap;

    let num_user_insts = user_instruction_info.len();
    let total_pcs: usize = user_pc_map.iter().map(|pcs| pcs.len()).sum();

    debug!(
        "Building PicoRV32 execution output: {} user instructions, {} total PCs mapped",
        num_user_insts, total_pcs
    );

    // Pre-allocate vectors
    let mut register_writes: Vec<Vec<TraitRegisterValue>> = Vec::with_capacity(num_user_insts);
    let mut memory_writes: Vec<Vec<TraitMemValue>> = Vec::with_capacity(num_user_insts);
    let mut exceptions: Vec<TraitExceptionInfo> = Vec::new();

    // Iterate through each user instruction (similar to Spike)
    for (user_idx, pcs) in user_pc_map.iter().enumerate() {
        // Collect register writes for this user instruction
        // Use HashMap to accumulate and deduplicate (last write wins)
        let mut reg_map: HashMap<String, (u64, u64)> = HashMap::new(); // name -> (pc, value)

        for &pc in pcs {
            if let Some(regs_at_pc) = trace.register_writes.get(&pc) {
                for (reg_name, write) in regs_at_pc {
                    reg_map
                        .entry(reg_name.clone())
                        .and_modify(|(existing_pc, val)| {
                            if pc > *existing_pc {
                                *existing_pc = pc;
                                *val = write.value;
                            }
                        })
                        .or_insert((pc, write.value));
                }
            }
        }

        // Convert to output format and sort
        let mut user_regs: Vec<_> = reg_map
            .into_iter()
            .map(|(name, (_pc, value))| TraitRegisterValue { name, value })
            .collect();

        user_regs.sort_by(|a, b| compare_register_names(&a.name, &b.name));
        register_writes.push(user_regs);

        // Collect memory writes for this user instruction
        let mut mem_map: HashMap<u64, (u64, u8)> = HashMap::new(); // addr -> (pc, value)

        for &pc in pcs {
            if let Some(mems_at_pc) = trace.memory_writes.get(&pc) {
                for (addr, write) in mems_at_pc {
                    // PicoRV32 stores full values, need to extract bytes based on SIZE
                    let bytes = memory_bytes_from_value(write.value, *addr, write.size);
                    for (byte_addr, byte_val) in bytes {
                        mem_map
                            .entry(byte_addr)
                            .and_modify(|(existing_pc, val)| {
                                if pc > *existing_pc {
                                    *existing_pc = pc;
                                    *val = byte_val;
                                }
                            })
                            .or_insert((pc, byte_val));
                    }
                }
            }
        }

        let mut user_mem: Vec<_> = mem_map
            .into_iter()
            .map(|(addr, (_pc, value))| TraitMemValue { addr, value })
            .collect();
        user_mem.sort_by_key(|m| m.addr);
        memory_writes.push(user_mem);

        // Collect exception at this user instruction (use highest PC if multiple)
        let mut exception_opt: Option<(u64, &ExceptionEvent)> = None;

        for &pc in pcs {
            if let Some(event) = trace.exceptions.get(&pc) {
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

        if let Some((_pc, event)) = exception_opt {
            let cause_string = if let Some(code) = event
                .mcause
                .or_else(|| canonical_cause_code_from_name(&event.cause))
            {
                format_cause_code(code)
            } else {
                if event.cause != "TRAP" {
                    warn!(
                        "PicoRV32: unknown exception cause '{}' at PC 0x{:x}",
                        event.cause, event.pc
                    );
                }
                event.cause.clone()
            };

            exceptions.push(TraitExceptionInfo {
                user_instruction_index: user_idx,
                cause: cause_string,
            });
        }
    }

    debug!(
        "PicoRV32 execution output built: {} register write sets, {} memory write sets, {} exceptions",
        register_writes.len(),
        memory_writes.len(),
        exceptions.len()
    );

    ExecutionOutput {
        riscv_impl,
        isa_base,
        register_write: register_writes,
        memory_write: memory_writes,
        exceptions,
    }
}

/// Extract bytes from a memory value based on SIZE field
/// PicoRV32 stores memory as u64 values with SIZE indicating the actual write width
fn memory_bytes_from_value(value: u64, base_addr: u64, size: u8) -> Vec<(u64, u8)> {
    let mut result = Vec::new();

    // Extract bytes based on SIZE (little-endian)
    // SIZE=1 for sb (store byte), SIZE=2 for sh (store half), SIZE=4 for sw (store word)
    let num_bytes = size as u64;
    for i in 0..num_bytes {
        let byte = ((value >> (i * 8)) & 0xFF) as u8;
        result.push((base_addr + i, byte));
    }

    result
}

fn build_user_pc_map(
    tracer: &Tracer,
    user_inst_count: usize,
) -> Result<Vec<Vec<u64>>, PicoRV32Error> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(LogParseError::NoPcMappingFound { index: idx })
            .map_err(PicoRV32Error::from)?;

        if pcs.is_empty() {
            return Err(PicoRV32Error::from(LogParseError::NoPcMappingFound {
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
