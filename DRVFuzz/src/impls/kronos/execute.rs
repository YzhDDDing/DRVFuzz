use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::{error, info, warn};
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

const KRONOS_RV32_BINARY_ENV: &str = "RISCV_WRAPPER_KRONOS_RV32_BIN";

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
pub struct KronosTrace {
    pub register_writes: HashMap<u64, Vec<RegisterWrite>>, // pc -> writes
    pub memory_writes: HashMap<u64, Vec<MemoryWrite>>,     // pc -> writes
    pub exceptions: HashMap<u64, ExceptionEvent>,          // pc -> exception
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub run_root: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub timeout: Option<Duration>,
    pub kronos_binary: Option<PathBuf>,
}

impl ExecutorConfig {
    pub fn new(
        run_root: PathBuf,
        isa_base: ISABase,
        riscv_impl: RiscVImpl,
    ) -> Result<Self, ImplExecutionError> {
        if isa_base != ISABase::Rv32 {
            return Err(ImplExecutionError::Generic(
                "Kronos only supports RV32".into(),
            ));
        }
        Ok(Self {
            run_root,
            isa_base,
            riscv_impl,
            timeout: None,
            kronos_binary: None,
        })
    }
}

pub struct KronosExecutor {
    config: ExecutorConfig,
}

