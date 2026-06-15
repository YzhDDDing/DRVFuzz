use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use log::{error, info, warn};
use serde::Deserialize;
use serde_json::{from_reader, to_writer_pretty};
use strum::IntoEnumIterator;

use DRVFuzz::build_elf::build_elf_with_extensions;
use DRVFuzz::diff_analysis::{
    DiffAnalysisConfig, DiffAnalysisError, DiffAnalysisSettings, run_diff_analysis,
    run_diff_analysis_with_testcase,
};
use DRVFuzz::execution_output::{ExecutionContextOutput, generate_execution_context_report};
use DRVFuzz::isa_base::ISABase;
use DRVFuzz::riscv_impls::RiscVImpl;
use DRVFuzz::riscv_impls_vec::{GeneratedTestCase, RiscVImplVec, TestCaseConfig};
use DRVFuzz::sd_model::{
    analyze_transitions, write_transition_report_json, write_transition_report_md,
};
use DRVFuzz::transition_guidance::{GuidanceState, GuidanceStrategy, splice_guided_seed};
use riscv_instruction_types::RegisterConfig;
type Result<T> = std::result::Result<T, CliError>;
const DEFAULT_SDMODEL_PROBABILITY: f64 = 0.5;

#[derive(Debug)]
struct CliError {
    message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    fn with_source<E>(message: impl Into<String>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for CliError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|src| src.as_ref() as &(dyn StdError + 'static))
    }
}

trait ResultExt<T> {
    fn with_context<F, M>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> M,
        M: Into<String>;

    fn context<M>(self, message: M) -> Result<T>
    where
        M: Into<String>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn with_context<F, M>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> M,
        M: Into<String>,
    {
        self.map_err(|err| CliError::with_source(f().into(), err))
    }

