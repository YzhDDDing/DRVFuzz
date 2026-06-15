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
    extension_map::ExtensionMap,
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
    tracer::Tracer,
    user_instruction::UserInstructionInfo,
};
use riscv_instruction::separated_instructions::RV32Extensions;

const VEX_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_VEX_RV32_BIN";
const VEX_RV32F_BINARY_ENV: &str = "RISCV_WRAPPER_VEX_RV32F_BIN";
const VEX_RV32D_BINARY_ENV: &str = "RISCV_WRAPPER_VEX_RV32D_BIN";

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub pc: u64,
    pub mcause: u64,
    pub mtval: u64,
    pub cycle: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub pc: u64,
    pub reg: String,
    pub value: u64,
    pub cycle: u64,
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
    pub bytes: Vec<MemoryWriteByte>,
    pub cycle: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VexTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>, // pc -> writes
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,     // pc -> writes
    pub exceptions: HashMap<u64, ExceptionEvent>,          // pc -> exception
}

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub reg_trace: PathBuf,
    pub freg_trace: PathBuf,
    pub mem_trace: PathBuf,
    pub log_trace: PathBuf,
    pub dbg_trace: PathBuf,
    pub exc_trace: PathBuf,
}

impl TraceConfig {
    pub fn from_run_directory<P: AsRef<Path>>(dir: P) -> Self {
        let dir = dir.as_ref();
        Self {
            reg_trace: dir.join("run.regTrace"),
            freg_trace: dir.join("run.fregTrace"),
            mem_trace: dir.join("run.memTrace"),
            log_trace: dir.join("run.logTrace"),
            dbg_trace: dir.join("run.debugTrace"),
            exc_trace: dir.join("run.excTrace"),
        }
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
    pub vex_binary: Option<PathBuf>,
    pub extension_override: Option<ExtensionMap>,
    pub allow_unaligned: bool,
}

impl ExecutorConfig {
    pub fn new(
        run_root: PathBuf,
        isa_base: ISABase,
        riscv_impl: RiscVImpl,
    ) -> Result<Self, ImplExecutionError> {
        if isa_base != ISABase::Rv32 {
            return Err(ImplExecutionError::Generic("Vex only supports RV32".into()));
        }
        Ok(Self {
            run_root,
            isa_base,
            riscv_impl,
            timeout: None,
            vex_binary: None,
            extension_override: None,
            allow_unaligned: false,
        })
    }

    pub fn set_extension_override(&mut self, extension_map: ExtensionMap) {
        self.extension_override = Some(extension_map);
    }

    pub fn extension_map(&self) -> ExtensionMap {
        if let Some(map) = &self.extension_override {
            map.clone()
        } else {
            self.riscv_impl.extension_map()
        }
    }
}

pub struct VexExecutor {
    config: ExecutorConfig,
}

impl VexExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    fn get_vex_binary(&self) -> Result<PathBuf, ImplExecutionError> {
        if let Some(p) = &self.config.vex_binary {
            return Ok(p.clone());
        }
        // Decide whether we need an F-only or F+D binary based on the
        // effective extension map (including overrides).
        let ext_map = self.config.extension_map();
        let has_d = matches!(
            self.config.isa_base,
            ISABase::Rv32 if ext_map.rv32.contains(&RV32Extensions::D)
        );
        let has_f = matches!(
            self.config.isa_base,
            ISABase::Rv32 if ext_map.rv32.contains(&RV32Extensions::F)
        );

        // Helper: try environment variable then fallback path.
        fn pick_binary(env_name: &str, fallback_name: &str) -> Option<PathBuf> {
            if let Ok(s) = env::var(env_name) {
                let p = PathBuf::from(s);
                if p.exists() {
                    return Some(p);
                }
            }
            let fb = PathBuf::from("riscv_impls_bins").join(fallback_name);
            if fb.exists() {
                return Some(fb);
            }
            None
        }

