use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use log::{info, warn};
use serde::Deserialize;

use DRVFuzz::build_elf::build_elf_with_extensions;
use DRVFuzz::isa_base::ISABase;
use DRVFuzz::riscv_impls::RiscVImpl;
use DRVFuzz::riscv_impls_vec::{RiscVImplVec, TestCaseConfig};

#[derive(Debug, Deserialize)]
struct TestCaseFileConfig {
    testcase_config: TestCaseConfig,
}

#[derive(Debug, Default, Deserialize)]
struct ImplTimeoutFile {
    #[serde(default)]
    impl_timeout_secs: HashMap<RiscVImpl, u64>,
}

struct SampleAccumulator {
    count: u64,
    total_gen: Duration,
    total_spike: Duration,
    total_elf: Duration,
    total_rtl: Duration,
    total_instrs: u64,
}

impl SampleAccumulator {
    fn new() -> Self {
        Self {
            count: 0,
            total_gen: Duration::from_secs(0),
            total_spike: Duration::from_secs(0),
            total_elf: Duration::from_secs(0),
            total_rtl: Duration::from_secs(0),
            total_instrs: 0,
        }
    }

    fn add(
        &mut self,
        instr_count: u64,
        gen_time: Duration,
        spike: Duration,
        elf: Duration,
        rtl: Duration,
    ) {
        self.count += 1;
        self.total_instrs += instr_count;
        self.total_gen += gen_time;
        self.total_spike += spike;
        self.total_elf += elf;
        self.total_rtl += rtl;
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init()
        .ok();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let testcase_config_path = manifest_dir.join("configs/rv64.toml");
    let timeout_path = manifest_dir.join("configs/timeout.toml");
    let run_root = manifest_dir.join("temp/cva6_spike_overhead_timeboxed");

    let testcase_config = load_testcase_only_config(&testcase_config_path)?;
    if testcase_config.isa_base != ISABase::Rv64 {
        warn!(
            "Expected RV64 ISA base but testcase config uses {}",
            testcase_config.isa_base.to_str()
        );
    }

    let impl_timeouts = load_impl_timeouts(&timeout_path)?;
    let spike_timeout = impl_timeouts.get(&RiscVImpl::Spike).copied();
    let cva6_timeout = impl_timeouts.get(&RiscVImpl::CVA6).copied();

    let cva6_bin = locate_cva6_rv64_binary()?;
    fs::create_dir_all(&run_root)?;

    let riscv_impls = RiscVImplVec::from_impls(vec![RiscVImpl::Spike, RiscVImpl::CVA6]);

    let default_secs: u64 = 30 * 60;
    let timebox_secs = std::env::var("TIMEBOX_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default_secs);
    let timebox = Duration::from_secs(timebox_secs);

    info!(
        "Starting CVA6/Spike RV64 overhead timeboxed run for up to {} seconds",
        timebox_secs
    );

    let mut acc = SampleAccumulator::new();
    let global_start = Instant::now();
    let mut sample_index: u64 = 0;

    while global_start.elapsed() < timebox {
        sample_index += 1;
        let sample_dir = run_root.join(format!("sample_{:06}", sample_index));
        let spike_dir = sample_dir.join("Spike");
        let cva6_dir = sample_dir.join("CVA6");
        fs::create_dir_all(&spike_dir)?;
        fs::create_dir_all(&cva6_dir)?;

        // 1) Generate basic blocks (random testcase)
        let gen_start = Instant::now();
        let testcase = match riscv_impls.generate_random_testcase(testcase_config.clone()) {
            Ok(tc) => tc,
            Err(err) => {
                warn!(
                    "Skipping sample {}: testcase generation failed: {}",
                    sample_index, err
                );
                continue;
            }
        };
        let gen_time = gen_start.elapsed();

        let spike_insts = match testcase.combined_insts_of(&RiscVImpl::Spike) {
            Some(v) if !v.is_empty() => v,
            _ => {
                warn!(
                    "Skipping sample {}: missing Spike instructions in generated testcase",
                    sample_index
                );
                continue;
            }
        };

        let cva6_insts = match testcase.combined_insts_of(&RiscVImpl::CVA6) {
            Some(v) if !v.is_empty() => v,
            _ => {
                warn!(
                    "Skipping sample {}: missing CVA6 instructions in generated testcase",
                    sample_index
                );
                continue;
            }
        };

        let cva6_test_len = match testcase.test_insts.get(&RiscVImpl::CVA6) {
            Some(block) if !block.lines().is_empty() => block.len() as u64,
            _ => {
                warn!(
                    "Skipping sample {}: missing CVA6 test instruction block",
                    sample_index
                );
                continue;
            }
        };

        // 2) Spike execution (golden reference)
        let spike_start = Instant::now();
        let spike_result = RiscVImpl::Spike.execute(
            &spike_dir,
            ISABase::Rv64,
            &spike_insts,
            spike_timeout,
            testcase.config.unaligned_access_required.unwrap_or(false),
        );
        let spike_time = spike_start.elapsed();
        if let Err(err) = spike_result {
            warn!(
                "Skipping sample {}: Spike execution failed: {}",
                sample_index, err
            );
            continue;
        }

        // 3) Build CVA6 ELF
        let cva6_program = match RiscVImpl::CVA6.build_asm_content(&cva6_insts, ISABase::Rv64) {
            Ok(p) => p,
            Err(err) => {
                warn!(
                    "Skipping sample {}: failed to build CVA6 assembly: {}",
                    sample_index, err
                );
                continue;
            }
        };
        let asm_path = cva6_dir.join("program.S");
        if let Err(err) = fs::write(&asm_path, cva6_program) {
            warn!(
                "Skipping sample {}: failed to write CVA6 assembly {}: {}",
                sample_index,
                asm_path.display(),
                err
            );
            continue;
        }

        let elf_start = Instant::now();
        let build = match build_elf_with_extensions(
            &asm_path,
            &testcase.extension_map,
            &ISABase::Rv64,
            &RiscVImpl::CVA6,
        ) {
            Ok(b) => b,
            Err(err) => {
                warn!(
                    "Skipping sample {}: CVA6 ELF build failed for {}: {}",
                    sample_index,
                    asm_path.display(),
                    err
                );
                continue;
            }
        };
        let elf_time = elf_start.elapsed();

        // 4) CVA6 RTL simulation
        let rtl_start = Instant::now();
        if let Err(err) = run_cva6_elf(
            &cva6_bin,
            build.executable_file.as_path(),
            &cva6_dir,
            cva6_timeout,
        ) {
            warn!(
                "Skipping sample {}: CVA6 RTL simulation failed: {}",
                sample_index, err
            );
            continue;
        }
        let rtl_time = rtl_start.elapsed();

        acc.add(cva6_test_len, gen_time, spike_time, elf_time, rtl_time);

        if sample_index % 10 == 0 {
            info!(
                "Collected {} successful samples so far; elapsed {:.1}s",
                acc.count,
                global_start.elapsed().as_secs_f64()
            );
        }
    }

    if acc.count == 0 {
        println!(
            "No successful samples collected within {} seconds; cannot compute overhead.",
            timebox_secs
        );
        return Ok(());
    }

    let n = acc.count as f64;
    let avg_gen_ms = acc.total_gen.as_secs_f64() * 1000.0 / n;
    let avg_spike_ms = acc.total_spike.as_secs_f64() * 1000.0 / n;
    let avg_elf_ms = acc.total_elf.as_secs_f64() * 1000.0 / n;
    let avg_rtl_ms = acc.total_rtl.as_secs_f64() * 1000.0 / n;

    let avg_instrs = acc.total_instrs as f64 / n;

    let avg_analysis_ms = avg_gen_ms + avg_spike_ms + avg_elf_ms;
    let overhead_pct = if avg_rtl_ms > 0.0 {
        avg_analysis_ms / avg_rtl_ms * 100.0
    } else {
        0.0
    };

    println!("CVA6 vs Spike RV64 overhead timeboxed run");
    println!("Timebox (s): {}", timebox_secs);
    println!("Successful samples: {}", acc.count);
    println!("Avg basic-block generation (ms): {:.3}", avg_gen_ms);
    println!("Avg Spike execution (ms): {:.3}", avg_spike_ms);
    println!("Avg CVA6 ELF build (ms): {:.3}", avg_elf_ms);
    println!("Avg CVA6 RTL simulation (ms): {:.3}", avg_rtl_ms);
    println!("Avg CVA6 test instruction count: {:.1}", avg_instrs);
    println!("Avg analysis process (ms): {:.3}", avg_analysis_ms);
    println!(
        "Overhead (analysis / execution * 100): {:.2}%",
        overhead_pct
    );
    println!("Run root: {}", run_root.display());

    Ok(())
}

fn load_testcase_only_config(path: &Path) -> Result<TestCaseConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let parsed: TestCaseFileConfig = toml::from_str(&content)?;
    Ok(parsed.testcase_config)
}

fn load_impl_timeouts(path: &Path) -> Result<HashMap<RiscVImpl, Duration>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let parsed: ImplTimeoutFile = toml::from_str(&content)?;
    Ok(parsed
        .impl_timeout_secs
        .into_iter()
        .map(|(impl_ref, secs)| (impl_ref, Duration::from_secs(secs)))
        .collect())
}
fn is_command_available(path_or_command: &Path) -> bool {
    if path_or_command.components().count() > 1 || path_or_command.is_absolute() {
        return path_or_command.exists();
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join(path_or_command);
            if candidate.exists() {
                return true;
            }
        }
    }

    false
}