impl KronosExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    fn get_kronos_binary(&self) -> Result<PathBuf, ImplExecutionError> {
        if let Some(p) = &self.config.kronos_binary {
            return Ok(p.clone());
        }
        if let Ok(s) = env::var(KRONOS_RV32_BINARY_ENV) {
            return Ok(PathBuf::from(s));
        }
        // Fallbacks: repo-local or kronos repo default path
        let local = PathBuf::from("riscv_impls_bins").join("kronos_rv32");
        if local.exists() {
            return Ok(local);
        }
        let kronos_repo = PathBuf::from("/home/canxin/Git/kronos/build_result/kronos_rv32");
        if kronos_repo.exists() {
            return Ok(kronos_repo);
        }
        Err(ImplExecutionError::Generic(
            "Kronos runner not found; set RISCV_WRAPPER_KRONOS_RV32_BIN".into(),
        ))
    }

    fn ensure_dir(&self, d: &Path) -> io::Result<()> {
        fs::create_dir_all(d)
    }

    fn build_program(&self, user_insts: &[String]) -> Result<ElfBuildResult, ImplExecutionError> {
        let asm_path = self.config.run_root.join("program.S");
        let asm = crate::impls::kronos::build_asm_content(user_insts, self.config.isa_base)
            .map_err(|e| ImplExecutionError::Generic(format!("Kronos asm build error: {e}")))?;
        fs::write(&asm_path, asm).map_err(ImplExecutionError::from)?;
        let ext = self.config.riscv_impl.extension_map();
        build_elf_with_extensions(
            &asm_path,
            &ext,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )
        .map_err(|e| ImplExecutionError::Generic(format!("Kronos ELF build failed: {e}")))
    }

    fn run_kronos(&self, elf_path: &Path, log_file: &Path) -> Result<(), ImplExecutionError> {
        let kronos = self.get_kronos_binary()?;
        let mut cmd = Command::new(kronos);
        cmd.arg(elf_path)
            .arg("--tohost")
            .arg("0x100")
            .arg("--max-cycles")
            .arg("200000")
            .arg("--log")
            .arg("reg,mem,trap");
        info!("Running Kronos: {:?}", cmd);

        let timeout = self.config.timeout.unwrap_or(Duration::from_secs(10));
        let start = Instant::now();
        let stdout_file = File::create(log_file).map_err(ImplExecutionError::from)?;
        let mut child = cmd
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProcessError::SpawnFailed {
                command: "kronos_rv32".into(),
                source: e,
            })?;

        loop {
            if let Some(status) = child.try_wait().map_err(|e| {
                ImplExecutionError::Generic(format!("kronos_rv32 try_wait failed: {e}"))
            })? {
                if !status.success() {
                    error!("kronos_rv32 exited with code {:?}", status.code());
                    return Err(ImplExecutionError::Generic(format!(
                        "kronos_rv32 exited with non-zero status: {:?}",
                        status.code()
                    )));
                }
                let dur = Instant::now().duration_since(start);
                info!("Kronos finished in {:?}", dur);
                if dur > timeout {
                    warn!("Kronos run exceeded timeout {:?}", timeout);
                }
                break;
            }
            if Instant::now().duration_since(start) > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ImplExecutionError::from(ProcessError::TimedOut {
                    command: "kronos_rv32".into(),
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

        // Run kronos runner and capture stdout log
        let log_path = self.config.run_root.join("kronos.log");
        self.run_kronos(&build.executable_file, &log_path)?;

        // Parse logs
        let kronos_trace = parse_kronos_log(&log_path)?;

        // Build user PC map using objdump
        let tracer =
            Tracer::new(user_insts, &build.disassembly_file).map_err(ImplExecutionError::from)?;
        let user_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_info.len())?;

        Ok(build_execution_output(
            kronos_trace,
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
    trace: KronosTrace,
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

fn parse_kronos_log<P: AsRef<Path>>(path: P) -> Result<KronosTrace, LogParseError> {
    let re_reg =
        Regex::new(r"^\[REG\]\s+pc=0x([0-9a-fA-F]+)\s+x(\d+)\s+<=\s+0x([0-9a-fA-F]+)").unwrap();
    let re_mem = Regex::new(r"^\[MEMW\]\s+pc=0x([0-9a-fA-F]+)\s+addr=0x([0-9a-fA-F]+)\s+data=0x([0-9a-fA-F]+)\s+mask=0x([0-9a-fA-F]+)").unwrap();
    let re_trap = Regex::new(r"^\[TRAP\]\s+pc=0x([0-9a-fA-F]+).+cause=0x([0-9a-fA-F]+)").unwrap();

    let mut trace = KronosTrace::default();
    let f = File::open(path.as_ref()).map_err(|e| LogParseError::LogFileReadError {
        path: path.as_ref().to_path_buf(),
        source: e,
    })?;
    let mut cycle: u64 = 0;
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| LogParseError::LogFileReadError {
            path: path.as_ref().to_path_buf(),
            source: e,
        })?;
        if let Some(c) = re_reg.captures(&line) {
            let pc = u64::from_str_radix(&c[1], 16).unwrap_or(0);
            let rd: u8 = c[2].parse().unwrap_or(255);
            let val = u64::from_str_radix(&c[3], 16).unwrap_or(0);
            trace
                .register_writes
                .entry(pc)
                .or_default()
                .push(RegisterWrite {
                    pc,
                    reg: format!("x{}", rd),
                    value: val,
                    cycle,
                });
            cycle += 1;
            continue;
        }
        if let Some(c) = re_mem.captures(&line) {
            let pc = u64::from_str_radix(&c[1], 16).unwrap_or(0);
            let base = u64::from_str_radix(&c[2], 16).unwrap_or(0);
            let data = u64::from_str_radix(&c[3], 16).unwrap_or(0);
            let mask = u64::from_str_radix(&c[4], 16).unwrap_or(0) as u8;
            // expand bytes according to mask bits (bit i -> addr+ i)
            let mut bytes = Vec::new();
            let mut width = 0usize;
            for i in 0..4 {
                if (mask >> i) & 1 == 1 {
                    let b = ((data >> (8 * i)) & 0xFF) as u8;
                    bytes.push(MemoryWriteByte {
                        addr: base + i as u64,
                        value: b,
                    });
                    width += 1;
                }
            }
            let mut value_u64 = 0u64;
            for (i, b) in bytes.iter().enumerate() {
                value_u64 |= (b.value as u64) << (8 * i.min(7));
            }
            trace
                .memory_writes
                .entry(pc)
                .or_default()
                .push(MemoryWrite {
                    pc,
                    addr: base,
                    value: value_u64,
                    width,
                    bytes,
                    cycle,
                });
            cycle += 1;
            continue;
        }
        if let Some(c) = re_trap.captures(&line) {
            let pc = u64::from_str_radix(&c[1], 16).unwrap_or(0);
            let cause = u64::from_str_radix(&c[2], 16).unwrap_or(0);
            trace.exceptions.insert(
                pc,
                ExceptionEvent {
                    pc,
                    mcause: cause,
                    cycle,
                },
            );
            cycle += 1;
            continue;
        }
    }
    Ok(trace)
}
