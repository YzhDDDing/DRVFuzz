use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::{error, info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::{
    build_elf::{ElfBuildResult, build_elf_with_extensions},
    error::{ImplExecutionError, LogParseError, ProcessError},
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

const IBEX_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_IBEX_RV32_BIN";

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub pc: u64,
    pub mcause: u64,
    pub cycle: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub pc: u64,
    pub reg: String,
    pub value: u64,
    pub instruction: Option<u32>,
    pub cycle: u64, // execution order discriminator
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWriteByte {
    pub addr: u64,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub addr: u64,
    pub value: u64,
    pub width: usize,
    pub mask: u64,
    pub instruction: Option<u32>,
    pub bytes: Vec<MemoryWriteByte>,
    pub cycle: u64, // execution order discriminator
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IbexTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>, // pc -> writes
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,     // pc -> writes
    pub exceptions: HashMap<u64, ExceptionEvent>,          // pc -> exception
}

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub ibex_trace_path: PathBuf,
}

impl TraceConfig {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            ibex_trace_path: path.into(),
        }
    }

    pub fn from_run_directory<P: AsRef<Path>>(dir: P) -> Self {
        let dir = dir.as_ref();
        // Default name that matches +ibex_tracer_file_base=... : <base>_00000000.log
        // Do not switch based on existence here to avoid pre-simulation fallback mistakes.
        Self::new(dir.join("ibex_trace_00000000.log"))
    }
}