    fn context<M>(self, message: M) -> Result<T>
    where
        M: Into<String>,
    {
        self.map_err(|err| CliError::with_source(message.into(), err))
    }
}

macro_rules! bail {
    ($($arg:tt)*) => {
        return Err(CliError::new(format!($($arg)*)));
    };
}

#[derive(Parser)]
#[command(
    version,
    about = "Generate, execute, and differential-test RISC-V programs",
    long_about = "Run standalone assembly, iterate diff analysis across implementations, replay saved testcases, or emit build artifacts without executing them."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(
        name = "exec",
        alias = "execute",
        about = "Execute one assembly program on a selected implementation",
        long_about = "Compile and run the provided assembly for the chosen implementation and ISA base. Writes execution_output.json, execution_report.md, and instruction snapshots under the run directory for later inspection."
    )]
    Execute {
        /// Assembly file containing the initialization and test instructions to execute
        #[arg(long)]
        asm: PathBuf,

        /// Implementation wrapper to run against (e.g. spike, rocket, picorv32, cva6)
        #[arg(long, value_enum)]
        riscv_impl: RiscVImpl,

        /// ISA base to assemble for (rv32 or rv64)
        #[arg(long, value_enum)]
        isa: ISABase,

        /// Directory where artifacts are written; a per-implementation subdirectory is created
        #[arg(long = "run-dir")]
        run_dir: PathBuf,

        /// Optional execution timeout in seconds; omit to let the run complete naturally
        #[arg(long = "timeout-secs")]
        timeout_secs: Option<u64>,

        /// Request unaligned memory accesses if the implementation supports them
        #[arg(long = "allow-unaligned", default_value_t = false)]
        allow_unaligned: bool,

        /// Emit transition_report.json/.md using SDModel execution-state transitions
        #[arg(long = "transition-report", default_value_t = false)]
        transition_report: bool,
    },
    #[command(
        name = "diff",
        alias = "diff-analysis",
        about = "Iteratively fuzz multiple implementations and minimize divergences",
        long_about = "Continuously generates random programs from the provided testcase_config, executes them across the selected implementations, and minimizes any mismatching behavior. Each worker writes a timestamped subdirectory under --run-dir. Leave --max-runs unset to keep iterating until a worker fails or you stop the process."
    )]
    DiffAnalysis {
        /// TOML file containing the [testcase_config] used to generate random programs
        #[arg(long = "testcase-config")]
        testcase_config: PathBuf,

        /// Implementations to include; defaults to every ISA-compatible core, filtered by unaligned_access_required
        #[arg(long = "riscv-impl", value_enum, num_args = 0.., value_delimiter = ',')]
        riscv_impls: Vec<RiscVImpl>,

        /// TOML file with an [impl_timeout_secs] table mapping implementations to per-run timeouts (seconds)
        #[arg(long = "impl-timeout")]
        impl_timeout_file: Option<PathBuf>,

        /// TOML file providing diff runner options (iterations, emit, cleanup); supports flat or [run_options] formats
        #[arg(long = "run-options")]
        run_options_file: Option<PathBuf>,

        /// Base directory for diff runs; each iteration gets a timestamped subdirectory
        #[arg(long = "run-dir")]
        run_dir: PathBuf,

        /// Number of diff-analysis workers to run in parallel
        #[arg(long, default_value_t = 1)]
        concurrency: usize,

        /// Total diff iterations to execute across all workers; omit to keep running until stopped
        #[arg(long = "max-runs")]
        max_runs: Option<usize>,

        /// Enable the SDModel sensitive-data generator for boundary values and exception triggers
        #[arg(long = "sdmodel", default_value_t = false)]
        sdmodel: bool,

        /// Override SDModel injection probability (0.0 to 1.0)
        #[arg(long = "sdmodel-probability")]
        sdmodel_probability: Option<f64>,

        /// Enable transition-guided seed feedback using SDModel execution states
        #[arg(long = "transition-guided", default_value_t = false)]
        transition_guided: bool,

        /// Enable mode-guided ablation feedback using individual SDModel states only
        #[arg(long = "mode-guided", default_value_t = false)]
        mode_guided: bool,

        /// Maximum number of transition-interesting seeds retained
        #[arg(long = "transition-seed-pool-limit")]
        transition_seed_pool_limit: Option<usize>,

        /// Number of seed test instructions spliced into each guided testcase
        #[arg(long = "transition-seed-window")]
        transition_seed_window: Option<usize>,
    },
    #[command(
        name = "diff-testcase",
        about = "Replay diff analysis from an existing testcase",
        long_about = "Re-run the diff-analysis pipeline using a previously generated testcase.json. Useful for reproducing failures with new timeouts or run options; artifacts are written under --run-dir."
    )]
    DiffTestcase {
        /// Path to a previously generated testcase.json to replay
        #[arg(long = "testcase")]
        testcase: PathBuf,

        /// Directory where artifacts for this targeted diff run are written
        #[arg(long = "run-dir")]
        run_dir: PathBuf,

        /// TOML file with an [impl_timeout_secs] table mapping implementations to per-run timeouts (seconds)
        #[arg(long = "impl-timeout")]
        impl_timeout_file: Option<PathBuf>,

        /// TOML file providing diff runner options (iterations, emit, cleanup); supports flat or [run_options] formats
        #[arg(long = "run-options")]
        run_options_file: Option<PathBuf>,

        /// Enable transition report extraction during replay
        #[arg(long = "transition-guided", default_value_t = false)]
        transition_guided: bool,
    },
    #[command(
        name = "diff-spike",
        about = "Compare Spike against other implementations for the configured ISA",
        long_about = "For each selected implementation that supports the configured ISA base, run the diff-analysis workflow between Spike and that target. Artifacts are written under --run-dir/spike_<impl>_<isa>. Defaults to testing Spike against every supported implementation."
    )]
    DiffSpike {
        /// TOML file containing the [testcase_config] used to generate random programs
        #[arg(long = "testcase-config")]
        testcase_config: PathBuf,

        /// Base directory used to store run artifacts for each Spike pairing (subdirectories are created automatically)
        #[arg(long = "run-dir")]
        run_dir: PathBuf,

        /// Number of diff-analysis workers to run in parallel for each pairing
        #[arg(long, default_value_t = 1)]
        concurrency: usize,

        /// Total diff-analysis runs to execute per pairing; omit to keep iterating
        #[arg(long = "max-runs")]
        max_runs: Option<usize>,

        /// Optional list of non-Spike implementations to pair with Spike; defaults to all ISA-compatible targets
        #[arg(long = "riscv-impl", value_enum, num_args = 0.., value_delimiter = ',')]
        riscv_impls: Vec<RiscVImpl>,

        /// TOML file with an [impl_timeout_secs] table mapping implementations to per-run timeouts (seconds)
        #[arg(long = "impl-timeout")]
        impl_timeout_file: Option<PathBuf>,

        /// TOML file providing diff runner options (iterations, emit, cleanup); supports flat or [run_options] formats
        #[arg(long = "run-options")]
        run_options_file: Option<PathBuf>,

        /// Enable the SDModel sensitive-data generator for boundary values and exception triggers
        #[arg(long = "sdmodel", default_value_t = false)]
        sdmodel: bool,

        /// Override SDModel injection probability (0.0 to 1.0)
        #[arg(long = "sdmodel-probability")]
        sdmodel_probability: Option<f64>,

        /// Enable transition-guided seed feedback using SDModel execution states
        #[arg(long = "transition-guided", default_value_t = false)]
        transition_guided: bool,

        /// Enable mode-guided ablation feedback using individual SDModel states only
        #[arg(long = "mode-guided", default_value_t = false)]
        mode_guided: bool,

        /// Maximum number of transition-interesting seeds retained
        #[arg(long = "transition-seed-pool-limit")]
        transition_seed_pool_limit: Option<usize>,

        /// Number of seed test instructions spliced into each guided testcase
        #[arg(long = "transition-seed-window")]
        transition_seed_window: Option<usize>,
    },
    #[command(
        name = "generate",
        about = "Generate a random testcase and ELF artifacts without executing",
        long_about = "Builds an implementation-specific program from the testcase_config (optionally overriding the ISA base) and writes testcase.json, user_instructions.asm, program.S, and ELF outputs under --run-dir. With --minimal-artifacts only the assembly and ELF are emitted."
    )]
    Generate {
        /// TOML file containing the [testcase_config] used to generate instructions
        #[arg(long = "config")]
        testcase_config: PathBuf,

        /// Target implementation; initialization and extensions are tailored to this core
        #[arg(long = "riscv-impl", value_enum)]
        riscv_impl: RiscVImpl,

        /// Optional override for the isa_base in the config (rv32 or rv64)
        #[arg(long, value_enum)]
        isa: Option<ISABase>,

        /// Directory that will store generated instructions and ELF outputs
        #[arg(long = "run-dir")]
        run_dir: PathBuf,

        /// Emit only program.S/.s and ELF artifacts; skip testcase.json and instruction snapshot
        #[arg(long = "minimal-artifacts")]
        minimal_artifacts: bool,

        /// Enable the SDModel sensitive-data generator for boundary values and exception triggers
        #[arg(long = "sdmodel", default_value_t = false)]
        sdmodel: bool,

        /// Override SDModel injection probability (0.0 to 1.0)
        #[arg(long = "sdmodel-probability")]
        sdmodel_probability: Option<f64>,
    },
}

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
    #[serde(default)]
    transition_guided_mode: bool,
    #[serde(default)]
    mode_guided_mode: bool,
    #[serde(default)]
    transition_seed_pool_limit: Option<usize>,
    #[serde(default)]
    transition_seed_window: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct RunOptions {
    max_iterations: Option<usize>,
    history_min_test_threshold: Option<usize>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
    cleanup_successful_iteration_artifacts: bool,
    cleanup_successful_diff_run: bool,
    guidance_strategy: Option<GuidanceStrategy>,
    transition_seed_pool_limit: usize,
    transition_seed_window: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            max_iterations: None,
            history_min_test_threshold: None,
            emit_execution_output_json: false,
            emit_execution_report_md: false,
            cleanup_successful_iteration_artifacts: false,
            cleanup_successful_diff_run: false,
            guidance_strategy: None,
            transition_seed_pool_limit: 64,
            transition_seed_window: 16,
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init()
        .map_err(|err| CliError::new(format!("failed to initialize logger: {err}")))?;

    let cli = Cli::parse();

    match cli.command {
        Command::Execute {
            asm,
            riscv_impl,
            isa,
            run_dir,
            timeout_secs,
            allow_unaligned,
            transition_report,
        } => {
            let asm = absolutize_path(asm)?;
            let run_dir = absolutize_path(run_dir)?;
            run_execute_mode(
                asm,
                riscv_impl,
                isa,
                run_dir,
                timeout_secs,
                allow_unaligned,
                transition_report,
            )
        }
        Command::DiffAnalysis {
            testcase_config,
            riscv_impls,
            impl_timeout_file,
            run_options_file,
            run_dir,
            concurrency,
            max_runs,
            sdmodel,
            sdmodel_probability,
            transition_guided,
            mode_guided,
            transition_seed_pool_limit,
            transition_seed_window,
        } => {
            let testcase_config = absolutize_path(testcase_config)?;
            let impl_timeout_file = absolutize_optional_path(impl_timeout_file)?;
            let run_options_file = absolutize_optional_path(run_options_file)?;
            let run_dir = absolutize_path(run_dir)?;
            run_diff_analysis_mode(
                testcase_config,
                riscv_impls,
                impl_timeout_file,
                run_options_file,
                concurrency,
                run_dir,
                max_runs,
                sdmodel,
                sdmodel_probability,
                transition_guided,
                mode_guided,
                transition_seed_pool_limit,
                transition_seed_window,
            )
        }
        Command::DiffTestcase {
            testcase,
            run_dir,
            impl_timeout_file,
            run_options_file,
            transition_guided,
        } => {
            let testcase = absolutize_path(testcase)?;
            let run_dir = absolutize_path(run_dir)?;
            let impl_timeout_file = absolutize_optional_path(impl_timeout_file)?;
            let run_options_file = absolutize_optional_path(run_options_file)?;
            run_diff_testcase_mode(
                testcase,
                run_dir,
                impl_timeout_file,
                run_options_file,
                transition_guided,
            )
        }
        Command::DiffSpike {
            testcase_config,
            run_dir,
            concurrency,
            max_runs,
            riscv_impls,
            impl_timeout_file,
            run_options_file,
            sdmodel,
            sdmodel_probability,
            transition_guided,
            mode_guided,
            transition_seed_pool_limit,
            transition_seed_window,
        } => {
            let testcase_config = absolutize_path(testcase_config)?;
            let run_dir = absolutize_path(run_dir)?;
            let impl_timeout_file = absolutize_optional_path(impl_timeout_file)?;
            let run_options_file = absolutize_optional_path(run_options_file)?;
            run_diff_spike_mode(
                testcase_config,
                run_dir,
                concurrency,
                max_runs,
                riscv_impls,
                impl_timeout_file,
                run_options_file,
                sdmodel,
                sdmodel_probability,
                transition_guided,
                mode_guided,
                transition_seed_pool_limit,
                transition_seed_window,
            )
        }
        Command::Generate {
            testcase_config,
            riscv_impl,
            isa,
            run_dir,
            minimal_artifacts,
            sdmodel,
            sdmodel_probability,
        } => {
            let testcase_config = absolutize_path(testcase_config)?;
            let run_dir = absolutize_path(run_dir)?;
            run_generate_mode(
                testcase_config,
                run_dir,
                riscv_impl,
                isa,
                minimal_artifacts,
                sdmodel,
                sdmodel_probability,
            )
        }
    }
}

