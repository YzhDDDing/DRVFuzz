use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use log::{debug, info};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::build_elf::{ElfBuildResult, build_elf_with_extensions};
use crate::error::{ElfError, ParseError, ProcessError, XiangShanError};
use crate::exception_cause::format_cause_code;
use crate::execution_output::{
    ExceptionInfo as TraitExceptionInfo, ExecutionOutput, MemValue as TraitMemValue,
    RegisterValue as TraitRegisterValue,
};
use crate::extension_map::ExtensionMap;
use crate::isa_base::ISABase;
use crate::riscv_impls::RiscVImpl;
use crate::tracer::Tracer;
use crate::user_instruction::UserInstructionInfo;

const XIANGSHAN_EMU_ENV: &str = "RISCV_WRAPPER_XS_EMU";
const XIANGSHAN_EMU_NO_UNALIGNED_ENV: &str = "RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW";
const XIANGSHAN_DIFF_ENV: &str = "RISCV_WRAPPER_XS_DIFF_SO";

#[derive(Debug, Clone, Serialize)]
pub struct RegisterWrite {
    pub pc: u64,
    pub register: String,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryWrite {
    pub pc: u64,
    pub addr: u64,
    pub bytes: Vec<(u64, u8)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExceptionEvent {
    pub pc: u64,
    pub cause: u64,
}

#[derive(Debug, Clone, Default)]
pub struct XiangShanTrace {
    pub registers: HashMap<u64, Vec<RegisterWrite>>,
    pub memory: HashMap<u64, Vec<MemoryWrite>>,
    pub exceptions: HashMap<u64, ExceptionEvent>,
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub work_dir: PathBuf,
    pub isa_base: ISABase,
    pub riscv_impl: RiscVImpl,
    pub timeout: Option<Duration>,
    pub extension_override: Option<ExtensionMap>,
    pub emulator_path: Option<PathBuf>,
    pub diff_so_path: Option<PathBuf>,
    pub extra_emu_args: Vec<String>,
    pub allow_unaligned: bool,
}

impl ExecutorConfig {
    pub fn new(work_dir: PathBuf, isa_base: ISABase, riscv_impl: RiscVImpl) -> Self {
        Self {
            work_dir,
            isa_base,
            riscv_impl,
            timeout: None,
            extension_override: None,
            emulator_path: None,
            diff_so_path: None,
            extra_emu_args: Vec::new(),
            allow_unaligned: true,
        }
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

    fn emulator_path(&self) -> Result<PathBuf, XiangShanError> {
        if let Some(path) = &self.emulator_path {
            return Ok(path.clone());
        }
        let env_name = if self.allow_unaligned {
            XIANGSHAN_EMU_ENV
        } else {
            XIANGSHAN_EMU_NO_UNALIGNED_ENV
        };
        let path = env::var(env_name).map_err(|_| XiangShanError::EnvVarNotSet {
            var: env_name.to_string(),
        })?;
        Ok(PathBuf::from(path))
    }

    fn diff_so_path(&self) -> Result<PathBuf, XiangShanError> {
        if let Some(path) = &self.diff_so_path {
            return Ok(path.clone());
        }
        let path = env::var(XIANGSHAN_DIFF_ENV).map_err(|_| XiangShanError::EnvVarNotSet {
            var: XIANGSHAN_DIFF_ENV.to_string(),
        })?;
        Ok(PathBuf::from(path))
    }
}

#[derive(Debug)]
pub struct XiangShanExecutor {
    config: ExecutorConfig,
}

impl XiangShanExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    pub fn execute<P: AsRef<Path>>(
        &self,
        run_root: P,
        user_insts: &[String],
    ) -> Result<ExecutionOutput, XiangShanError> {
        if self.config.isa_base != ISABase::Rv64 {
            return Err(XiangShanError::Config(
                crate::error::ConfigError::UnsupportedIsaBase {
                    impl_name: "XiangShan".to_string(),
                    isa_base: format!("{:?}", self.config.isa_base),
                },
            ));
        }

        let run_dir = run_root.as_ref();
        fs::create_dir_all(run_dir).map_err(|e| {
            XiangShanError::ProcessError(ProcessError::DirectoryCreationFailed {
                path: run_dir.to_path_buf(),
                source: e,
            })
        })?;

        let extension_map = self.config.extension_map();
        let program = self
            .config
            .riscv_impl
            .build_asm_content(user_insts, self.config.isa_base)?;
        let asm_path = run_dir.join("program.S");
        fs::write(&asm_path, &program).map_err(|e| {
            XiangShanError::ProcessError(ProcessError::FileWriteFailed {
                path: asm_path.clone(),
                source: e,
            })
        })?;

        let build = build_elf_with_extensions(
            &asm_path,
            &extension_map,
            &self.config.isa_base,
            &self.config.riscv_impl,
        )?;

        let run_result = self.run_emu(run_dir, &build)?;

        let trace = parse_xs_commit_trace(&run_result.commit_log_path)?;
        if trace.registers.is_empty() && trace.memory.is_empty() && trace.exceptions.is_empty() {
            return Err(XiangShanError::EmptyTrace);
        }

        let tracer = Tracer::new(user_insts, &build.disassembly_file).map_err(|e| {
            XiangShanError::ElfError(ElfError::DumpLoadFailed {
                path: build.disassembly_file.clone(),
                source: e,
            })
        })?;
        let user_instruction_info = UserInstructionInfo::build(user_insts);
        let user_pc_map = build_user_pc_map(&tracer, user_instruction_info.len())?;

        Ok(build_execution_output(
            trace,
            user_instruction_info,
            user_pc_map,
            self.config.riscv_impl,
            self.config.isa_base,
        ))
    }

    fn run_emu(
        &self,
        run_dir: &Path,
        build: &ElfBuildResult,
    ) -> Result<XiangShanRunResult, XiangShanError> {
        let emu_path_raw = self.config.emulator_path()?;
        let emu_path = emu_path_raw
            .canonicalize()
            .unwrap_or_else(|_| emu_path_raw.clone());
        if !emu_path.exists() {
            return Err(XiangShanError::BinaryNotFound {
                path: emu_path.display().to_string(),
            });
        }

        let diff_so_raw = self.config.diff_so_path()?;
        let diff_so = diff_so_raw
            .canonicalize()
            .unwrap_or_else(|_| diff_so_raw.clone());
        if !diff_so.exists() {
            return Err(XiangShanError::DiffSoNotFound {
                path: diff_so.display().to_string(),
            });
        }

        let commit_log_path = run_dir.join("skiptrap.commit.log");

        let mut cmd = Command::new(&emu_path);
        cmd.current_dir(run_dir);
        cmd.arg("-i").arg(&build.executable_file);
        cmd.arg("--diff").arg(&diff_so);
        cmd.arg("--dump-commit-trace");
        for arg in &self.config.extra_emu_args {
            cmd.arg(arg);
        }
        // Redirect emulator stdout/stderr directly to a file to avoid pipe backpressure hangs
        let log_file = fs::File::create(&commit_log_path).map_err(|e| {
            XiangShanError::ProcessError(ProcessError::FileWriteFailed {
                path: commit_log_path.clone(),
                source: e,
            })
        })?;
        let log_file_err = log_file.try_clone().map_err(|e| {
            XiangShanError::ProcessError(ProcessError::FileWriteFailed {
                path: commit_log_path.clone(),
                source: e,
            })
        })?;
        cmd.stdout(Stdio::from(log_file));
        cmd.stderr(Stdio::from(log_file_err));

        let mut command_parts = Vec::new();
        command_parts.push(quote_os_str(emu_path.as_os_str()));
        command_parts.extend(cmd.get_args().map(|arg| quote_os_str(arg)));

        let command_display = format!(
            "cd \"{}\" && {}",
            run_dir.display(),
            command_parts.join(" ")
        );
        info!("Launching XiangShan command: {}", command_display);

        let mut child = cmd.spawn().map_err(|err| {
            XiangShanError::ProcessError(ProcessError::SpawnFailed {
                command: command_display.clone(),
                source: err,
            })
        })?;

        let timeout = self.config.timeout;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| {
                XiangShanError::ProcessError(ProcessError::SpawnFailed {
                    command: command_display.clone(),
                    source: e,
                })
            })? {
                break status;
            }
            if let Some(limit) = timeout {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    return Err(XiangShanError::ProcessError(ProcessError::TimedOut {
                        command: command_display.clone(),
                        timeout: limit,
                    }));
                }
            }
            // avoid busy spin
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        // Child's output has been streamed into commit_log_path directly
        let _ = status; // reserved for future checks

        Ok(XiangShanRunResult {
            build: build.clone(),
            commit_log_path,
        })
    }
}

