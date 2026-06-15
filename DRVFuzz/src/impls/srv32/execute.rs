use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use log::{debug, info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{ImplExecutionError, LogParseError, ParseError, ProcessError, Srv32Error},
    exception_cause::format_cause_code,
    execution_output::{
        ExceptionInfo as TraitExceptionInfo, ExecutionOutput, MemValue as TraitMemValue,
        RegisterValue as TraitRegisterValue,
    },
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
    tracer::Tracer,
    user_instruction::UserInstructionInfo,
};

const SRV32_BINARY_ENV: &str = "RISCV_WRAPPER_SRV32_BIN";

#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub index: u8,
    pub pc: u64,
    pub register: String,
    pub value: u64,
    pub cycle: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub address: u64,
    pub value: u64,
    pub width: u8,
    pub cycle: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub pc: u64,
    pub mcause: u64,
    pub cycle: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Srv32Trace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>, // pc -> writes
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,     // pc -> writes
    pub exceptions: HashMap<u64, ExceptionEvent>,          // mepc -> exception
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub run_root: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub timeout: Option<Duration>,
    pub srv32_binary: Option<PathBuf>,
    pub extra_args: Vec<String>,
}

impl ExecutorConfig {
    pub fn new(
        run_root: PathBuf,
        isa_base: ISABase,
        riscv_impl: RiscVImpl,
    ) -> Result<Self, ImplExecutionError> {
        if isa_base != ISABase::Rv32 {
            return Err(Srv32Error::UnsupportedIsaBase {
                isa_base: isa_base.to_string(),
            }
            .into());
        }
        Ok(Self {
            run_root,
            isa_base,
            riscv_impl,
            timeout: None,
            srv32_binary: None,
            extra_args: Vec::new(),
        })
    }

    fn extension_map(&self) -> crate::extension_map::ExtensionMap {
        self.riscv_impl.extension_map()
    }
}

pub struct Srv32Executor {
    config: ExecutorConfig,
}