fn run_execute_mode(
    asm: PathBuf,
    riscv_impl: RiscVImpl,
    isa: ISABase,
    run_dir: PathBuf,
    timeout_secs: Option<u64>,
    allow_unaligned: bool,
    transition_report: bool,
) -> Result<()> {
    let (instructions, instruction_offsets) = load_instructions(&asm)?;
    if instructions.is_empty() {
        bail!("no instructions were found in `{}`", asm.display());
    }

    let isa_base: ISABase = isa.into();
    let impl_run_dir = run_dir.join(riscv_impl.to_string());
    fs::create_dir_all(&impl_run_dir)
        .with_context(|| format!("failed to create run directory {}", impl_run_dir.display()))?;

    let timeout = timeout_secs.map(Duration::from_secs);

    let exec_start = Instant::now();
    let mut execution_output = riscv_impl
        .execute(
            &impl_run_dir,
            isa_base,
            &instructions,
            timeout,
            allow_unaligned,
        )
        .with_context(|| {
            format!(
                "failed to execute instructions on implementation {} ({isa_base})",
                riscv_impl
            )
        })?;
    let exec_duration = exec_start.elapsed();

    let mem_range = riscv_impl.user_mem_range();
    execution_output
        .normalize(
            mem_range,
            &RegisterConfig {
                integer_register_range: (0, 31),
                floating_point_register_range: (0, 31),
                vector_register_range: (0, 31),
            },
            (0, 31),
        )
        .with_context(|| "failed to normalize execution output")?;
    let context_output = ExecutionContextOutput::from_execution_output(
        execution_output,
        mem_range,
        &instructions,
        &instruction_offsets,
    )
    .with_context(|| "failed to compute execution context output")?;

    let json_path = impl_run_dir.join("execution_output.json");
    let json_file = fs::File::create(&json_path)
        .with_context(|| format!("failed to create {}", json_path.display()))?;
    to_writer_pretty(json_file, &context_output)
        .with_context(|| format!("failed to write {}", json_path.display()))?;

    let report_path = impl_run_dir.join("execution_report.md");
    generate_execution_context_report(&context_output, &report_path, &instructions).map_err(
        |err| CliError::new(format!("failed to write {}: {err}", report_path.display())),
    )?;

    if transition_report {
        let analysis = analyze_transitions(&context_output, &instructions);
        let transition_json_path = impl_run_dir.join("transition_report.json");
        write_transition_report_json(&transition_json_path, &analysis).map_err(|err| {
            CliError::new(format!(
                "failed to write {}: {err}",
                transition_json_path.display()
            ))
        })?;
        let transition_md_path = impl_run_dir.join("transition_report.md");
        write_transition_report_md(&transition_md_path, &analysis, &instructions).map_err(
            |err| {
                CliError::new(format!(
                    "failed to write {}: {err}",
                    transition_md_path.display()
                ))
            },
        )?;
    }

    let snapshot_path = impl_run_dir.join("user_instructions.asm");
    fs::write(&snapshot_path, instructions.join("\n") + "\n").with_context(|| {
        format!(
            "failed to write instruction snapshot to {}",
            snapshot_path.display()
        )
    })?;

    let timing_report = format!(
        "# Execution Timing\n\n| Implementation | Duration (s) |\n| --- | --- |\n| {} | {:.3} |\n",
        riscv_impl,
        exec_duration.as_secs_f64()
    );
    let timing_path = impl_run_dir.join("execution_timing.md");
    fs::write(&timing_path, timing_report)
        .with_context(|| format!("failed to write {}", timing_path.display()))?;

    info!(
        "Executed {} instructions on {} ({isa_base}); report at {}",
        instructions.len(),
        riscv_impl,
        report_path.display()
    );

    Ok(())
}

