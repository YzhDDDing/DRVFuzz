use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use toml::Value as TomlValue;

use DRVFuzz::diff_analysis::{DiffAnalysisConfig, DiffAnalysisSettings, run_diff_analysis};
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

#[derive(Debug, Default, Deserialize)]
struct RunOptionsFile {
    #[serde(default)]
    max_iterations: Option<usize>,
    #[serde(default)]
    history_min_test_threshold: Option<usize>,
    #[serde(default)]
    emit_execution_output_json: bool,
    #[serde(default)]
    emit_execution_report_md: bool,
    #[serde(default)]
    cleanup_successful_iteration_artifacts: bool,
    #[serde(default)]
    cleanup_successful_diff_run: bool,
}

#[derive(Debug, Clone, Copy)]
struct RunOptions {
    max_iterations: Option<usize>,
    history_min_test_threshold: Option<usize>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
    cleanup_successful_iteration_artifacts: bool,
    cleanup_successful_diff_run: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init()
        .ok();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let testcase_config_path = manifest_dir.join("configs/rv64.toml");
    let timeout_path = manifest_dir.join("configs/timeout.toml");
    let run_options_path = manifest_dir.join("configs/run_options.toml");
    let run_root = manifest_dir.join("temp/rocket_spike_rv64_time_stats");

    let testcase_config = load_testcase_only_config(&testcase_config_path)?;
    if testcase_config.isa_base != ISABase::Rv64 {
        eprintln!(
            "Warning: expected rv64 ISA base, but config uses {}",
            testcase_config.isa_base.to_str()
        );
    }

    let impl_timeouts = load_impl_timeouts(&timeout_path)?;
    let run_options = load_run_options(&run_options_path)?;

    let riscv_impls = RiscVImplVec::from_impls(vec![RiscVImpl::Spike, RiscVImpl::Rocket]);

    let settings = DiffAnalysisSettings {
        testcase_config,
        max_iterations: run_options.max_iterations,
        history_min_test_threshold: run_options.history_min_test_threshold,
        impl_timeouts,
        emit_execution_output_json: run_options.emit_execution_output_json,
        emit_execution_report_md: run_options.emit_execution_report_md,
        cleanup_successful_iteration_artifacts: run_options.cleanup_successful_iteration_artifacts,
        cleanup_successful_diff_run: run_options.cleanup_successful_diff_run,
        guidance_strategy: None,
        transition_seed_pool_limit: 64,
        transition_seed_window: 16,
    };

    fs::create_dir_all(&run_root)?;
    let config = DiffAnalysisConfig {
        testcase_config: settings.testcase_config.clone(),
        run_root: run_root.clone(),
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

    let start = Instant::now();
    let result = run_diff_analysis(&riscv_impls, config)?;
    let elapsed = start.elapsed();

    println!("Rocket vs Spike RV64 diff-analysis");
    println!("Total time: {:.3} seconds", elapsed.as_secs_f64());
    println!(
        "Exception removal rounds: {}",
        result.exception_removal_rounds
    );
    println!("Write removal rounds: {}", result.write_removal_rounds);
    println!("Output directory: {}", result.output_root.display());

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

fn load_run_options(path: &Path) -> Result<RunOptions, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let value: TomlValue = toml::from_str(&content)?;

    let table_value = value.get("run_options").cloned().unwrap_or(value);

    let opts: RunOptionsFile = table_value.try_into()?;
    Ok(RunOptions {
        max_iterations: opts.max_iterations,
        history_min_test_threshold: opts.history_min_test_threshold,
        emit_execution_output_json: opts.emit_execution_output_json,
        emit_execution_report_md: opts.emit_execution_report_md,
        cleanup_successful_iteration_artifacts: opts.cleanup_successful_iteration_artifacts,
        cleanup_successful_diff_run: opts.cleanup_successful_diff_run,
    })
}