fn is_command_available(path_or_cmd: &Path) -> bool {
    if path_or_cmd.is_absolute() || path_or_cmd.components().count() > 1 {
        return path_or_cmd.exists();
    }
    if let Some(path_env) = env::var_os("PATH") {
        for d in env::split_paths(&path_env) {
            if d.join(path_or_cmd).exists() {
                return true;
            }
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub run_root: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub timeout: Option<Duration>,
    pub ibex_binary: Option<PathBuf>,
}

impl ExecutorConfig {
    pub fn new(
        run_root: PathBuf,
        isa_base: ISABase,
        riscv_impl: RiscVImpl,
    ) -> Result<Self, ImplExecutionError> {
        if isa_base != ISABase::Rv32 {
            return Err(ImplExecutionError::Generic(
                "Ibex only supports RV32".into(),
            ));
        }
        Ok(Self {
            run_root,
            isa_base,
            riscv_impl,
            timeout: None,
            ibex_binary: None,
        })
    }
}

pub struct IbexExecutor {
    config: ExecutorConfig,
}

impl IbexExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    fn get_ibex_binary(&self) -> Result<PathBuf, ImplExecutionError> {
        if let Some(p) = &self.config.ibex_binary {
            return Ok(p.clone());
        }
        if let Ok(s) = env::var(IBEX_RV32_BINARY_ENV) {
            return Ok(PathBuf::from(s));
        }
        // Fallback: try repo-local prebuilt path riscv_impls_bins/ibex_rv32
        let fallback = PathBuf::from("riscv_impls_bins").join("ibex_rv32");
        if fallback.exists() {
            return Ok(fallback);
        }
        Err(ImplExecutionError::Generic(format!(
            "Ibex binary not found. Please set {} or place binary at ./riscv_impls_bins/ibex_rv32",
            IBEX_RV32_BINARY_ENV
        )))
    }

    fn ensure_dir(&self, d: &Path) -> io::Result<()> {
        fs::create_dir_all(d)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<ElfBuildResult, ImplExecutionError> {
        let asm_path = self.config.run_root.join("program.S");
        let asm = crate::impls::ibex::build_asm_content(user_insts, self.config.isa_base)
            .map_err(|e| ImplExecutionError::Generic(format!("Ibex asm build error: {e}")))?;
        fs::write(&asm_path, asm).map_err(ImplExecutionError::from)?;
        let ext = self.config.riscv_impl.extension_map();
        build_elf_with_extensions(
            &asm_path,
            &ext,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|e| ImplExecutionError::Generic(format!("Ibex ELF build failed: {e}")))
    }

    fn run_ibex(&self, elf_path: &Path, trace_base: &Path) -> Result<(), ImplExecutionError> {
        let ibex = self.get_ibex_binary()?;
        if !is_command_available(&ibex) {
            return Err(ImplExecutionError::Generic(format!(
                "Ibex binary not found: {}",
                ibex.display()
            )));
        }

        let mut cmd = Command::new(ibex);
        cmd.arg(format!("--meminit=ram,{}", elf_path.display()))
            .arg(format!("+ibex_tracer_file_base={}", trace_base.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        info!("Running Ibex: {:?}", cmd);

        let timeout = self.config.timeout.unwrap_or(Duration::from_secs(10));
        let start = Instant::now();
        let mut child = cmd.spawn().map_err(|e| ProcessError::SpawnFailed {
            command: "ibex_sim".into(),
            source: e,
        })?;

        // Poll with timeout
        loop {
            if let Some(status) = child.try_wait().map_err(|e| {
                ImplExecutionError::Generic(format!("ibex_sim try_wait failed: {e}"))
            })? {
                // Process exited
                if !status.success() {
                    error!("Ibex exited with code {:?}", status.code());
                    return Err(ImplExecutionError::Generic(format!(
                        "Ibex exited with non-zero status: {:?}",
                        status.code()
                    )));
                }
                let dur = Instant::now().duration_since(start);
                info!("Ibex finished in {:?}", dur);
                if dur > timeout {
                    warn!("Ibex run exceeded timeout {:?}", timeout);
                }
                break;
            }
            if Instant::now().duration_since(start) > timeout {
                // Timeout: kill process and report
                let _ = child.kill();
                let _ = child.wait(); // reap
                return Err(ImplExecutionError::from(ProcessError::TimedOut {
                    command: "ibex_sim".into(),
                    timeout,
                }));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    pub fn execute<P: AsRef<Path>>(
        &self,
        run_root: P,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, ImplExecutionError> {
        let run_root = run_root.as_ref();
        self.ensure_dir(run_root)
            .map_err(ImplExecutionError::from)?;

        // Build program and objdump
        let build = self
            .build_program(user_insts)
            .map_err(ImplExecutionError::from)?;

        // Prepare trace output path
        let trace_base = self.config.run_root.join("ibex_trace");
        let trace_cfg = TraceConfig::from_run_directory(&self.config.run_root);

        // Clean any previous trace (both possible names)
        let _ = fs::remove_file(self.config.run_root.join("ibex_trace_00000000.log"));
        let _ = fs::remove_file(self.config.run_root.join("trace_core_00000000.log"));

        // Run ibex verilator sim
        self.run_ibex(&build.executable_file, &trace_base)
            .map_err(ImplExecutionError::from)?;

        // Parse ibex tracer log
        let ibex_trace = parse_ibex_trace(&trace_cfg).map_err(ImplExecutionError::from)?;

        // Build user PC map using objdump
        let tracer =
            Tracer::new(user_insts, &build.disassembly_file).map_err(ImplExecutionError::from)?;
        let user_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_info.len())?;

        Ok(build_execution_output(
            ibex_trace,
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
) -> Result<Vec<Vec<u64>>, ImplExecutionError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer.get_all_pcs_for_user_inst(idx).ok_or_else(|| {
            ImplExecutionError::Generic(format!("No PC mapping for user inst {}", idx))
        })?;
        if pcs.is_empty() {
            return Err(ImplExecutionError::Generic(format!(
                "No PC mapping for user inst {}",
                idx
            )));
        }
        result.push(pcs.to_vec());
    }
    Ok(result)
}

fn build_execution_output(
    trace: IbexTrace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    let mut register_writes = Vec::with_capacity(user_instruction_info.len());
    let mut memory_writes = Vec::with_capacity(user_instruction_info.len());
    let mut exceptions = Vec::new();

    for (user_idx, pcs) in user_pc_map.iter().enumerate() {
        // Use cycle ordering to choose the last-effective write among multiple traces for the same user instruction
        let mut reg_map: HashMap<String, (u64, u64)> = HashMap::new(); // name -> (cycle, value)
        let mut mem_map: HashMap<u64, (u64, u8)> = HashMap::new(); // addr  -> (cycle, byte)
        let mut exception_opt: Option<(u64, &ExceptionEvent)> = None; // (cycle, event)

        for &pc in pcs {
            if let Some(wrs) = trace.register_writes.get(&pc) {
                for w in wrs {
                    reg_map
                        .entry(w.reg.clone())
                        .and_modify(|e| {
                            if w.cycle >= e.0 {
                                *e = (w.cycle, w.value)
                            }
                        })
                        .or_insert((w.cycle, w.value));
                }
            }
            if let Some(mws) = trace.memory_writes.get(&pc) {
                for mw in mws {
                    for b in &mw.bytes {
                        mem_map
                            .entry(b.addr)
                            .and_modify(|e| {
                                if mw.cycle >= e.0 {
                                    *e = (mw.cycle, b.value)
                                }
                            })
                            .or_insert((mw.cycle, b.value));
                    }
                }
            }
            if let Some(ev) = trace.exceptions.get(&pc) {
                if exception_opt.map(|(t, _)| ev.cycle >= t).unwrap_or(true) {
                    exception_opt = Some((ev.cycle, ev));
                }
            }
        }

        let regs_vec: Vec<TraitRegisterValue> = reg_map
            .into_iter()
            .map(|(name, (_pc, value))| TraitRegisterValue { name, value })
            .collect();
        let mems_vec: Vec<TraitMemValue> = mem_map
            .into_iter()
            .map(|(addr, (_pc, value))| TraitMemValue { addr, value })
            .collect();
        register_writes.push(regs_vec);
        memory_writes.push(mems_vec);

        if let Some((_pc, ev)) = exception_opt {
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

// Parse each line by splitting on tabs to avoid relying on spacing alignment.
// New value printouts use '=', source operands may use ':'. We only record writes ('=').
// Also accept optional 'f' prefix if future tracer emits FP reg writes.
// Only record actual register writes (like x5=0x... or f3=0x...). Skip xN:0x... (source operands).
static RE_REG_WRITE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s([xf]\d+)=0x([0-9a-fA-F]+)").unwrap());
static RE_MEM_PA: Lazy<Regex> = Lazy::new(|| Regex::new(r"PA:0x([0-9a-fA-F]+)").unwrap());
static RE_STORE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"store:0x([0-9a-fA-F]+)\s+wmask:0x([0-9a-fA-F]+)\s+wsize:(\d+)").unwrap()
});
static RE_EXC: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s(TRAP|INTR)(?:\s+mcause:0x([0-9a-fA-F]+))?").unwrap());

pub fn parse_ibex_trace(cfg: &TraceConfig) -> Result<IbexTrace, ImplExecutionError> {
    let mut path = cfg.ibex_trace_path.clone();
    // If the default ibex_trace_00000000.log is missing, fall back to trace_core_00000000.log
    if !path.exists() {
        let alt = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("trace_core_00000000.log");
        if alt.exists() {
            path = alt;
        }
    }
    info!("Parsing Ibex trace at {}", path.display());
    let f = File::open(&path).map_err(|e| LogParseError::LogFileReadError {
        path: path.clone(),
        source: e,
    })?;
    let mut reader = BufReader::new(f);
    let mut buf = String::new();
    let mut trace = IbexTrace::default();
    while reader
        .read_line(&mut buf)
        .map_err(|e| LogParseError::LogFileReadError {
            path: path.clone(),
            source: e,
        })?
        > 0
    {
        let line = buf.trim_end_matches(['\n', '\r']).to_string();
        if line.is_empty() || line.starts_with("Time\tCycle\tPC\tInsn") {
            buf.clear();
            continue;
        }

        let cols: Vec<&str> = line.split('\t').collect();
        // Need at least 5 columns (Time, Cycle, PC, Insn, Mnemonic).
        if cols.len() < 5 {
            buf.clear();
            continue;
        }

        // Parse the base fields
        let cycle: u64 = cols[1].trim().parse().unwrap_or(0);
        let pc: u64 = u64::from_str_radix(cols[2].trim(), 16).unwrap_or(0);
        let _insn_hex: Option<u32> = u32::from_str_radix(cols[3].trim(), 16).ok();

        // Columns from index 5 onward are optional and may contain exceptions, register writes, or memory writes; join them together
        let rest_buf = if cols.len() > 5 {
            cols[5..].join("\t")
        } else {
            String::new()
        };
        let rest = rest_buf.as_str();

        // Register writes (xN=...)
        for rc in RE_REG_WRITE.captures_iter(rest) {
            let reg = rc[1].to_string();
            // Ignore writes to x0 to avoid recording pseudo-writes (x0 is always zero)
            if reg == "x0" {
                continue;
            }
            let val = u64::from_str_radix(&rc[2], 16).unwrap_or(0);
            trace
                .register_writes
                .entry(pc)
                .or_default()
                .push(RegisterWrite {
                    pc,
                    reg,
                    value: val,
                    instruction: _insn_hex,
                    cycle,
                });
        }

        // Memory access: we only record store writes for memory_write; loads do not modify memory
        let mut base_addr_opt = None;
        if let Some(m) = RE_MEM_PA.captures(rest) {
            base_addr_opt = Some(u64::from_str_radix(&m[1], 16).unwrap_or(0));
        }
        if let (Some(base), Some(st)) = (base_addr_opt, RE_STORE.captures(rest)) {
            let value = u64::from_str_radix(&st[1], 16).unwrap_or(0);
            let wmask = u64::from_str_radix(&st[2], 16).unwrap_or(0);
            let wsize: usize = st[3].parse().unwrap_or(0);

            // Conservative handling: write wsize consecutive bytes (little endian), matching the Ibex tracer output.
            // For non-contiguous bytes in the future, wmask could be used to refine this.
            let mut bytes = Vec::with_capacity(wsize);
            for i in 0..wsize {
                let b = ((value >> (i * 8)) & 0xff) as u8;
                bytes.push(MemoryWriteByte {
                    addr: base + i as u64,
                    value: b,
                });
            }

            trace
                .memory_writes
                .entry(pc)
                .or_default()
                .push(MemoryWrite {
                    pc,
                    addr: base,
                    value,
                    width: wsize,
                    mask: wmask,
                    instruction: _insn_hex,
                    bytes,
                    cycle,
                });
        }

        // Exceptions / interrupts
        if let Some(ex) = RE_EXC.captures(rest) {
            let mcause = ex
                .get(2)
                .map(|m| u64::from_str_radix(m.as_str(), 16).unwrap_or(0))
                .unwrap_or(0);
            trace
                .exceptions
                .insert(pc, ExceptionEvent { pc, mcause, cycle });
        }

        buf.clear();
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Temporary hardcoded regression test: directly parse a user-provided real log.
    // Note: this test depends on local file paths and is only for debugging; CI can ignore/skip it.
    #[test]
    fn parse_real_ibex_log_should_not_be_empty() {
        let log_path = PathBuf::from(
            "/home/canxin/Git/DRVFuzz/temp/fuzz_rv32_spike_ibex/1762492393_497884016_rv32_0_w00/iter_000/Ibex/ibex_trace_00000000.log",
        );
        let dump_path = PathBuf::from(
            "/home/canxin/Git/DRVFuzz/temp/fuzz_rv32_spike_ibex/1762492393_497884016_rv32_0_w00/iter_000/Ibex/program.dump",
        );

        if !log_path.exists() {
            eprintln!("Skipping test: log not found at {}", log_path.display());
            return; // Local debugging only; skip when files are missing
        }
        if !dump_path.exists() {
            eprintln!("Skipping test: dump not found at {}", dump_path.display());
            return;
        }

        let cfg = TraceConfig::new(&log_path);
        let trace = parse_ibex_trace(&cfg).expect("failed to parse ibex trace");

        let regs_total: usize = trace.register_writes.values().map(|v| v.len()).sum();
        let mem_total: usize = trace.memory_writes.values().map(|v| v.len()).sum();
        let exc_total: usize = trace.exceptions.len();

        eprintln!(
            "ibex trace parsed: regs_pcs={}, mem_pcs={}, exc_pcs={}, total_regs={}, total_mems={}, total_excs={}",
            trace.register_writes.len(),
            trace.memory_writes.len(),
            trace.exceptions.len(),
            regs_total,
            mem_total,
            exc_total
        );

        assert!(
            regs_total > 0 || mem_total > 0 || exc_total > 0,
            "parsed ibex trace is empty"
        );
    }

    #[test]
    fn parse_exception_in_operand_column() {
        use std::fs;

        let tmp = tempfile::tempdir().expect("temp dir");
        let log_path = tmp.path().join("ibex_trace_00000000.log");
        let content = "Time\tCycle\tPC\tInsn\tMnemonic\tOperands\n5520\t2756\t00100cc6\t9002\tc.ebreak\t TRAP mcause:0x00000000\n";
        fs::write(&log_path, content).expect("write log");

        let cfg = TraceConfig::new(&log_path);
        let trace = parse_ibex_trace(&cfg).expect("parse trace");

        let evt = trace
            .exceptions
            .get(&0x0010_0cc6)
            .expect("exception missing");
        assert_eq!(evt.mcause, 0);
    }
}