fn run_diff_analysis_mode(
    testcase_config_path: PathBuf,
    explicit_impls: Vec<RiscVImpl>,
    impl_timeout_file: Option<PathBuf>,
    run_options_file: Option<PathBuf>,
    concurrency: usize,
    run_dir: PathBuf,
    max_runs: Option<usize>,
    sdmodel: bool,
    sdmodel_probability: Option<f64>,
    transition_guided: bool,
    mode_guided: bool,
    transition_seed_pool_limit: Option<usize>,
    transition_seed_window: Option<usize>,
) -> Result<()> {
    let mut testcase_config = load_testcase_only_config(&testcase_config_path)?;
    apply_sdmodel_overrides(&mut testcase_config, sdmodel, sdmodel_probability)?;
    let timeout_map = load_impl_timeouts(impl_timeout_file.as_deref())?;
    let run_options = load_run_options(run_options_file.as_deref())?;
    let base_impls = if explicit_impls.is_empty() {
        RiscVImplVec::all(testcase_config.isa_base)
    } else {
        RiscVImplVec::from_impls(explicit_impls.clone())
    };
    let guidance_strategy = resolve_guidance_strategy(
        transition_guided,
        mode_guided,
        run_options.guidance_strategy,
    )?;
    let settings = DiffAnalysisSettings {
        testcase_config,
        max_iterations: run_options.max_iterations,
        history_min_test_threshold: run_options.history_min_test_threshold,
        impl_timeouts: timeout_map,
        emit_execution_output_json: run_options.emit_execution_output_json,
        emit_execution_report_md: run_options.emit_execution_report_md,
        cleanup_successful_iteration_artifacts: run_options.cleanup_successful_iteration_artifacts,
        cleanup_successful_diff_run: run_options.cleanup_successful_diff_run,
        guidance_strategy,
        transition_seed_pool_limit: transition_seed_pool_limit
            .unwrap_or(run_options.transition_seed_pool_limit)
            .max(1),
        transition_seed_window: transition_seed_window
            .unwrap_or(run_options.transition_seed_window)
            .max(1),
    };
    let filtered_impl_vec = base_impls.filter_by_unaligned_access_requirement(
        settings.testcase_config.unaligned_access_required,
        &HashMap::new(),
    );
    if filtered_impl_vec.iter().next().is_none() {
        bail!("no implementations satisfy the configured unaligned_access requirement");
    }
    let label = format!("config {}", testcase_config_path.display());
    run_diff_analysis_with_impls(
        filtered_impl_vec,
        settings,
        concurrency,
        run_dir,
        max_runs,
        &label,
    )
}

