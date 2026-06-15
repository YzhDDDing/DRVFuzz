use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::info;
use serde::Deserialize;

use DRVFuzz::diff_analysis::{
    DiffAnalysisConfig, DiffAnalysisError, DiffAnalysisSettings, LocalizationTiming,
    localization_probe_get, localization_probe_reset, run_diff_analysis_with_testcase,
};
use DRVFuzz::riscv_impls::RiscVImpl;
use DRVFuzz::riscv_impls_vec::{GeneratedTestCase, RiscVImplVec};

#[derive(Debug, Deserialize)]
struct ImplTimeoutFile {
    #[serde(default)]
    impl_timeout_secs: HashMap<RiscVImpl, u64>,
}

#[derive(Debug)]
struct Descriptor {
    label: &'static str,
    testcase_path: PathBuf,
    kind: BugKind,
}

#[derive(Debug, Clone, Copy)]
enum BugKind {
    HistoryWrite,
    SingleWrite,
    Exception,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init()
        .ok();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let descriptors = build_descriptors(&manifest_dir);

    let timeout_path = manifest_dir.join("configs/timeout.toml");
    let impl_timeouts = load_impl_timeouts(&timeout_path)?;

    let mut times_ms = Vec::new();
    let mut instr_counts = Vec::new();

    let duration_secs = env::var("LOCALIZATION_DURATION_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok());

    if let Some(secs) = duration_secs {
        let timebox = Duration::from_secs(secs);
        println!(
            "Running bug localization in timeboxed mode for {} seconds...",
            secs
        );

        let start = Instant::now();
        let mut run_idx: usize = 0;

        loop {
            let now = Instant::now();
            if run_idx > 0 && now.duration_since(start) >= timebox {
                break;
            }

            let desc = &descriptors[run_idx % descriptors.len()];
            let testcase_path = &desc.testcase_path;
            println!("\n=== Run {}: {} ===", run_idx, desc.label);
            println!("Testcase: {}", testcase_path.display());

            let run_root = manifest_dir
                .join("temp/bug_localization_time")
                .join(format!("run_{:04}", run_idx));
            fs::create_dir_all(&run_root)?;

            match run_single_localization(testcase_path, &run_root, &impl_timeouts, desc.kind) {
                Ok((elapsed_ms, instrs)) => {
                    println!(
                        "Localization result: time = {:.2} ms, final test instructions = {}",
                        elapsed_ms, instrs
                    );
                    times_ms.push(elapsed_ms);
                    instr_counts.push(instrs as f64);
                }
                Err(err) => {
                    println!(
                        "Localization failed for {}: {}",
                        testcase_path.display(),
                        err
                    );
                }
            }

            run_idx += 1;
        }
    } else {
        for (index, desc) in descriptors.iter().enumerate() {
            let testcase_path = &desc.testcase_path;
            println!("\n=== Descriptor {}: {} ===", index, desc.label);
            println!("Testcase: {}", testcase_path.display());

            let run_root = manifest_dir
                .join("temp/bug_localization_time")
                .join(format!("descriptor_{:02}", index));
            fs::create_dir_all(&run_root)?;

            match run_single_localization(testcase_path, &run_root, &impl_timeouts, desc.kind) {
                Ok((elapsed_ms, instrs)) => {
                    println!(
                        "Localization result: time = {:.2} ms, final test instructions = {}",
                        elapsed_ms, instrs
                    );
                    times_ms.push(elapsed_ms);
                    instr_counts.push(instrs as f64);
                }
                Err(err) => {
                    println!(
                        "Localization failed for {}: {}",
                        testcase_path.display(),
                        err
                    );
                }
            }
        }
    }

    if times_ms.is_empty() {
        println!("\nNo successful localization runs; cannot compute averages.");
        return Ok(());
    }

    let n = times_ms.len() as f64;
    let avg_time_ms: f64 = times_ms.iter().sum::<f64>() / n;
    let avg_instrs: f64 = instr_counts.iter().sum::<f64>() / n;

    println!("\n--- Bug localization summary ---");
    println!("Descriptors:        {}", times_ms.len());
    println!("Avg localization time (ms): {:.2}", avg_time_ms);
    println!("Avg final instruction count:  {:.1}", avg_instrs);

    Ok(())
}

fn build_descriptors(manifest_dir: &Path) -> Vec<Descriptor> {
    vec![
        Descriptor {
            label: "picorv32 history-min write-diff",
            testcase_path: manifest_dir.join("bugs/picorv32/3/verify/testcase.json"),
            kind: BugKind::HistoryWrite,
        },
        Descriptor {
            label: "rv64 Spike-CVA6 single-write #1",
            testcase_path: manifest_dir.join(
                "temp/fuzz_rv64_spike_cva6/1763221063_674540544_rv64_4_w04/iter_000/testcase.json",
            ),
            kind: BugKind::SingleWrite,
        },
        Descriptor {
            label: "rv64 Spike-CVA6 single-write #2",
            testcase_path: PathBuf::from(
                "/home/canxin/Git/DRVFuzz/temp/fuzz_rv64_spike_cva6/1763221128_763313649_rv64_371_w81/iter_001/testcase.json",
            ),
            kind: BugKind::SingleWrite,
        },
        Descriptor {
            label: "rv32 Spike-Vex exception",
            testcase_path: PathBuf::from(
                "/home/canxin/Git/DRVFuzz/temp/fuzz_rv32_spike_vex/1763218880_939643348_rv32_0_w01/iter_000/testcase.json",
            ),
            kind: BugKind::Exception,
        },
    ]
}

fn load_impl_timeouts(
    path: &Path,
) -> Result<HashMap<RiscVImpl, std::time::Duration>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let parsed: ImplTimeoutFile = toml::from_str(&content)?;
    Ok(parsed
        .impl_timeout_secs
        .into_iter()
        .map(|(impl_ref, secs)| (impl_ref, std::time::Duration::from_secs(secs)))
        .collect())
}