impl Srv32Executor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    fn ensure_dir(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)
    }

    fn get_srv32_binary(&self) -> Result<PathBuf, Srv32Error> {
        if let Some(p) = &self.config.srv32_binary {
            return Ok(p.clone());
        }

        if let Ok(env_path) = env::var(SRV32_BINARY_ENV) {
            let path = PathBuf::from(env_path);
            if path.exists() {
                return Ok(path);
            }
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            manifest_dir.join("riscv_impls_bins").join("srv32"),
            manifest_dir
                .join("..")
                .join("srv32")
                .join("tools")
                .join("rvsim"),
        ];
        for cand in candidates.iter() {
            if cand.exists() {
                return Ok(cand.clone());
            }
        }

        Err(Srv32Error::BinaryNotFound {
            path: format!(
                "set {} or place binary at {:?}",
                SRV32_BINARY_ENV,
                manifest_dir.join("riscv_impls_bins/srv32")
            ),
        })
    }

    fn build_program(&self, user_insts: &[String]) -> Result<ElfBuildResult, Srv32Error> {
        let asm_path = self.config.run_root.join("program.S");
        let asm = crate::impls::srv32::build_asm_content(user_insts, self.config.isa_base)
            .map_err(Srv32Error::from)?;
        fs::write(&asm_path, asm)?;
        let extension_map = self.config.extension_map();
        build_elf_with_extensions(
            &asm_path,
            &extension_map,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(Srv32Error::from)
    }

    fn run_srv32(&self, elf_path: &Path, log_file: &Path) -> Result<(), Srv32Error> {
        let binary = self.get_srv32_binary()?;
        let binary_name = binary
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let has_plusargs = self
            .config
            .extra_args
            .iter()
            .any(|arg| arg.starts_with('+'));
        let is_rtl_sim = has_plusargs || binary_name.contains("srv32_cov");

        let mut cmd = Command::new(&binary);
        if is_rtl_sim {
            // RTL sim emits trace.log with +trace instead of -l.
            if !self.config.extra_args.iter().any(|arg| arg == "+trace") {
                cmd.arg("+trace");
            }
        } else {
            cmd.arg("-l").arg(log_file);
        }
        cmd.args(&self.config.extra_args);
        cmd.arg(elf_path);
        cmd.current_dir(&self.config.run_root);
        info!("Running Srv32: {:?}", cmd);

        let timeout = self.config.timeout.unwrap_or(Duration::from_secs(10));
        let start = Instant::now();
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProcessError::SpawnFailed {
                command: format!("{binary:?}"),
                source: e,
            })?;

        loop {
            if let Some(_status) = child.try_wait().map_err(|e| ProcessError::SpawnFailed {
                command: format!("{binary:?}"),
                source: e,
            })? {
                let output = child
                    .wait_with_output()
                    .map_err(|e| ProcessError::SpawnFailed {
                        command: format!("{binary:?}"),
                        source: e,
                    })?;
                if !output.status.success() {
                    let has_log = log_file
                        .exists()
                        .then(|| fs::metadata(log_file).map(|m| m.len() > 0).unwrap_or(false))
                        .unwrap_or(false);
                    if has_log {
                        warn!(
                            "Srv32 exited with code {:?}; continuing with log parsing. stderr: {}",
                            output.status.code(),
                            String::from_utf8_lossy(&output.stderr)
                        );
                    } else {
                        return Err(ProcessError::ProcessFailed {
                            command: binary.display().to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        }
                        .into());
                    }
                }
                break;
            }

            if Instant::now().duration_since(start) > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcessError::TimedOut {
                    command: binary.display().to_string(),
                    timeout,
                }
                .into());
            }
            thread::sleep(Duration::from_millis(10));
        }

        if !log_file.exists() {
            let trace_path = self.config.run_root.join("trace.log");
            if trace_path.exists() {
                if let Err(err) = fs::rename(&trace_path, log_file) {
                    // Fallback if cross-device rename fails.
                    if let Err(copy_err) = fs::copy(&trace_path, log_file) {
                        return Err(ProcessError::ProcessFailed {
                            command: binary.display().to_string(),
                            stderr: format!(
                                "failed to move trace.log to srv32.log: {err}; copy error: {copy_err}"
                            ),
                        }
                        .into());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn execute<P: AsRef<Path>>(
        &self,
        run_root: P,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, Srv32Error> {
        let run_root = run_root.as_ref();
        self.ensure_dir(run_root)?;

        let build = self.build_program(user_insts)?;

        let log_path = self.config.run_root.join("srv32.log");
        self.run_srv32(&build.executable_file, &log_path)?;

        let dump_content = fs::read_to_string(&build.disassembly_file)?;
        let trap_handler_pcs = collect_trap_handler_pcs(&dump_content);

        let trace = parse_srv32_log(&log_path, &trap_handler_pcs)?;

        let tracer = Tracer::new(user_insts, &build.disassembly_file)?;
        let user_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_info.len())?;

        Ok(build_execution_output(
            trace,
            user_info,
            user_pc_map,
            self.config.riscv_impl,
            self.config.isa_base,
        ))
    }
}

fn build_user_pc_map(
    tracer: &Tracer,
    user_inst_count: usize,
) -> Result<Vec<Vec<u64>>, LogParseError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(LogParseError::NoPcMappingFound { index: idx })?;
        if pcs.is_empty() {
            return Err(LogParseError::NoPcMappingFound { index: idx });
        }
        result.push(pcs.to_vec());
    }
    Ok(result)
}

fn build_execution_output(
    trace: Srv32Trace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    let mut register_writes = Vec::with_capacity(user_instruction_info.len());
    let mut memory_writes = Vec::with_capacity(user_instruction_info.len());
    let mut exceptions = Vec::new();

    for (user_idx, pcs) in user_pc_map.iter().enumerate() {
        let mut reg_map: HashMap<String, (u64, u64)> = HashMap::new();
        let mut mem_map: HashMap<u64, (u64, u8)> = HashMap::new();
        let mut exception_opt: Option<(u64, &ExceptionEvent)> = None;

        for &pc in pcs {
            if let Some(wrs) = trace.register_writes.get(&pc) {
                for w in wrs {
                    reg_map
                        .entry(w.register.clone())
                        .and_modify(|e| {
                            if w.cycle >= e.0 {
                                *e = (w.cycle, w.value);
                            }
                        })
                        .or_insert((w.cycle, w.value));
                }
            }

            if let Some(mws) = trace.memory_writes.get(&pc) {
                for mw in mws {
                    for (addr, byte) in store_bytes_from_write(mw) {
                        mem_map
                            .entry(addr)
                            .and_modify(|e| {
                                if mw.cycle >= e.0 {
                                    *e = (mw.cycle, byte);
                                }
                            })
                            .or_insert((mw.cycle, byte));
                    }
                }
            }

            if let Some(ev) = trace.exceptions.get(&pc) {
                if exception_opt
                    .map(|(cycle, _)| ev.cycle >= cycle)
                    .unwrap_or(true)
                {
                    exception_opt = Some((ev.cycle, ev));
                }
            }
        }

        let mut regs_vec: Vec<TraitRegisterValue> = reg_map
            .into_iter()
            .map(|(name, (_t, value))| TraitRegisterValue { name, value })
            .collect();
        regs_vec.sort_by(|a, b| a.name.cmp(&b.name));

        let mut mems_vec: Vec<TraitMemValue> = mem_map
            .into_iter()
            .map(|(addr, (_t, value))| TraitMemValue { addr, value })
            .collect();
        mems_vec.sort_by_key(|m| m.addr);

        register_writes.push(regs_vec);
        memory_writes.push(mems_vec);

        if let Some((_cycle, ev)) = exception_opt {
            exceptions.push(TraitExceptionInfo {
                user_instruction_index: user_idx,
                cause: format_cause_code(ev.mcause),
            });
        }
    }

    ExecutionOutput {
        exceptions,
        register_write: register_writes,
        memory_write: memory_writes,
        riscv_impl,
        isa_base,
    }
}

fn collect_trap_handler_pcs(dump: &str) -> HashSet<u64> {
    let mut pcs = HashSet::new();
    let mut in_trap = false;
    for line in dump.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<trap_handler>:") {
            in_trap = true;
            continue;
        }
        if in_trap && trimmed.contains('<') && trimmed.ends_with(':') {
            in_trap = false;
            continue;
        }
        if in_trap {
            if let Some(pc) = parse_pc_from_line(trimmed) {
                pcs.insert(pc);
            }
        }
    }
    pcs
}