fn run_diff_analysis_with_impls(
    riscv_impls: RiscVImplVec,
    settings: DiffAnalysisSettings,
    concurrency: usize,
    run_dir: PathBuf,
    max_runs: Option<usize>,
    label: &str,
) -> Result<()> {
    if concurrency == 0 {
        bail!("--concurrency must be greater than zero");
    }

    if matches!(max_runs, Some(0)) {
        info!("No diff-analysis runs requested for {label}; exiting.");
        return Ok(());
    }

    fs::create_dir_all(&run_dir)
        .with_context(|| format!("failed to create base run directory {}", run_dir.display()))?;

    if riscv_impls.iter().next().is_none() {
        bail!("diff-analysis requires at least one implementation to execute");
    }

    let shared_settings = Arc::new(settings);
    let guidance = if let Some(strategy) = shared_settings.guidance_strategy {
        Some(Arc::new(Mutex::new(GuidanceState::new(
            strategy,
            shared_settings.transition_seed_pool_limit,
        ))))
    } else {
        None
    };
    let shared_impls = Arc::new(riscv_impls);
    let run_counter = Arc::new(AtomicU64::new(0));
    let max_runs_limit = max_runs.map(|v| v as u64);

    info!(
        "Starting diff-analysis runner ({label}) with concurrency {} in {}",
        concurrency,
        run_dir.display()
    );

    let running = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::with_capacity(concurrency);
    for worker_id in 0..concurrency {
        let worker_impls = Arc::clone(&shared_impls);
        let worker_settings = Arc::clone(&shared_settings);
        let worker_base_dir = run_dir.clone();
        let worker_counter = Arc::clone(&run_counter);
        let worker_flag = Arc::clone(&running);
        let worker_limit = max_runs_limit;
        let worker_guidance = guidance.as_ref().map(Arc::clone);

        handles.push(thread::spawn(move || {
            diff_worker_loop(
                worker_id,
                worker_impls,
                worker_settings,
                worker_base_dir,
                worker_counter,
                worker_flag,
                worker_limit,
                worker_guidance,
            )
        }));
    }

    let mut first_error: Option<CliError> = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                running.store(false, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(panic) => {
                running.store(false, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(CliError::new(format!(
                        "diff analysis worker panicked: {:?}",
                        panic
                    )));
                }
            }
        }
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        info!("All diff-analysis workers ({label}) exited cleanly.");
        Ok(())
    }
}

fn run_diff_testcase_mode(
    testcase_path: PathBuf,
    run_dir: PathBuf,
    impl_timeout_file: Option<PathBuf>,
    run_options_file: Option<PathBuf>,
    transition_guided: bool,
) -> Result<()> {
    let file = fs::File::open(&testcase_path).with_context(|| {
        format!(
            "failed to open testcase definition {}",
            testcase_path.display()
        )
    })?;
    let testcase: GeneratedTestCase = from_reader(file)
        .with_context(|| format!("failed to parse testcase {}", testcase_path.display()))?;

    let impls: Vec<RiscVImpl> = testcase.init_insts.keys().cloned().collect();
    if impls.is_empty() {
        bail!(
            "testcase {} does not contain any RISC-V implementations",
            testcase_path.display()
        );
    }

    let timeout_map = load_impl_timeouts(impl_timeout_file.as_deref())?;
    let run_options = load_run_options(run_options_file.as_deref())?;

    let riscv_impls = RiscVImplVec::from_impls(impls);
    let diff_config = DiffAnalysisConfig {
        testcase_config: testcase.config.clone(),
        run_root: run_dir.clone(),
        max_iterations: run_options.max_iterations,
        history_min_test_threshold: run_options.history_min_test_threshold,
        impl_timeouts: timeout_map,
        emit_execution_output_json: run_options.emit_execution_output_json,
        emit_execution_report_md: run_options.emit_execution_report_md,
        cleanup_successful_iteration_artifacts: run_options.cleanup_successful_iteration_artifacts,
        cleanup_successful_diff_run: run_options.cleanup_successful_diff_run,
        guidance_strategy: resolve_guidance_strategy(
            transition_guided,
            false,
            run_options.guidance_strategy,
        )?,
        transition_seed_pool_limit: run_options.transition_seed_pool_limit,
        transition_seed_window: run_options.transition_seed_window,
    };
    let filtered_impls = riscv_impls.filter_by_unaligned_access_requirement(
        diff_config.testcase_config.unaligned_access_required,
        &HashMap::new(),
    );
    if filtered_impls.iter().next().is_none() {
        bail!(
            "testcase {} has no implementations matching the configured unaligned_access requirement",
            testcase_path.display()
        );
    }

    run_diff_analysis_with_testcase(&filtered_impls, diff_config, testcase).map_err(|err| {
        CliError::new(format!(
            "diff analysis failed when replaying testcase {}: {err}",
            testcase_path.display()
        ))
    })?;

    info!(
        "Replayed diff testcase {}; artifacts written to {}",
        testcase_path.display(),
        run_dir.display()
    );
    Ok(())
}