#[derive(Debug)]
pub struct XiangShanRunResult {
    #[allow(dead_code)]
    pub build: ElfBuildResult,
    pub commit_log_path: PathBuf,
}

fn build_user_pc_map(
    tracer: &Tracer,
    user_inst_count: usize,
) -> Result<Vec<Vec<u64>>, XiangShanError> {
    let mut result = Vec::with_capacity(user_inst_count);
    for idx in 0..user_inst_count {
        let pcs = tracer
            .get_all_pcs_for_user_inst(idx)
            .ok_or(XiangShanError::NoPcMapping { index: idx })?;
        if pcs.is_empty() {
            return Err(XiangShanError::NoPcMapping { index: idx });
        }
        result.push(pcs.to_vec());
    }
    Ok(result)
}

fn build_execution_output(
    trace: XiangShanTrace,
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
            if let Some(writes) = trace.registers.get(&pc) {
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

            if let Some(mem_writes) = trace.memory.get(&pc) {
                for write in mem_writes {
                    for (addr, byte) in &write.bytes {
                        mem_map
                            .entry(*addr)
                            .and_modify(|entry| {
                                if pc > entry.0 {
                                    *entry = (pc, *byte);
                                }
                            })
                            .or_insert((pc, *byte));
                    }
                }
            }

            if let Some(event) = trace.exceptions.get(&pc) {
                exception_opt = match exception_opt {
                    Some((existing_pc, _)) if pc <= existing_pc => exception_opt,
                    _ => Some((pc, event)),
                };
            }
        }

        let mut regs: Vec<_> = reg_map
            .into_iter()
            .map(|(name, (_pc, value))| TraitRegisterValue { name, value })
            .collect();
        regs.sort_by(|a, b| a.name.cmp(&b.name));
        register_writes.push(regs);

        let mut mem: Vec<_> = mem_map
            .into_iter()
            .map(|(addr, (_pc, value))| TraitMemValue { addr, value })
            .collect();
        mem.sort_by_key(|m| m.addr);
        memory_writes.push(mem);

        if let Some((_pc, event)) = exception_opt {
            exceptions.push(TraitExceptionInfo {
                user_instruction_index: user_idx,
                cause: format_cause_code(event.cause),
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

fn parse_xs_commit_trace<P: AsRef<Path>>(path: P) -> Result<XiangShanTrace, XiangShanError> {
    let file = fs::File::open(path.as_ref()).map_err(|e| {
        XiangShanError::ProcessError(ProcessError::LogFileOpenFailed {
            path: path.as_ref().to_path_buf(),
            source: e,
        })
    })?;
    let reader = BufReader::new(file);
    parse_xs_commit_trace_reader(reader)
}

fn parse_xs_commit_trace_reader<R: BufRead>(reader: R) -> Result<XiangShanTrace, XiangShanError> {
    static RE_COMMIT: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\[(?P<idx>\d+)\] commit pc (?P<pc>[0-9a-fA-F]{16}) inst (?P<inst>[0-9a-fA-F]{8}) wen (?P<wen>\d+) dst (?P<dstprefix>[xXfFvV])(?P<dst>\d{1,2}) data (?P<data>[0-9a-fA-F]{16}) idx (?P<rob>[0-9a-fA-F]{3})(?P<rest>.*)$").unwrap()
    });
    static RE_EXCEPTION: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^\[(?P<idx>\d+)\] exception pc (?P<pc>[0-9a-fA-F]{16}) inst (?P<inst>[0-9a-fA-F]{8}) cause (?P<cause>[0-9a-fA-F]{16})(?P<rest>.*)$").unwrap()
    });
    static RE_MEM_LINE: Lazy<Regex> = Lazy::new(|| {
        // log-only mode standalone mem line: "mem pc <pc> addr <addr> data <data> mask 0x.."
        Regex::new(r"^mem pc (?P<pc>[0-9a-fA-F]{16}) addr (?P<addr>[0-9a-fA-F]{16}) data (?P<data>[0-9a-fA-F]{16}) mask 0x(?P<mask>[0-9a-fA-F]{2})$").unwrap()
    });
    static RE_INLINE_MEM_IN_COMMIT: Lazy<Regex> = Lazy::new(|| {
        // Inline memory fragment on commit line: "... addr <addr> data <data> mask 0x.. <asm>"
        Regex::new(
            r"addr (?P<addr>[0-9a-fA-F]{16}) data (?P<data>[0-9a-fA-F]{16}) mask 0x(?P<mask>[0-9a-fA-F]{2})",
        )
        .unwrap()
    });

    let mut trace = XiangShanTrace::default();

    for line in reader.lines() {
        let line = line.map_err(|e| XiangShanError::IoError(e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(cap) = RE_COMMIT.captures(trimmed) {
            let pc = u64::from_str_radix(cap.name("pc").unwrap().as_str(), 16).map_err(|e| {
                XiangShanError::ParseError(ParseError::HexParseError {
                    value: cap.name("pc").unwrap().as_str().to_string(),
                    source: e,
                })
            })?;
            let wen = cap.name("wen").unwrap().as_str() == "1";
            let rest = cap.name("rest").map(|m| m.as_str()).unwrap_or("");
            if wen {
                let dst_str = cap.name("dst").unwrap().as_str();
                // Normalize register index to remove any leading zeros (e.g., x03 -> x3)
                let dst_num: u32 = dst_str.parse().unwrap_or(0);
                let data =
                    u64::from_str_radix(cap.name("data").unwrap().as_str(), 16).map_err(|e| {
                        XiangShanError::ParseError(ParseError::HexParseError {
                            value: cap.name("data").unwrap().as_str().to_string(),
                            source: e,
                        })
                    })?;

                let register = match cap.name("dstprefix").unwrap().as_str() {
                    "f" | "F" => format!("f{}", dst_num),
                    "v" | "V" => format!("v{}", dst_num),
                    _ => format!("x{}", dst_num),
                };
                trace
                    .registers
                    .entry(pc)
                    .or_insert_with(Vec::new)
                    .push(RegisterWrite {
                        pc,
                        register,
                        value: data,
                    });
            }

            // Parse inline memory write fragments on commit lines (newer XiangShan logs).
            // Older logs also emit standalone "mem pc ..." lines; we deduplicate here.
            if let Some(mem_cap) = RE_INLINE_MEM_IN_COMMIT.captures(rest) {
                let addr = u64::from_str_radix(mem_cap.name("addr").unwrap().as_str(), 16)
                    .map_err(|e| {
                        XiangShanError::ParseError(ParseError::HexParseError {
                            value: mem_cap.name("addr").unwrap().as_str().to_string(),
                            source: e,
                        })
                    })?;
                let data = u64::from_str_radix(mem_cap.name("data").unwrap().as_str(), 16)
                    .map_err(|e| {
                        XiangShanError::ParseError(ParseError::HexParseError {
                            value: mem_cap.name("data").unwrap().as_str().to_string(),
                            source: e,
                        })
                    })?;
                let mask = u8::from_str_radix(mem_cap.name("mask").unwrap().as_str(), 16).map_err(
                    |e| {
                        XiangShanError::ParseError(ParseError::HexParseError {
                            value: mem_cap.name("mask").unwrap().as_str().to_string(),
                            source: e,
                        })
                    },
                )?;

                let bytes = expand_store_bytes(addr, data, mask);
                let entry = trace.memory.entry(pc).or_insert_with(Vec::new);
                let duplicate = entry
                    .iter()
                    .any(|w| w.addr == addr && w.bytes.len() == bytes.len() && w.bytes == bytes);
                if !duplicate {
                    entry.push(MemoryWrite { pc, addr, bytes });
                }
            }

            continue;
        }

        if let Some(cap) = RE_EXCEPTION.captures(trimmed) {
            let pc = u64::from_str_radix(cap.name("pc").unwrap().as_str(), 16).map_err(|e| {
                XiangShanError::ParseError(ParseError::HexParseError {
                    value: cap.name("pc").unwrap().as_str().to_string(),
                    source: e,
                })
            })?;
            let cause =
                u64::from_str_radix(cap.name("cause").unwrap().as_str(), 16).map_err(|e| {
                    XiangShanError::ParseError(ParseError::HexParseError {
                        value: cap.name("cause").unwrap().as_str().to_string(),
                        source: e,
                    })
                })?;
            trace.exceptions.insert(pc, ExceptionEvent { pc, cause });
            continue;
        }

        if let Some(cap) = RE_MEM_LINE.captures(trimmed) {
            let pc = u64::from_str_radix(cap.name("pc").unwrap().as_str(), 16).map_err(|e| {
                XiangShanError::ParseError(ParseError::HexParseError {
                    value: cap.name("pc").unwrap().as_str().to_string(),
                    source: e,
                })
            })?;
            let addr =
                u64::from_str_radix(cap.name("addr").unwrap().as_str(), 16).map_err(|e| {
                    XiangShanError::ParseError(ParseError::HexParseError {
                        value: cap.name("addr").unwrap().as_str().to_string(),
                        source: e,
                    })
                })?;
            let data =
                u64::from_str_radix(cap.name("data").unwrap().as_str(), 16).map_err(|e| {
                    XiangShanError::ParseError(ParseError::HexParseError {
                        value: cap.name("data").unwrap().as_str().to_string(),
                        source: e,
                    })
                })?;
            let mask = u8::from_str_radix(cap.name("mask").unwrap().as_str(), 16).map_err(|e| {
                XiangShanError::ParseError(ParseError::HexParseError {
                    value: cap.name("mask").unwrap().as_str().to_string(),
                    source: e,
                })
            })?;

            let bytes = expand_store_bytes(addr, data, mask);
            trace
                .memory
                .entry(pc)
                .or_insert_with(Vec::new)
                .push(MemoryWrite { pc, addr, bytes });
            continue;
        }

        debug!("Unrecognized XiangShan commit trace line: {}", trimmed);
    }

    Ok(trace)
}

fn expand_store_bytes(addr: u64, data: u64, mask: u8) -> Vec<(u64, u8)> {
    let mut result = Vec::new();
    for i in 0..8 {
        if (mask & (1 << i)) != 0 {
            let byte = ((data >> (i * 8)) & 0xFF) as u8;
            result.push((addr + i as u64, byte));
        }
    }
    result
}

fn quote_os_str(arg: &OsStr) -> String {
    let value = arg.to_string_lossy();
    if value.is_empty() {
        "\"\"".to_string()
    } else if value.chars().all(|c| !c.is_whitespace() && c != '"') {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_commit_with_inline_mem_only() {
        let log = "[571] commit pc 0000000080000792 inst 01151423 wen 0 dst x08 data 0000000000000000 idx 02b (0c) addr 000000008fffffe0 data 0000000045000000 mask 0x08 sh      a7, 8(a0)\n";
        let trace = parse_xs_commit_trace_reader(log.as_bytes()).expect("parse failed");

        let pc = 0x0000_0000_8000_0792u64;
        let mem = trace.memory.get(&pc).expect("memory entry missing");
        assert_eq!(mem.len(), 1);
        assert_eq!(mem[0].addr, 0x0000_0000_8fff_ffe0u64);
        assert_eq!(mem[0].bytes.len(), 1);
        assert_eq!(mem[0].bytes[0].0, 0x0000_0000_8fff_ffe3u64);
        assert_eq!(mem[0].bytes[0].1, 0x45u8);
    }

    #[test]
    fn parse_commit_inline_mem_deduplicates_with_mem_line() {
        let log = "mem pc 0000000080000792 addr 000000008fffffe0 data 0000000045000000 mask 0x08\n[571] commit pc 0000000080000792 inst 01151423 wen 0 dst x08 data 0000000000000000 idx 02b (0c) addr 000000008fffffe0 data 0000000045000000 mask 0x08 sh      a7, 8(a0)\n";
        let trace = parse_xs_commit_trace_reader(log.as_bytes()).expect("parse failed");

        let pc = 0x0000_0000_8000_0792u64;
        let mem = trace.memory.get(&pc).expect("memory entry missing");
        // Only one memory write entry for this PC even though it appears twice in the log.
        assert_eq!(mem.len(), 1);
        assert_eq!(mem[0].addr, 0x0000_0000_8fff_ffe0u64);
    }
}