        // Prefer RV32D binary when D is requested; otherwise, if only F is
        // requested, prefer RV32F binary; finally fall back to legacy
        // single-binary configuration.
        if has_d {
            if let Some(p) = pick_binary(VEX_RV32D_BINARY_ENV, "vex_rv32d") {
                return Ok(p);
            }
        } else if has_f {
            if let Some(p) = pick_binary(VEX_RV32F_BINARY_ENV, "vex_rv32f") {
                return Ok(p);
            }
        }

        // Legacy environment / path: single RV32 binary (typically RV32D).
        if let Ok(s) = env::var(VEX_RV32_BINARY_ENV) {
            let p = PathBuf::from(s);
            if p.exists() {
                return Ok(p);
            }
        }
        let fallback = PathBuf::from("riscv_impls_bins").join("vex_rv32");
        if fallback.exists() {
            return Ok(fallback);
        }
        Err(ImplExecutionError::Generic(format!(
            "Vex binary not found. Please set one of {}, {}, {} or place a binary at ./riscv_impls_bins/{{vex_rv32d,vex_rv32f,vex_rv32}}",
            VEX_RV32D_BINARY_ENV, VEX_RV32F_BINARY_ENV, VEX_RV32_BINARY_ENV
        )))
    }

    fn ensure_dir(&self, d: &Path) -> io::Result<()> {
        fs::create_dir_all(d)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<ElfBuildResult, ImplExecutionError> {
        let asm_path = self.config.run_root.join("program.S");
        let asm = crate::impls::vex::build_asm_content(user_insts, self.config.isa_base)
            .map_err(|e| ImplExecutionError::Generic(format!("Vex asm build error: {e}")))?;
        fs::write(&asm_path, asm).map_err(ImplExecutionError::from)?;
        let ext = self.config.extension_map();
        build_elf_with_extensions(
            &asm_path,
            &ext,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|e| ImplExecutionError::Generic(format!("Vex ELF build failed: {e}")))
    }

    fn run_vex(&self, elf_path: &Path) -> Result<(), ImplExecutionError> {
        let vex = self.get_vex_binary()?;
        if !is_command_available(&vex) {
            return Err(ImplExecutionError::Generic(format!(
                "Vex binary not found: {}",
                vex.display()
            )));
        }

        let mut cmd = Command::new(vex);
        // Run in run_root and pass relative path to avoid odd path handling in wrapper
        let rel = elf_path
            .file_name()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| elf_path.as_os_str().to_owned());
        cmd.arg(rel)
            .current_dir(&self.config.run_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        info!("Running Vex: {:?}", cmd);

        let timeout = self.config.timeout.unwrap_or(Duration::from_secs(10));
        let start = Instant::now();
        let mut child = cmd.spawn().map_err(|e| ProcessError::SpawnFailed {
            command: "vex_rv32".into(),
            source: e,
        })?;

        loop {
            if let Some(status) = child.try_wait().map_err(|e| {
                ImplExecutionError::Generic(format!("vex_rv32 try_wait failed: {e}"))
            })? {
                if !status.success() {
                    error!("Vex exited with code {:?}", status.code());
                    return Err(ImplExecutionError::Generic(format!(
                        "Vex exited with non-zero status: {:?}",
                        status.code()
                    )));
                }
                let dur = Instant::now().duration_since(start);
                info!("Vex finished in {:?}", dur);
                if dur > timeout {
                    warn!("Vex run exceeded timeout {:?}", timeout);
                }
                break;
            }
            if Instant::now().duration_since(start) > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ImplExecutionError::from(ProcessError::TimedOut {
                    command: "vex_rv32".into(),
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

        // Clean previous run traces
        let trace_cfg = TraceConfig::from_run_directory(run_root);
        let _ = fs::remove_file(&trace_cfg.reg_trace);
        let _ = fs::remove_file(&trace_cfg.freg_trace);
        let _ = fs::remove_file(&trace_cfg.mem_trace);
        let _ = fs::remove_file(&trace_cfg.log_trace);
        let _ = fs::remove_file(&trace_cfg.dbg_trace);
        let _ = fs::remove_file(&trace_cfg.exc_trace);

        let build = self
            .build_program(user_insts)
            .map_err(ImplExecutionError::from)?;
        self.run_vex(&build.executable_file)
            .map_err(ImplExecutionError::from)?;

        // Vex RV32 binaries can emit garbage in the upper 32 bits of FP registers; trim them when
        // we are not using a 64-bit floating-point extension.
        let ext_map = self.config.extension_map();
        let mask_freg_upper_32 = !ext_map.rv32.contains(&RV32Extensions::D)
            && !ext_map.rv32.contains(&RV32Extensions::Q);

        let trace =
            parse_vex_traces(&trace_cfg, mask_freg_upper_32).map_err(ImplExecutionError::from)?;

        // Build user PC map using objdump
        let tracer =
            Tracer::new(user_insts, &build.disassembly_file).map_err(ImplExecutionError::from)?;
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
    trace: VexTrace,
    user_instruction_info: Vec<UserInstructionInfo>,
    user_pc_map: Vec<Vec<u64>>,
    riscv_impl: RiscVImpl,
    isa_base: ISABase,
) -> ExecutionOutput {
    let mut register_writes = Vec::with_capacity(user_instruction_info.len());
    let mut memory_writes = Vec::with_capacity(user_instruction_info.len());
    let mut exceptions = Vec::new();

    for (user_idx, pcs) in user_pc_map.iter().enumerate() {
        // Choose last-by-cycle write when multiple PCs map to same user inst
        let mut reg_map: HashMap<String, (u64, u64)> = HashMap::new(); // name -> (cycle, value)
        let mut mem_map: HashMap<u64, (u64, u8)> = HashMap::new(); // addr -> (cycle, byte)
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
            .map(|(name, (_t, value))| TraitRegisterValue { name, value })
            .collect();
        let mems_vec: Vec<TraitMemValue> = mem_map
            .into_iter()
            .map(|(addr, (_t, value))| TraitMemValue { addr, value })
            .collect();
        register_writes.push(regs_vec);
        memory_writes.push(mems_vec);

        if let Some((_t, ev)) = exception_opt {
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

// Vex run.regTrace lines:
// "PC 80000024 : reg[ 8] = 12345678"
static RE_REG_WRITE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"PC\s+([0-9a-fA-F]+)\s+:\s+reg\[\s*(\d+)\s*\]\s*=\s*([0-9a-fA-F]+)").unwrap()
});
// Vex run.memTrace lines (writes):
// Legacy format: "PC 8000008e W SZ 4 @80000140 = 0x00000015"
// New format (with optional leading timestamp):
// "364 PC 80000008 : MEM[0x80000310] <= 4 bytes : 0x12345678"
static RE_MEM_WRITE_NEW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^\s*(?:\d+\s+)?PC\s+([0-9a-fA-F]+)\s+:\s+MEM\[(?:0x)?([0-9a-fA-F]+)\]\s+<=\s+(\d+)\s+bytes\s+:\s+0x([0-9a-fA-F]+)",
    )
    .unwrap()
});
static RE_MEM_WRITE_OLD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"PC\s+([0-9a-fA-F]+)\s+W\s+SZ\s+(\d+)\s+@([0-9a-fA-F]+)\s+=\s*(?:0x)?([0-9a-fA-F]+)",
    )
    .unwrap()
});
// Vex legacy run.debugTrace lines:
// "TRAP PC 80000044 MCAUSE 0x80000004 MTVAL 0x5e2fc750 time=402"
static RE_TRAP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"TRAP\s+PC\s+([0-9a-fA-F]+)\s+MCAUSE\s+0x([0-9a-fA-F]+)\s+MTVAL\s+0x([0-9a-fA-F]+)\s+time=(\d+)").unwrap()
});