fn run_single_localization(
    testcase_path: &Path,
    run_root: &Path,
    impl_timeouts: &HashMap<RiscVImpl, std::time::Duration>,
    kind: BugKind,
) -> Result<(f64, u64), Box<dyn Error>> {
    let file = fs::File::open(testcase_path)?;
    let testcase: GeneratedTestCase = serde_json::from_reader(file)?;

    let impls: Vec<RiscVImpl> = testcase.init_insts.keys().cloned().collect();
    if impls.is_empty() {
        return Err("testcase has no implementations".into());
    }

    let riscv_impls = RiscVImplVec::from_impls(impls.clone());

    let settings = DiffAnalysisSettings {
        testcase_config: testcase.config.clone(),
        max_iterations: None,
        history_min_test_threshold: None,
        impl_timeouts: impl_timeouts.clone(),
        emit_execution_output_json: false,
        emit_execution_report_md: false,
        cleanup_successful_iteration_artifacts: false,
        cleanup_successful_diff_run: false,
        guidance_strategy: None,
        transition_seed_pool_limit: 64,
        transition_seed_window: 16,
    };

    fs::create_dir_all(run_root)?;
    let config = DiffAnalysisConfig {
        testcase_config: settings.testcase_config.clone(),
        run_root: run_root.to_path_buf(),
        max_iterations: settings.max_iterations,
        history_min_test_threshold: settings.history_min_test_threshold,
        impl_timeouts: settings.impl_timeouts.clone(),
        emit_execution_output_json: settings.emit_execution_output_json,
        emit_execution_report_md: settings.emit_execution_report_md,
        cleanup_successful_iteration_artifacts: settings.cleanup_successful_iteration_artifacts,
        cleanup_successful_diff_run: settings.cleanup_successful_diff_run,
        guidance_strategy: settings.guidance_strategy,
        transition_seed_pool_limit: settings.transition_seed_pool_limit,
        transition_seed_window: settings.transition_seed_window,
    };

    let filtered_impls = riscv_impls.filter_by_unaligned_access_requirement(
        config.testcase_config.unaligned_access_required,
        &HashMap::new(),
    );

    if filtered_impls.iter().next().is_none() {
        return Err("no implementations satisfy unaligned_access requirement".into());
    }

    info!(
        "Starting diff-analysis localization for {} in {}",
        testcase_path.display(),
        run_root.display()
    );

    let start = Instant::now();
    localization_probe_reset(start);
    let result = run_diff_analysis_with_testcase(&filtered_impls, config, testcase)
        .map_err(|err| format_diff_error(err, testcase_path))?;
    let elapsed = start.elapsed();

    let timing: Option<LocalizationTiming> = localization_probe_get();
    let localization_ms = timing.and_then(|t| match kind {
        BugKind::HistoryWrite | BugKind::SingleWrite => t.first_write_ms,
        BugKind::Exception => t.first_exception_ms,
    });

    let primary_impl = filtered_impls
        .iter()
        .copied()
        .find(|impl_ref| *impl_ref != RiscVImpl::Spike)
        .unwrap_or_else(|| *filtered_impls.iter().next().expect("non-empty impl set"));

    let final_block = result
        .final_testcase
        .test_insts
        .get(&primary_impl)
        .or_else(|| result.final_testcase.test_insts.values().next())
        .ok_or_else(|| "final testcase has no test instruction blocks".to_string())?;

    let instr_count = final_block.len() as u64;
    let total_ms = elapsed.as_secs_f64() * 1e3;
    let used_ms = localization_ms.unwrap_or(total_ms);
    Ok((used_ms, instr_count))
}

fn format_diff_error(err: DiffAnalysisError, testcase_path: &Path) -> String {
    format!(
        "diff analysis failed for {}: {}",
        testcase_path.display(),
        err
    )
}