fn parse_pc_from_line(line: &str) -> Option<u64> {
    let colon = line.find(':')?;
    let addr_str = line[..colon].trim();
    u64::from_str_radix(addr_str, 16).ok()
}

fn parse_srv32_log<P: AsRef<Path>>(
    path: P,
    trap_handler_pcs: &HashSet<u64>,
) -> Result<Srv32Trace, LogParseError> {
    static LINE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\s*(\d+)\s+([0-9a-fA-F]{8})\s+([0-9a-fA-F]{8})(?:\s+(.*))?$").unwrap()
    });
    static REG_WRITE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"x(?P<idx>\d{2})\s+\((?P<name>[^)]+)\)\s+<=\s+0x(?P<value>[0-9a-fA-F]+)")
            .unwrap()
    });
    static MEM_WRITE_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"write\s+0x(?P<addr>[0-9a-fA-F]+)\s+<=\s+0x(?P<value>[0-9a-fA-F]+)").unwrap()
    });

    let file = File::open(path.as_ref()).map_err(|e| LogParseError::LogFileReadError {
        path: path.as_ref().to_path_buf(),
        source: e,
    })?;

    let mut trace = Srv32Trace::default();
    let mut pending_mepc: Option<u64> = None;
    let mut pending_mcause: Option<(u64, u64)> = None;

    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| LogParseError::LogFileReadError {
            path: path.as_ref().to_path_buf(),
            source: e,
        })?;
        let Some(caps) = LINE_RE.captures(&line) else {
            continue;
        };

        let cycle: u64 = caps[1]
            .trim()
            .parse()
            .map_err(|e| LogParseError::CaptureParseError {
                source: ParseError::IntParseError {
                    value: caps[1].to_string(),
                    source: e,
                },
            })?;
        let pc =
            u64::from_str_radix(&caps[2], 16).map_err(|e| LogParseError::CaptureParseError {
                source: ParseError::HexParseError {
                    value: caps[2].to_string(),
                    source: e,
                },
            })?;
        let inst =
            u32::from_str_radix(&caps[3], 16).map_err(|e| LogParseError::CaptureParseError {
                source: ParseError::HexParseError {
                    value: caps[3].to_string(),
                    source: e,
                },
            })?;
        let tail = caps.get(4).map(|m| m.as_str().trim()).unwrap_or("");

        if let Some(mem_caps) = MEM_WRITE_RE.captures(tail) {
            let addr = u64::from_str_radix(&mem_caps["addr"], 16).map_err(|e| {
                LogParseError::CaptureParseError {
                    source: ParseError::HexParseError {
                        value: mem_caps["addr"].to_string(),
                        source: e,
                    },
                }
            })?;
            let value = u64::from_str_radix(&mem_caps["value"], 16).map_err(|e| {
                LogParseError::CaptureParseError {
                    source: ParseError::HexParseError {
                        value: mem_caps["value"].to_string(),
                        source: e,
                    },
                }
            })?;
            let width = store_width_from_inst(inst);
            trace
                .memory_writes
                .entry(pc)
                .or_default()
                .push(MemoryWrite {
                    pc,
                    address: addr,
                    value,
                    width,
                    cycle,
                });
        }

        if let Some(reg_caps) = REG_WRITE_RE.captures(tail) {
            let idx: u8 =
                reg_caps["idx"]
                    .parse()
                    .map_err(|e| LogParseError::CaptureParseError {
                        source: ParseError::IntParseError {
                            value: reg_caps["idx"].to_string(),
                            source: e,
                        },
                    })?;
            let value = u64::from_str_radix(&reg_caps["value"], 16).map_err(|e| {
                LogParseError::CaptureParseError {
                    source: ParseError::HexParseError {
                        value: reg_caps["value"].to_string(),
                        source: e,
                    },
                }
            })?;
            let reg_name = format!("x{}", idx);
            trace
                .register_writes
                .entry(pc)
                .or_default()
                .push(RegisterWrite {
                    index: idx,
                    pc,
                    register: reg_name.clone(),
                    value,
                    cycle,
                });

            if trap_handler_pcs.contains(&pc) {
                if idx == 5 {
                    // New trap invocation, clear any stale mcause from previous trap
                    pending_mcause = None;
                    pending_mepc = Some(value);
                } else if idx == 6 {
                    pending_mcause = Some((cycle, value));
                }

                if let (Some(mepc), Some((c_cycle, mcause))) = (pending_mepc, pending_mcause) {
                    trace.exceptions.insert(
                        mepc,
                        ExceptionEvent {
                            pc: mepc,
                            mcause,
                            cycle: c_cycle,
                        },
                    );
                    pending_mepc = None;
                    pending_mcause = None;
                }
            }
        }
    }

    debug!(
        "Parsed Srv32 log: {} pcs with reg writes, {} pcs with mem writes, {} exceptions",
        trace.register_writes.len(),
        trace.memory_writes.len(),
        trace.exceptions.len()
    );
    Ok(trace)
}

fn store_width_from_inst(inst: u32) -> u8 {
    let funct3 = (inst >> 12) & 0x7;
    match funct3 {
        0b000 => 1,
        0b001 => 2,
        _ => 4,
    }
}

fn store_bytes_from_write(write: &MemoryWrite) -> Vec<(u64, u8)> {
    let mut bytes = Vec::new();
    for i in 0..write.width {
        let byte = ((write.value >> (8 * i)) & 0xFF) as u8;
        bytes.push((write.address + i as u64, byte));
    }
    bytes
}