fn run_diff_spike_mode(
    testcase_config_path: PathBuf,
    run_dir: PathBuf,
    concurrency: usize,
    max_runs: Option<usize>,
    explicit_impls: Vec<RiscVImpl>,
    impl_timeout_file: Option<PathBuf>,
    run_options_file: Option<PathBuf>,
    sdmodel: bool,
    sdmodel_probability: Option<f64>,
    transition_guided: bool,
    mode_guided: bool,
    transition_seed_pool_limit: Option<usize>,
    transition_seed_window: Option<usize>,
) -> Result<()> {
    fs::create_dir_all(&run_dir)
        .with_context(|| format!("failed to create base run directory {}", run_dir.display()))?;

    let mut testcase_config = load_testcase_only_config(&testcase_config_path)?;
    apply_sdmodel_overrides(&mut testcase_config, sdmodel, sdmodel_probability)?;
    let configured_isa = testcase_config.isa_base;
    let timeout_map = load_impl_timeouts(impl_timeout_file.as_deref())?;
    let run_options = load_run_options(run_options_file.as_deref())?;

    let selected_impls: Vec<RiscVImpl> = if explicit_impls.is_empty() {
        RiscVImpl::iter().collect()
    } else {
        explicit_impls
    };

    let targets: Vec<RiscVImpl> = selected_impls
        .into_iter()
        .filter(|impl_ref| *impl_ref != RiscVImpl::Spike)
        .collect();

    if targets.is_empty() {
        bail!("diff-spike requires at least one non-Spike implementation");
    }

    let spike_supported = RiscVImpl::Spike.supported_isa_bases();
    if !spike_supported.contains(&configured_isa) {
        bail!(
            "diff-spike requires Spike to support the configured ISA base {}",
            configured_isa
        );
    }
    let guidance_strategy = resolve_guidance_strategy(
        transition_guided,
        mode_guided,
        run_options.guidance_strategy,
    )?;
    let mut executed_pairs = 0usize;

    for impl_ref in targets {
        if !impl_ref.supported_isa_bases().contains(&configured_isa) {
            warn!(
                "Skipping {} because it does not support ISA base {}",
                impl_ref, configured_isa
            );
            continue;
        }

        executed_pairs += 1;
        let mut pair_settings = DiffAnalysisSettings {
            testcase_config: testcase_config.clone(),
            max_iterations: run_options.max_iterations,
            history_min_test_threshold: run_options.history_min_test_threshold,
            impl_timeouts: timeout_map.clone(),
            emit_execution_output_json: run_options.emit_execution_output_json,
            emit_execution_report_md: run_options.emit_execution_report_md,
            cleanup_successful_iteration_artifacts: run_options
                .cleanup_successful_iteration_artifacts,
            cleanup_successful_diff_run: run_options.cleanup_successful_diff_run,
            guidance_strategy,
            transition_seed_pool_limit: transition_seed_pool_limit
                .unwrap_or(run_options.transition_seed_pool_limit)
                .max(1),
            transition_seed_window: transition_seed_window
                .unwrap_or(run_options.transition_seed_window)
                .max(1),
        };
        pair_settings.testcase_config.isa_base = configured_isa;

        let impl_name = impl_ref.to_string().to_lowercase();
        let pair_run_dir = run_dir.join(format!("spike_{}_{}", impl_name, configured_isa.to_str()));
        let pair_impls = RiscVImplVec::from_impls(vec![RiscVImpl::Spike, impl_ref])
            .filter_by_unaligned_access_requirement(
                pair_settings.testcase_config.unaligned_access_required,
                &HashMap::new(),
            );
        if pair_impls.iter().count() < 2 {
            warn!(
                "Skipping Spike vs {} because unaligned_access requirement filters out a participant",
                impl_ref
            );
            continue;
        }
        let label = format!("Spike vs {} ({})", impl_ref, configured_isa);
        run_diff_analysis_with_impls(
            pair_impls,
            pair_settings.clone(),
            concurrency,
            pair_run_dir.clone(),
            max_runs,
            &label,
        )
        .with_context(|| {
            format!(
                "diff analysis failed for Spike vs {} ({}) in {}",
                impl_ref,
                configured_isa,
                pair_run_dir.display()
            )
        })?;
    }

    if executed_pairs == 0 {
        bail!(
            "no diff-spike pairings were executed for ISA base {}; ensure other implementations support it",
            configured_isa
        );
    }

    info!(
        "Completed {} Spike diff pairings; artifacts written to {}",
        executed_pairs,
        run_dir.display()
    );
    Ok(())
}

fn run_generate_mode(
    config_path: PathBuf,
    run_dir: PathBuf,
    riscv_impl: RiscVImpl,
    isa_override: Option<ISABase>,
    minimal_artifacts: bool,
    sdmodel: bool,
    sdmodel_probability: Option<f64>,
) -> Result<()> {
    let mut testcase_config = load_testcase_only_config(&config_path)?;
    apply_sdmodel_overrides(&mut testcase_config, sdmodel, sdmodel_probability)?;
    if let Some(isa) = isa_override {
        testcase_config.isa_base = isa;
    }
    let isa_base = testcase_config.isa_base;

    if !riscv_impl.supported_isa_bases().contains(&isa_base) {
        bail!(
            "implementation {} does not support ISA base {}",
            riscv_impl,
            isa_base.to_str()
        );
    }

    fs::create_dir_all(&run_dir)
        .with_context(|| format!("failed to create run directory {}", run_dir.display()))?;

    let riscv_impls = RiscVImplVec::from_impls(vec![riscv_impl]);
    let testcase = riscv_impls
        .generate_random_testcase(testcase_config.clone())
        .with_context(|| {
            format!(
                "failed to generate random testcase using {}",
                config_path.display()
            )
        })?;

    let instructions = testcase
        .combined_insts_of(&riscv_impl)
        .ok_or_else(|| CliError::new(format!("missing instructions for {}", riscv_impl)))?;

    if !minimal_artifacts {
        let testcase_path = run_dir.join("testcase.json");
        let testcase_file = fs::File::create(&testcase_path)
            .with_context(|| format!("failed to create {}", testcase_path.display()))?;
        to_writer_pretty(testcase_file, &testcase)
            .with_context(|| format!("failed to write {}", testcase_path.display()))?;

        let mut snapshot_content = instructions.join("\n");
        snapshot_content.push('\n');
        let snapshot_path = run_dir.join("user_instructions.asm");
        fs::write(&snapshot_path, snapshot_content)
            .with_context(|| format!("failed to write {}", snapshot_path.display()))?;
    }

    let asm_program = riscv_impl
        .build_asm_content(&instructions, isa_base)
        .with_context(|| {
            format!(
                "failed to build assembly for {} ({})",
                riscv_impl,
                isa_base.to_str()
            )
        })?;
    let asm_path = run_dir.join("program.S");
    fs::write(&asm_path, asm_program)
        .with_context(|| format!("failed to write {}", asm_path.display()))?;

    let build_result =
        build_elf_with_extensions(&asm_path, &testcase.extension_map, &isa_base, &riscv_impl)
            .with_context(|| {
                format!(
                    "failed to build ELF artifacts for {} ({})",
                    riscv_impl,
                    isa_base.to_str()
                )
            })?;

    let test_len = testcase
        .test_insts
        .get(&riscv_impl)
        .map(|block| block.len())
        .unwrap_or_default();
    let artifact_note = if minimal_artifacts {
        "minimal artifact mode (only .S/.s/.elf emitted)"
    } else {
        "full artifact set emitted"
    };
    info!(
        "Generated {} test instructions for {} ({}) using {}; ELF at {}; {}",
        test_len,
        riscv_impl,
        isa_base.to_str(),
        config_path.display(),
        build_result.executable_file.display(),
        artifact_note
    );

    Ok(())
}