fn locate_cva6_rv64_binary() -> Result<PathBuf, Box<dyn Error>> {
    const CVA6_RV64_BINARY_ENV: &str = "RISCV_WRAPPER_CVA6_RV64_BIN";
    const CVA6_BINARY_ENV: &str = "RISCV_WRAPPER_CVA6_BIN";

    let value = std::env::var(CVA6_RV64_BINARY_ENV)
        .or_else(|_| std::env::var(CVA6_BINARY_ENV))
        .map_err(|_| {
            format!(
                "environment variables {} or {} are not set; please point one of them to the CVA6 RV64 binary",
                CVA6_RV64_BINARY_ENV, CVA6_BINARY_ENV
            )
        })?;

    let path = PathBuf::from(value);
    if !is_command_available(&path) {
        return Err(format!(
            "CVA6 RV64 binary not found or not executable: {}",
            path.display()
        )
        .into());
    }

    Ok(path)
}

fn run_cva6_elf(
    cva6_bin: &Path,
    elf_path: &Path,
    run_dir: &Path,
    timeout: Option<Duration>,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(run_dir)?;

    let riscv_root = std::env::var("RISCV")
        .map_err(|_| "environment variable RISCV is not set; please set RISCV toolchain root")?;
    let nm_path = Path::new(&riscv_root)
        .join("bin")
        .join("riscv64-unknown-elf-nm");
    if !is_command_available(&nm_path) {
        return Err(format!(
            "RISCV nm executable not found at {} (not found as file or in PATH)",
            nm_path.display()
        )
        .into());
    }

    let nm_output = Command::new(&nm_path).arg("-B").arg(elf_path).output()?;
    if !nm_output.status.success() {
        return Err(format!(
            "{} failed to locate tohost symbol: {}",
            nm_path.display(),
            String::from_utf8_lossy(&nm_output.stderr)
        )
        .into());
    }

    let nm_stdout = String::from_utf8(nm_output.stdout)?;
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
        .ok_or_else(|| "failed to locate tohost symbol in ELF")?;

    let mut cmd = Command::new(cva6_bin);
    cmd.arg(elf_path)
        .arg(format!("+elf_file={}", elf_path.display()))
        .arg(format!("+tohost_addr={}", tohost_addr))
        .arg(format!("+trace_log_dir={}", run_dir.display()))
        .arg("+debug_disable");
    cmd.current_dir(run_dir);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let start = Instant::now();

    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(format!("CVA6 exited with status {}", status).into());
            }
            break;
        }

        if let Some(limit) = timeout {
            if start.elapsed() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("CVA6 timed out after {:?}", limit).into());
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(())
}