// run.excTrace lines (optional, newer format used by some harnesses):
// "time=402 pc=0x80000044 cause=11 interrupt=0 badaddr=0x00000073 desc=machine ecall"
static RE_EXC_TRACE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^time=(\d+)\s+pc=0x([0-9a-fA-F]+)\s+cause=(\d+)\s+interrupt=(\d+)\s+badaddr=0x([0-9a-fA-F]+)",
    )
    .unwrap()
});
// Newer Vex main.cpp emits exception logs into run.logTrace as:
// "EXC pc=0x80000044 cause=11"
static RE_EXC_LOG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^EXC\s+pc=0x([0-9a-fA-F]+)\s+cause=(\d+)").unwrap());
// Vex run.fregTrace lines (optional leading timestamp):
// "954 PC 800000dc : f[00] = 0x3fc000003fc00000"
static RE_FREG_WRITE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:\d+\s+)?PC\s+([0-9a-fA-F]+)\s+:\s+f\[\s*(\d+)\s*\]\s*=\s*0x([0-9a-fA-F]+)")
        .unwrap()
});

fn parse_vex_traces(
    cfg: &TraceConfig,
    mask_freg_upper_32: bool,
) -> Result<VexTrace, LogParseError> {
    let mut trace = VexTrace::default();
    // Parse regTrace
    let mut cycle: u64 = 0;
    if cfg.reg_trace.exists() {
        let f = File::open(&cfg.reg_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.reg_trace.clone(),
            source: e,
        })?;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.reg_trace.clone(),
                source: e,
            })?;
            if let Some(cap) = RE_REG_WRITE.captures(&line) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let reg_num: u8 = cap[2].parse().unwrap_or(255);
                let val = u64::from_str_radix(&cap[3], 16).unwrap_or(0);
                let name = format!("x{}", reg_num);
                trace
                    .register_writes
                    .entry(pc)
                    .or_default()
                    .push(RegisterWrite {
                        pc,
                        reg: name,
                        value: val,
                        cycle,
                    });
                cycle += 1;
            }
        }
    }
    // Parse fregTrace (FP register writes)
    if cfg.freg_trace.exists() {
        let f = File::open(&cfg.freg_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.freg_trace.clone(),
            source: e,
        })?;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.freg_trace.clone(),
                source: e,
            })?;
            if let Some(cap) = RE_FREG_WRITE.captures(&line) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let freg_num: u8 = cap[2].parse().unwrap_or(255);
                let mut val = u64::from_str_radix(&cap[3], 16).unwrap_or(0);
                if mask_freg_upper_32 {
                    val &= 0xffff_ffff;
                }
                let name = format!("f{}", freg_num);
                trace
                    .register_writes
                    .entry(pc)
                    .or_default()
                    .push(RegisterWrite {
                        pc,
                        reg: name,
                        value: val,
                        cycle,
                    });
                cycle += 1;
            }
        }
    }
    // Parse memTrace (writes only)
    if cfg.mem_trace.exists() {
        let f = File::open(&cfg.mem_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.mem_trace.clone(),
            source: e,
        })?;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.mem_trace.clone(),
                source: e,
            })?;
            let mut parsed: Option<(u64, usize, u64, String)> = None;
            if let Some(cap) = RE_MEM_WRITE_NEW.captures(&line) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let sz: usize = cap[3].parse().unwrap_or(0);
                let addr = u64::from_str_radix(&cap[2], 16).unwrap_or(0);
                let hex = cap[4].to_string();
                parsed = Some((pc, sz, addr, hex));
            } else if let Some(cap) = RE_MEM_WRITE_OLD.captures(&line) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let sz: usize = cap[2].parse().unwrap_or(0);
                let addr = u64::from_str_radix(&cap[3], 16).unwrap_or(0);
                let hex = cap[4].to_string();
                parsed = Some((pc, sz, addr, hex));
            }
            if let Some((pc, sz, addr, hex)) = parsed {
                // hex string prints bytes from high->low; reconstruct little-endian bytes for addr..addr+sz-1
                let mut bytes = vec![0u8; sz];
                let mut hex_clean = hex;
                // Pad left if needed
                if hex_clean.len() < sz * 2 {
                    hex_clean = format!("{:0>width$}", hex_clean, width = sz * 2);
                }
                // For i in 0..sz: byte i at address addr+i equals hex pair at position from right
                for i in 0..sz {
                    let start = hex_clean.len().saturating_sub((i + 1) * 2);
                    let end = hex_clean.len().saturating_sub(i * 2);
                    let byte_str = &hex_clean[start..end];
                    bytes[i] = u8::from_str_radix(byte_str, 16).unwrap_or(0);
                }
                // Build MemoryWrite
                let mut value_u64: u64 = 0;
                for (i, b) in bytes.iter().enumerate() {
                    value_u64 |= (*b as u64) << (8 * i.min(7));
                }
                let bytes_out: Vec<MemoryWriteByte> = bytes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| MemoryWriteByte {
                        addr: addr + i as u64,
                        value: *b,
                    })
                    .collect();
                trace
                    .memory_writes
                    .entry(pc)
                    .or_default()
                    .push(MemoryWrite {
                        pc,
                        addr,
                        value: value_u64,
                        width: sz,
                        bytes: bytes_out,
                        cycle,
                    });
                cycle += 1;
            }
        }
    }
    // Prefer the new excTrace if available, otherwise fall back to debugTrace parsing
    let mut parsed_exc = false;
    if cfg.exc_trace.exists() {
        let f = File::open(&cfg.exc_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.exc_trace.clone(),
            source: e,
        })?;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.exc_trace.clone(),
                source: e,
            })?;
            if let Some(cap) = RE_EXC_TRACE.captures(line.trim()) {
                let cycle = cap[1].parse().unwrap_or(0);
                let pc = u64::from_str_radix(&cap[2], 16).unwrap_or(0);
                let cause = u64::from_str_radix(&cap[3], 10).unwrap_or(0);
                let interrupt_flag: u64 = cap[4].parse().unwrap_or(0);
                let badaddr = u64::from_str_radix(&cap[5], 16).unwrap_or(0);
                let mcause = if interrupt_flag != 0 {
                    (1u64 << 63) | cause
                } else {
                    cause
                };
                trace.exceptions.insert(
                    pc,
                    ExceptionEvent {
                        pc,
                        mcause,
                        mtval: badaddr,
                        cycle,
                    },
                );
            }
        }
        if !trace.exceptions.is_empty() {
            parsed_exc = true;
        }
    }

    // Next, parse EXC lines from run.logTrace if present.
    if !parsed_exc && cfg.log_trace.exists() {
        let f = File::open(&cfg.log_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.log_trace.clone(),
            source: e,
        })?;
        let mut exc_cycle: u64 = 0;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.log_trace.clone(),
                source: e,
            })?;
            if let Some(cap) = RE_EXC_LOG.captures(line.trim()) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let cause: u64 = cap[2].parse().unwrap_or(0);
                trace.exceptions.insert(
                    pc,
                    ExceptionEvent {
                        pc,
                        mcause: cause,
                        mtval: 0,
                        cycle: exc_cycle,
                    },
                );
                exc_cycle += 1;
            }
        }
        if !trace.exceptions.is_empty() {
            parsed_exc = true;
        }
    }

    if !parsed_exc && cfg.dbg_trace.exists() {
        let f = File::open(&cfg.dbg_trace).map_err(|e| LogParseError::LogFileReadError {
            path: cfg.dbg_trace.clone(),
            source: e,
        })?;
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| LogParseError::LogFileReadError {
                path: cfg.dbg_trace.clone(),
                source: e,
            })?;
            if let Some(cap) = RE_TRAP.captures(&line) {
                let pc = u64::from_str_radix(&cap[1], 16).unwrap_or(0);
                let mcause = u64::from_str_radix(&cap[2], 16).unwrap_or(0);
                let mtval = u64::from_str_radix(&cap[3], 16).unwrap_or(0);
                let cyc: u64 = cap[4].parse().unwrap_or(0);
                trace.exceptions.insert(
                    pc,
                    ExceptionEvent {
                        pc,
                        mcause,
                        mtval,
                        cycle: cyc,
                    },
                );
            }
        }
    }
    Ok(trace)
}
