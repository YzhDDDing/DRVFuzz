use std::{collections::HashMap, path::PathBuf, time::Duration};
use thiserror::Error;

use crate::{
    error::GenerationError,
    execution_output::ExecutionContextOutput,
    riscv_impls::RiscVImpl,
    riscv_impls_vec::{GeneratedTestCase, TestCaseConfig},
    sd_model::TransitionAnalysis,
    transition_guidance::GuidanceStrategy,
};

#[derive(Debug, Clone)]
pub struct DiffAnalysisConfig {
    pub testcase_config: TestCaseConfig,
    pub run_root: PathBuf,
    pub max_iterations: Option<usize>,
    pub history_min_test_threshold: Option<usize>,
    pub impl_timeouts: HashMap<RiscVImpl, Duration>,
    pub emit_execution_output_json: bool,
    pub emit_execution_report_md: bool,
    pub cleanup_successful_iteration_artifacts: bool,
    pub cleanup_successful_diff_run: bool,
    pub guidance_strategy: Option<GuidanceStrategy>,
    pub transition_seed_pool_limit: usize,
    pub transition_seed_window: usize,
}

/// Diff-analysis settings that do not include the output directory.
#[derive(Debug, Clone)]
pub struct DiffAnalysisSettings {
    pub testcase_config: TestCaseConfig,
    pub max_iterations: Option<usize>,
    pub history_min_test_threshold: Option<usize>,
    pub impl_timeouts: HashMap<RiscVImpl, Duration>,
    pub emit_execution_output_json: bool,
    pub emit_execution_report_md: bool,
    pub cleanup_successful_iteration_artifacts: bool,
    pub cleanup_successful_diff_run: bool,
    pub guidance_strategy: Option<GuidanceStrategy>,
    pub transition_seed_pool_limit: usize,
    pub transition_seed_window: usize,
}

impl DiffAnalysisSettings {
    pub fn with_run_root(&self, run_root: PathBuf) -> DiffAnalysisConfig {
        DiffAnalysisConfig {
            testcase_config: self.testcase_config.clone(),
            run_root,
            max_iterations: self.max_iterations,
            history_min_test_threshold: self.history_min_test_threshold,
            impl_timeouts: self.impl_timeouts.clone(),
            emit_execution_output_json: self.emit_execution_output_json,
            emit_execution_report_md: self.emit_execution_report_md,
            cleanup_successful_iteration_artifacts: self.cleanup_successful_iteration_artifacts,
            cleanup_successful_diff_run: self.cleanup_successful_diff_run,
            guidance_strategy: self.guidance_strategy,
            transition_seed_pool_limit: self.transition_seed_pool_limit,
            transition_seed_window: self.transition_seed_window,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiffAnalysisResult {
    pub initial_testcase: GeneratedTestCase,
    pub final_testcase: GeneratedTestCase,
    pub final_outputs: HashMap<RiscVImpl, ExecutionContextOutput>,
    pub exception_removal_rounds: usize,
    pub write_removal_rounds: usize,
    pub output_root: PathBuf,
    pub transition_analysis: Option<TransitionAnalysis>,
}

#[derive(Debug, Error)]
pub enum DiffAnalysisError {
    #[error("testcase generation failed: {0}")]
    TestcaseGeneration(#[from] GenerationError),
    #[error("execution failed: {0}")]
    Execution(#[source] Box<dyn std::error::Error>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("report generation failed: {0}")]
    Report(#[source] Box<dyn std::error::Error>),
    #[error("missing instructions for implementation {impl_name}")]
    MissingInstructions { impl_name: String },
    #[error("test instruction count mismatch: {details}")]
    TestInstructionCountMismatch { details: String },
    #[error("history minimization failed: {reason}")]
    HistoryMinimizationFailure { reason: String },
    #[error("initialization mismatch at instruction {instruction_index}: {details}")]
    InitializationMismatch {
        instruction_index: usize,
        details: String,
    },
    #[error("maximum iteration limit {0} reached without convergence")]
    IterationLimitReached(usize),
}
