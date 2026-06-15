mod analysis_utils;
mod detection;
mod reporting;
mod runner;
mod types;
mod verification;
mod write_tags;

pub use runner::run_diff_analysis;
pub use runner::run_diff_analysis_with_testcase;
pub use runner::{LocalizationTiming, localization_probe_get, localization_probe_reset};
pub use types::{DiffAnalysisConfig, DiffAnalysisError, DiffAnalysisResult, DiffAnalysisSettings};

#[cfg(test)]
mod tests;