fn diff_worker_loop(
    worker_id: usize,
    riscv_impls: Arc<RiscVImplVec>,
    settings: Arc<DiffAnalysisSettings>,
    base_run_dir: PathBuf,
    counter: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    max_runs: Option<u64>,
    guidance: Option<Arc<Mutex<GuidanceState>>>,
) -> Result<()> {
    while running.load(Ordering::SeqCst) {
        let maybe_run_root = next_run_directory(
            &base_run_dir,
            settings.testcase_config.isa_base,
            &*counter,
            worker_id,
            max_runs,
        )?;
        let run_root = match maybe_run_root {
            Some(path) => path,
            None => {
                running.store(false, Ordering::SeqCst);
                break;
            }
        };
        fs::create_dir_all(&run_root).with_context(|| {
            format!(
                "failed to create diff-analysis directory {}",
                run_root.display()
            )
        })?;

        let diff_config = settings.with_run_root(run_root.clone());
        let guided_seed = if let Some(guidance) = &guidance {
            let mut guard = guidance
                .lock()
                .map_err(|_| CliError::new("guidance state mutex poisoned"))?;
            guard.select_seed()
        } else {
            None
        };

        info!(
            "[worker {:02}] starting diff analysis in {}",
            worker_id,
            run_root.display()
        );

        let run_result = if let Some(seed) = guided_seed {
            let mut testcase = riscv_impls
                .generate_random_testcase(settings.testcase_config.clone())
                .with_context(|| {
                    format!(
                        "[worker {:02}] failed to generate guided testcase for {}",
                        worker_id,
                        run_root.display()
                    )
                })?;
            let spliced = splice_guided_seed(&mut testcase, &seed, settings.transition_seed_window);
            if spliced {
                let strategy = guidance
                    .as_ref()
                    .and_then(|guidance| guidance.lock().ok().map(|guard| guard.strategy()));
                info!(
                    "[worker {:02}] mixed a {}-guided seed into {}",
                    worker_id,
                    strategy.map(GuidanceStrategy::label).unwrap_or("SDModel"),
                    run_root.display()
                );
            }
            run_diff_analysis_with_testcase(&riscv_impls, diff_config, testcase)
        } else {
            run_diff_analysis(&riscv_impls, diff_config)
        };

        match run_result {
            Ok(result) => {
                if let Some(guidance) = &guidance {
                    if let Some(analysis) = &result.transition_analysis {
                        let mut guard = guidance
                            .lock()
                            .map_err(|_| CliError::new("guidance state mutex poisoned"))?;
                        let record = guard.record_analysis(&result.initial_testcase, analysis);
                        info!(
                            "[worker {:02}] {} feedback: {} new / {} unique items; visited {}, seed pool {}",
                            worker_id,
                            record.strategy.label(),
                            record.new_items,
                            record.unique_items,
                            record.visited_items,
                            record.seed_pool_size
                        );
                    }
                }
                info!(
                    "[worker {:02}] completed diff analysis: {} exception rounds, {} write rounds; output at {}",
                    worker_id,
                    result.exception_removal_rounds,
                    result.write_removal_rounds,
                    result.output_root.display()
                );
            }
            Err(err) => {
                error!(
                    "[worker {:02}] diff analysis failed in {}: {err}",
                    worker_id,
                    run_root.display()
                );
                if let Err(report_err) =
                    write_diff_analysis_failure_report(&run_root, worker_id, &err)
                {
                    error!(
                        "[worker {:02}] failed to write diff-analysis failure report in {}: {}",
                        worker_id,
                        run_root.display(),
                        report_err
                    );
                }
                continue;
            }
        }

        if !running.load(Ordering::SeqCst) {
            break;
        }
    }

    info!("[worker {:02}] shutting down", worker_id);
    Ok(())
}

fn write_diff_analysis_failure_report(
    run_root: &Path,
    worker_id: usize,
    error: &DiffAnalysisError,
) -> Result<()> {
    let report_path = run_root.join("diff_analysis_failure.md");
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to prepare failure report directory {}",
                parent.display()
            )
        })?;
    }

    let mut content = String::new();
    writeln!(
        &mut content,
        "# Diff Analysis Run Failed\n\n- Worker ID: {:02}\n- Error: {}\n",
        worker_id, error
    )
    .unwrap();
    writeln!(&mut content, "```text\n{:#?}\n```", error).unwrap();

    fs::write(&report_path, content).with_context(|| {
        format!(
            "failed to write diff-analysis failure report {}",
            report_path.display()
        )
    })?;
    Ok(())
}

fn next_run_directory(
    base_run_dir: &Path,
    isa_base: ISABase,
    counter: &AtomicU64,
    worker_id: usize,
    max_runs: Option<u64>,
) -> Result<Option<PathBuf>> {
    let seq = if let Some(limit) = max_runs {
        match counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if current >= limit {
                None
            } else {
                Some(current + 1)
            }
        }) {
            Ok(prev) => prev,
            Err(_) => return Ok(None),
        }
    } else {
        counter.fetch_add(1, Ordering::Relaxed)
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?;
    let dir_name = format!(
        "{}_{:09}_{}_{}_w{:02}",
        now.as_secs(),
        now.subsec_nanos(),
        isa_base.to_str(),
        seq,
        worker_id
    );
    Ok(Some(base_run_dir.join(dir_name)))
}

fn load_testcase_only_config(path: &Path) -> Result<TestCaseConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read testcase config {}", path.display()))?;
    let parsed: TestCaseFileConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse testcase config {}", path.display()))?;
    Ok(parsed.testcase_config)
}

fn load_impl_timeouts(path: Option<&Path>) -> Result<HashMap<RiscVImpl, Duration>> {
    if let Some(p) = path {
        let content = fs::read_to_string(p)
            .with_context(|| format!("failed to read impl timeouts {}", p.display()))?;
        let parsed: ImplTimeoutFile = toml::from_str(&content)
            .with_context(|| format!("failed to parse impl timeouts {}", p.display()))?;
        Ok(parsed
            .impl_timeout_secs
            .into_iter()
            .map(|(impl_ref, secs)| (impl_ref, Duration::from_secs(secs)))
            .collect())
    } else {
        Ok(HashMap::new())
    }
}

fn load_run_options(path: Option<&Path>) -> Result<RunOptions> {
    if let Some(p) = path {
        let content = fs::read_to_string(p)
            .with_context(|| format!("failed to read run options {}", p.display()))?;
        // Support both flat configs and legacy `[run_options]` wrapper tables by
        // first parsing into a generic TOML value and then selecting the right table.
        let value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("failed to parse run options {}", p.display()))?;

        // If there is a top-level `run_options` table, use that; otherwise treat
        // the whole document as the options table (flat format).
        let table_value = value.get("run_options").cloned().unwrap_or(value);

        let opts: RunOptionsFile = table_value.try_into().map_err(|err: toml::de::Error| {
            CliError::new(format!(
                "failed to decode run options {}: {err}",
                p.display()
            ))
        })?;

        Ok(RunOptions {
            max_iterations: opts.max_iterations,
            history_min_test_threshold: opts.history_min_test_threshold,
            emit_execution_output_json: opts.emit_execution_output_json,
            emit_execution_report_md: opts.emit_execution_report_md,
            cleanup_successful_iteration_artifacts: opts.cleanup_successful_iteration_artifacts,
            cleanup_successful_diff_run: opts.cleanup_successful_diff_run,
            guidance_strategy: guidance_strategy_from_flags(
                opts.transition_guided_mode,
                opts.mode_guided_mode,
            )?,
            transition_seed_pool_limit: opts.transition_seed_pool_limit.unwrap_or(64).max(1),
            transition_seed_window: opts.transition_seed_window.unwrap_or(16).max(1),
        })
    } else {
        Ok(RunOptions::default())
    }
}

fn guidance_strategy_from_flags(
    transition_guided: bool,
    mode_guided: bool,
) -> Result<Option<GuidanceStrategy>> {
    if transition_guided && mode_guided {
        bail!("--transition-guided and --mode-guided are mutually exclusive");
    }
    if transition_guided {
        Ok(Some(GuidanceStrategy::Transition))
    } else if mode_guided {
        Ok(Some(GuidanceStrategy::Mode))
    } else {
        Ok(None)
    }
}

fn resolve_guidance_strategy(
    transition_guided: bool,
    mode_guided: bool,
    configured: Option<GuidanceStrategy>,
) -> Result<Option<GuidanceStrategy>> {
    let cli = guidance_strategy_from_flags(transition_guided, mode_guided)?;
    if let (Some(cli), Some(configured)) = (cli, configured) {
        if cli != configured {
            bail!(
                "CLI guidance strategy {} conflicts with run-options strategy {}",
                cli.label(),
                configured.label()
            );
        }
    }
    Ok(cli.or(configured))
}

fn apply_sdmodel_overrides(
    testcase_config: &mut TestCaseConfig,
    enabled: bool,
    probability: Option<f64>,
) -> Result<()> {
    if let Some(probability) = probability {
        if !(0.0..=1.0).contains(&probability) {
            bail!("--sdmodel-probability must be within [0.0, 1.0]");
        }
        testcase_config.data_sensitive_probability = probability;
        testcase_config.data_sensitive_mode = true;
    }

    if enabled {
        testcase_config.data_sensitive_mode = true;
        if probability.is_none() && testcase_config.data_sensitive_probability <= 0.0 {
            testcase_config.data_sensitive_probability = DEFAULT_SDMODEL_PROBABILITY;
        }
    }

    Ok(())
}

fn absolutize_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    let cwd = std::env::current_dir().context("failed to get current working directory")?;
    let joined = cwd.join(&path);
    if joined.exists() {
        joined
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", joined.display()))
    } else {
        Ok(joined)
    }
}

fn absolutize_optional_path(path: Option<PathBuf>) -> Result<Option<PathBuf>> {
    path.map(absolutize_path).transpose()
}

fn load_instructions(path: &Path) -> Result<(Vec<String>, Vec<Option<i64>>)> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read instructions from {}", path.display()))?;

    let instructions: Vec<String> = content
        .lines()
        .map(|line| strip_comment(line.trim(), '#'))
        .map(|line| strip_comment(line, ';'))
        .map(|line| strip_comment(line, '/'))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    let offsets = vec![None; instructions.len()];

    Ok((instructions, offsets))
}

fn strip_comment<'a>(line: &'a str, marker: char) -> &'a str {
    if line.is_empty() {
        return line;
    }

    if marker == '/' {
        if let Some(pos) = line.find("//") {
            return &line[..pos];
        }
        return line;
    }

    match line.find(marker) {
        Some(pos) => &line[..pos],
        None => line,
    }
}
