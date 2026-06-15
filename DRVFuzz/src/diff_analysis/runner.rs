use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    execution_output::{ExecutionContextOutput, RegisterValue, generate_execution_context_report},
    instruction::{
        generate_memory_context_restore_instructions,
        generate_register_context_restore_instructions,
    },
    riscv_impls::RiscVImpl,
    riscv_impls_vec::{GeneratedTestCase, InstructionBlock, RiscVImplVec},
    sd_model::{
        TransitionAnalysis, analyze_transitions, write_transition_report_json,
        write_transition_report_md,
    },
};

use super::{
    analysis_utils::{canonicalize_memory_changes, determine_init_length, determine_test_length},
    detection::{
        ExceptionDetectionResult, WriteDifference, detect_unique_exceptions, detect_unique_writes,
    },
    reporting::{
        write_exception_cause_report, write_exception_report, write_execution_state_report,
        write_final_summary, write_history_candidate_report,
        write_history_minimization_error_report, write_history_minimization_report,
        write_initial_failure_report, write_initial_report, write_replay_write_failure_report,
        write_replay_write_report, write_write_report,
    },
    types::{DiffAnalysisConfig, DiffAnalysisError, DiffAnalysisResult},
    verification::{InitializationSummary, verify_initialization},
};

use once_cell::sync::Lazy;

const HISTORY_MIN_START_CANDIDATE_LOSS_REASON: &str =
    "History-min start candidate did not retain the target instruction write difference";

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalizationTiming {
    pub first_exception_ms: Option<f64>,
    pub first_write_ms: Option<f64>,
}

static LOCALIZATION_PROBE: Lazy<Mutex<Option<(Instant, LocalizationTiming)>>> =
    Lazy::new(|| Mutex::new(None));

pub fn localization_probe_reset(start: Instant) {
    let mut guard = LOCALIZATION_PROBE
        .lock()
        .expect("localization probe mutex poisoned");
    *guard = Some((start, LocalizationTiming::default()));
}

pub fn localization_probe_get() -> Option<LocalizationTiming> {
    let guard = LOCALIZATION_PROBE
        .lock()
        .expect("localization probe mutex poisoned");
    guard.as_ref().map(|(_, timing)| *timing)
}

fn record_first_exception_localization() {
    let now = Instant::now();
    let mut guard = LOCALIZATION_PROBE
        .lock()
        .expect("localization probe mutex poisoned");
    if let Some((start, ref mut timing)) = *guard {
        if timing.first_exception_ms.is_none() {
            timing.first_exception_ms = Some((now - start).as_secs_f64() * 1e3);
        }
    }
}

fn record_first_write_localization() {
    let now = Instant::now();
    let mut guard = LOCALIZATION_PROBE
        .lock()
        .expect("localization probe mutex poisoned");
    if let Some((start, ref mut timing)) = *guard {
        if timing.first_write_ms.is_none() {
            timing.first_write_ms = Some((now - start).as_secs_f64() * 1e3);
        }
    }
}

#[derive(Clone)]
struct GroupedWriteDifference {
    difference: WriteDifference,
    impl_group: Vec<RiscVImpl>,
}

fn resolve_unaligned_support_map(
    impl_order: &[RiscVImpl],
    requirement: Option<bool>,
) -> HashMap<RiscVImpl, bool> {
    impl_order
        .iter()
        .map(|impl_ref| {
            let support = if let Some(required) = requirement {
                assert!(
                    impl_ref.supports_unaligned_requirement(required, &HashMap::new()),
                    "implementation {impl_ref:?} does not satisfy the configured unaligned requirement",
                );
                required
            } else {
                impl_ref.default_unaligned_access_support()
            };
            (*impl_ref, support)
        })
        .collect()
}

fn build_alignment_groups(
    impl_order: &[RiscVImpl],
    support_map: &HashMap<RiscVImpl, bool>,
) -> Vec<Vec<RiscVImpl>> {
    let mut groups: Vec<(bool, Vec<RiscVImpl>)> = Vec::new();
    for impl_ref in impl_order {
        let support = *support_map
            .get(impl_ref)
            .expect("unaligned support map missing implementation entry");
        if let Some((_, members)) = groups.iter_mut().find(|(key, _)| *key == support) {
            members.push(*impl_ref);
        } else {
            groups.push((support, vec![*impl_ref]));
        }
    }
    groups.into_iter().map(|(_, members)| members).collect()
}

fn collect_exception_detections(
    impl_groups: &[Vec<RiscVImpl>],
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
) -> Result<ExceptionDetectionResult, DiffAnalysisError> {
    let mut combined = ExceptionDetectionResult::default();
    for group in impl_groups {
        if group.len() < 2 {
            continue;
        }
        let detection = detect_unique_exceptions(group, testcase, outputs)?;
        combined
            .trigger_mismatches
            .extend(detection.trigger_mismatches);
        combined.cause_mismatches.extend(detection.cause_mismatches);
    }
    combined
        .trigger_mismatches
        .sort_by(|a, b| a.test_index.cmp(&b.test_index));
    combined
        .cause_mismatches
        .sort_by(|a, b| a.test_index.cmp(&b.test_index));
    Ok(combined)
}

fn collect_grouped_write_differences(
    impl_groups: &[Vec<RiscVImpl>],
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
) -> Result<Vec<GroupedWriteDifference>, DiffAnalysisError> {
    let mut grouped = Vec::new();
    for group in impl_groups {
        if group.len() < 2 {
            continue;
        }
        let differences = detect_unique_writes(group, testcase, outputs)?;
        grouped.extend(
            differences
                .into_iter()
                .map(|difference| GroupedWriteDifference {
                    difference,
                    impl_group: group.clone(),
                }),
        );
    }
    grouped.sort_by(|a, b| {
        a.difference
            .test_index
            .cmp(&b.difference.test_index)
            .then_with(|| a.impl_group.cmp(&b.impl_group))
    });
    Ok(grouped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_support_map_reflects_requirement() {
        let impl_order = vec![RiscVImpl::Rocket, RiscVImpl::Vex, RiscVImpl::Spike];

        let support = resolve_unaligned_support_map(&impl_order, Some(false));
        for impl_ref in &impl_order {
            assert_eq!(support.get(impl_ref), Some(&false));
        }

        let support_true = resolve_unaligned_support_map(&[RiscVImpl::Spike], Some(true));
        assert_eq!(support_true.get(&RiscVImpl::Spike), Some(&true));
    }

    #[test]
    fn resolve_support_map_uses_defaults_when_unspecified() {
        let impl_order = vec![RiscVImpl::Rocket, RiscVImpl::Spike, RiscVImpl::XiangShan];

        let support = resolve_unaligned_support_map(&impl_order, None);
        assert_eq!(support.get(&RiscVImpl::Rocket), Some(&false));
        assert_eq!(support.get(&RiscVImpl::Spike), Some(&true));
        assert_eq!(support.get(&RiscVImpl::XiangShan), Some(&true));
    }

    #[test]
    fn alignment_groups_preserve_impl_order() {
        let impl_order = vec![RiscVImpl::Rocket, RiscVImpl::Spike, RiscVImpl::Vex];
        let support = resolve_unaligned_support_map(&impl_order, None);
        let groups = build_alignment_groups(&impl_order, &support);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], impl_order);
    }
}

pub fn run_diff_analysis(
    riscv_impls: &RiscVImplVec,
    config: DiffAnalysisConfig,
) -> Result<DiffAnalysisResult, DiffAnalysisError> {
    let initial_testcase = riscv_impls.generate_random_testcase(config.testcase_config.clone())?;
    run_diff_analysis_with_testcase(riscv_impls, config, initial_testcase)
}

pub fn run_diff_analysis_with_testcase(
    riscv_impls: &RiscVImplVec,
    config: DiffAnalysisConfig,
    initial_testcase: GeneratedTestCase,
) -> Result<DiffAnalysisResult, DiffAnalysisError> {
    fs::create_dir_all(&config.run_root)?;

    let impl_order: Vec<RiscVImpl> = riscv_impls.iter().copied().collect();
    if impl_order.is_empty() {
        return Err(DiffAnalysisError::TestInstructionCountMismatch {
            details: "no RISC-V implementations available".to_string(),
        });
    }

    let unaligned_support = resolve_unaligned_support_map(
        &impl_order,
        config.testcase_config.unaligned_access_required,
    );
    let impl_groups = build_alignment_groups(&impl_order, &unaligned_support);

    let impl_timeouts = if config.impl_timeouts.is_empty() {
        None
    } else {
        Some(&config.impl_timeouts)
    };

    let mut testcase = initial_testcase.clone();

    let mut iteration_index = 0usize;
    let mut iterations_executed = 0usize;
    let mut report_counters: HashMap<usize, usize> = HashMap::new();
    let emit_json = config.emit_execution_output_json;
    let emit_markdown = config.emit_execution_report_md;

    let mut iteration_dir = iteration_directory(&config.run_root, iteration_index);
    let mut outputs = run_iteration(
        riscv_impls,
        &testcase,
        &impl_order,
        &iteration_dir,
        impl_timeouts,
        emit_json,
        emit_markdown,
    )?;
    iterations_executed += 1;

    let mut init_summary: Option<InitializationSummary> = None;
    for group in &impl_groups {
        match verify_initialization(group, &testcase, &outputs) {
            Ok(summary) => {
                if init_summary.is_none() {
                    init_summary = Some(summary);
                }
            }
            Err(mismatch) => {
                let report_path = next_report_path(
                    &mut report_counters,
                    iteration_index,
                    &iteration_dir,
                    "initial_failure",
                );
                write_initial_failure_report(&report_path, &testcase, group, &mismatch)?;
                return Err(DiffAnalysisError::InitializationMismatch {
                    instruction_index: mismatch.instruction_index,
                    details: mismatch.reason.clone(),
                });
            }
        }
    }

    let success_summary = init_summary.unwrap_or(InitializationSummary {
        init_instruction_count: 0,
    });
    let report_path = next_report_path(
        &mut report_counters,
        iteration_index,
        &iteration_dir,
        "initial",
    );
    write_initial_report(&report_path, &success_summary)?;

    let summary_path = iteration_summary_path(&iteration_dir);
    write_execution_state_report(&summary_path, &testcase, &outputs, &impl_order)?;

    let transition_analysis =
        build_transition_analysis(&config, &initial_testcase, &outputs, &impl_order)?;

    let mut exception_round = 0usize;
    let mut write_round = 0usize;

    loop {
        let exception_detections = collect_exception_detections(&impl_groups, &testcase, &outputs)?;
        if !exception_detections.trigger_mismatches.is_empty() {
            let removal_indices: BTreeSet<usize> = exception_detections
                .trigger_mismatches
                .iter()
                .map(|diff| diff.test_index)
                .collect();
            exception_round += 1;

            let report_path = next_report_path(
                &mut report_counters,
                iteration_index,
                &iteration_dir,
                "exception_removal",
            );
            let previous_testcase = testcase.clone();
            let trimmed_testcase = previous_testcase.without_test_indices(&removal_indices);
            write_exception_report(
                &report_path,
                &previous_testcase,
                &trimmed_testcase,
                &exception_detections.trigger_mismatches,
                &impl_order,
            )?;
            testcase = trimmed_testcase;

            if let Some(limit) = config.max_iterations {
                if iterations_executed >= limit {
                    return Err(DiffAnalysisError::IterationLimitReached(limit));
                }
            }
            iteration_index += 1;
            iteration_dir = iteration_directory(&config.run_root, iteration_index);
            outputs = run_iteration(
                riscv_impls,
                &testcase,
                &impl_order,
                &iteration_dir,
                impl_timeouts,
                emit_json,
                emit_markdown,
            )?;
            iterations_executed += 1;

            let summary_path = iteration_summary_path(&iteration_dir);
            write_execution_state_report(&summary_path, &testcase, &outputs, &impl_order)?;

            if testcase
                .test_insts
                .values()
                .next()
                .map(|block| block.lines().is_empty())
                .unwrap_or(true)
            {
                break;
            }

            record_first_exception_localization();

            continue;
        } else if !exception_detections.cause_mismatches.is_empty() {
            let report_path = next_report_path(
                &mut report_counters,
                iteration_index,
                &iteration_dir,
                "exception_cause_diff",
            );
            write_exception_cause_report(
                &report_path,
                &testcase,
                &exception_detections.cause_mismatches,
                &impl_order,
            )?;
        }

        let write_differences =
            collect_grouped_write_differences(&impl_groups, &testcase, &outputs)?;
        if write_differences.is_empty() {
            if config.cleanup_successful_iteration_artifacts {
                cleanup_successful_iteration_outputs(&iteration_dir, &impl_order)
                    .map_err(DiffAnalysisError::Io)?;
            }
            break;
        }
        let GroupedWriteDifference {
            difference: first_difference,
            impl_group,
        } = write_differences
            .into_iter()
            .next()
            .expect("write_differences is non-empty; qed");
        let cause = classify_write_difference(
            &first_difference,
            &testcase,
            &outputs,
            riscv_impls,
            &impl_group,
            &iteration_dir,
            write_round + 1,
            config.history_min_test_threshold,
            impl_timeouts,
            emit_json,
            emit_markdown,
            config.cleanup_successful_iteration_artifacts,
        )?;
        let cause_label = cause.description();
        let cause_description = format!("Difference source: {}", cause_label);
        let diff_report = vec![(first_difference.clone(), cause_description)];

        let mut removal_indices = BTreeSet::new();
        match cause {
            WriteDifferenceCause::SingleInstruction => {
                removal_indices.insert(first_difference.test_index);
            }
            WriteDifferenceCause::DependsOnHistory => {
                for idx in 0..=first_difference.test_index {
                    removal_indices.insert(idx);
                }
            }
        }
        write_round += 1;
        let report_path = next_report_path(
            &mut report_counters,
            iteration_index,
            &iteration_dir,
            "write_removal",
        );

        let previous_testcase = testcase.clone();
        let trimmed_testcase = previous_testcase.without_test_indices(&removal_indices);
        write_write_report(
            &report_path,
            &previous_testcase,
            &trimmed_testcase,
            &diff_report,
            &impl_group,
        )?;
        testcase = trimmed_testcase;

        if let Some(limit) = config.max_iterations {
            if iterations_executed >= limit {
                return Err(DiffAnalysisError::IterationLimitReached(limit));
            }
        }
        iteration_index += 1;
        iteration_dir = iteration_directory(&config.run_root, iteration_index);
        outputs = run_iteration(
            riscv_impls,
            &testcase,
            &impl_order,
            &iteration_dir,
            impl_timeouts,
            emit_json,
            emit_markdown,
        )?;
        iterations_executed += 1;

        let summary_path = iteration_summary_path(&iteration_dir);
        write_execution_state_report(&summary_path, &testcase, &outputs, &impl_order)?;

        if testcase
            .test_insts
            .values()
            .next()
            .map(|block| block.lines().is_empty())
            .unwrap_or(true)
        {
            break;
        }
    }

    let final_summary_path = config.run_root.join("final_report.md");
    write_final_summary(
        &final_summary_path,
        &initial_testcase,
        &testcase,
        exception_round,
        write_round,
    )?;

    let result = DiffAnalysisResult {
        initial_testcase,
        final_testcase: testcase.clone(),
        final_outputs: outputs.clone(),
        exception_removal_rounds: exception_round,
        write_removal_rounds: write_round,
        output_root: config.run_root.clone(),
        transition_analysis,
    };

    if config.cleanup_successful_diff_run && exception_round == 0 && write_round == 0 {
        match fs::remove_dir_all(&config.run_root) {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(DiffAnalysisError::Io(err)),
        }
    }

    Ok(result)
}

fn build_transition_analysis(
    config: &DiffAnalysisConfig,
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    impl_order: &[RiscVImpl],
) -> Result<Option<TransitionAnalysis>, DiffAnalysisError> {
    if config.guidance_strategy.is_none() {
        return Ok(None);
    }

    let reference_impl = select_transition_reference_impl(impl_order);
    let output = outputs.get(&reference_impl).ok_or_else(|| {
        DiffAnalysisError::TestInstructionCountMismatch {
            details: format!("missing execution output for {reference_impl}"),
        }
    })?;
    let instructions = testcase.combined_insts_of(&reference_impl).ok_or_else(|| {
        DiffAnalysisError::MissingInstructions {
            impl_name: reference_impl.to_string(),
        }
    })?;

    let analysis = analyze_transitions(output, &instructions);
    write_transition_report_json(&config.run_root.join("transition_report.json"), &analysis)
        .map_err(DiffAnalysisError::Report)?;
    write_transition_report_md(
        &config.run_root.join("transition_report.md"),
        &analysis,
        &instructions,
    )
    .map_err(DiffAnalysisError::Report)?;

    Ok(Some(analysis))
}

fn select_transition_reference_impl(impl_order: &[RiscVImpl]) -> RiscVImpl {
    impl_order
        .iter()
        .copied()
        .find(|impl_ref| *impl_ref == RiscVImpl::Spike)
        .unwrap_or(impl_order[0])
}

#[derive(Debug, Clone, Copy)]
enum WriteDifferenceCause {
    SingleInstruction,
    DependsOnHistory,
}

impl WriteDifferenceCause {
    fn description(self) -> &'static str {
        match self {
            WriteDifferenceCause::SingleInstruction => {
                "Single test instruction causes the difference"
            }
            WriteDifferenceCause::DependsOnHistory => {
                "History instructions together with the current instruction cause the difference"
            }
        }
    }
}

fn is_history_start_candidate_failure(err: &DiffAnalysisError) -> bool {
    matches!(
        err,
        DiffAnalysisError::HistoryMinimizationFailure { reason }
            if reason == HISTORY_MIN_START_CANDIDATE_LOSS_REASON
    )
}

#[derive(Clone)]
struct HistoryEvaluation {
    start_index: usize,
    status: HistoryCandidateStatus,
    difference: Option<WriteDifference>,
    testcase: GeneratedTestCase,
    outputs: HashMap<RiscVImpl, ExecutionContextOutput>,
}

#[derive(Clone)]
enum HistoryCandidateStatus {
    TargetHasDifference,
    TargetExceptionMismatch,
    TargetExceptionAll,
    TargetNoDifference,
    NonTargetDifference { test_index: usize },
    NonTargetException { test_index: usize },
}

fn classify_write_difference(
    diff: &WriteDifference,
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    riscv_impls: &RiscVImplVec,
    impl_order: &[RiscVImpl],
    iteration_dir: &Path,
    verification_round: usize,
    history_min_threshold: Option<usize>,
    impl_timeouts: Option<&HashMap<RiscVImpl, Duration>>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
    cleanup_history_candidates: bool,
) -> Result<WriteDifferenceCause, DiffAnalysisError> {
    let replay_dir = iteration_dir.join(format!("write_replay_{:03}", verification_round));
    fs::create_dir_all(&replay_dir)?;
    let replay_report_path = replay_dir.join("report_01_write_replay.md");

    let (replay_testcase, target_index) =
        match build_replay_testcase(diff, testcase, outputs, impl_order, &replay_dir) {
            Ok(value) => value,
            Err(err) => {
                write_replay_write_failure_report(
                    &replay_report_path,
                    testcase,
                    diff,
                    impl_order,
                    diff.test_index,
                    &format!("Write replay construction failed: {err}"),
                )?;
                return Ok(WriteDifferenceCause::SingleInstruction);
            }
        };

    let replay_outputs = match run_iteration(
        riscv_impls,
        &replay_testcase,
        impl_order,
        &replay_dir,
        impl_timeouts,
        emit_execution_output_json,
        emit_execution_report_md,
    ) {
        Ok(outputs) => outputs,
        Err(err) => {
            write_replay_write_failure_report(
                &replay_report_path,
                testcase,
                diff,
                impl_order,
                target_index,
                &format!("Write replay execution failed: {err}"),
            )?;
            return Ok(WriteDifferenceCause::SingleInstruction);
        }
    };

    let summary_path = iteration_summary_path(&replay_dir);
    if let Err(err) =
        write_execution_state_report(&summary_path, &replay_testcase, &replay_outputs, impl_order)
    {
        write_replay_write_failure_report(
            &replay_report_path,
            testcase,
            diff,
            impl_order,
            target_index,
            &format!("Failed to generate write replay summary report: {err}"),
        )?;
        return Ok(WriteDifferenceCause::SingleInstruction);
    }

    let replay_differences =
        match detect_unique_writes(impl_order, &replay_testcase, &replay_outputs) {
            Ok(differences) => differences,
            Err(err) => {
                write_replay_write_failure_report(
                    &replay_report_path,
                    testcase,
                    diff,
                    impl_order,
                    target_index,
                    &format!("Write replay difference detection failed: {err}"),
                )?;
                return Ok(WriteDifferenceCause::SingleInstruction);
            }
        };
    let mut first_difference: Option<WriteDifference> = None;
    let mut target_difference: Option<WriteDifference> = None;
    for diff_entry in replay_differences.into_iter() {
        if first_difference.is_none() {
            first_difference = Some(diff_entry.clone());
        }
        if diff_entry.test_index == target_index && target_difference.is_none() {
            target_difference = Some(diff_entry.clone());
        }
    }

    if let Some(first) = &first_difference {
        if first.test_index != target_index {
            let failure_message = format!(
                "Write replay failed: first difference appeared at test instruction #{}, expected #{}.",
                first.test_index, target_index
            );
            let replay_report_path = replay_dir.join("report_01_write_replay.md");
            write_replay_write_failure_report(
                &replay_report_path,
                &replay_testcase,
                first,
                impl_order,
                target_index,
                &failure_message,
            )?;
            return Ok(WriteDifferenceCause::SingleInstruction);
        }
    }

    let difference_persists = target_difference.is_some();
    let mut cause = if difference_persists {
        WriteDifferenceCause::SingleInstruction
    } else {
        WriteDifferenceCause::DependsOnHistory
    };

    let report_difference = if let Some(diff_entry) = target_difference {
        diff_entry
    } else {
        build_replay_difference_snapshot(
            &replay_testcase,
            &replay_outputs,
            impl_order,
            target_index,
        )?
    };

    if matches!(cause, WriteDifferenceCause::DependsOnHistory) {
        if let Err(err) = minimize_history_write_difference(
            diff,
            testcase,
            riscv_impls,
            impl_order,
            iteration_dir,
            verification_round,
            history_min_threshold,
            impl_timeouts,
            emit_execution_output_json,
            emit_execution_report_md,
            cleanup_history_candidates,
        ) {
            let minimize_root =
                iteration_dir.join(format!("write_history_min_{:03}", verification_round));
            fs::create_dir_all(&minimize_root)?;
            let error_report = minimize_root.join("report_00_history_min_error.md");
            write_history_minimization_error_report(&error_report, diff, &err.to_string())?;
            if is_history_start_candidate_failure(&err) {
                cause = WriteDifferenceCause::SingleInstruction;
            }
        }
    }

    let cause_detail = cause.description();
    let replay_report_path = replay_dir.join("report_01_write_replay.md");
    write_replay_write_report(
        &replay_report_path,
        &replay_testcase,
        &report_difference,
        impl_order,
        cause_detail,
    )?;

    record_first_write_localization();

    Ok(cause)
}

fn minimize_history_write_difference(
    diff: &WriteDifference,
    testcase: &GeneratedTestCase,
    riscv_impls: &RiscVImplVec,
    impl_order: &[RiscVImpl],
    iteration_dir: &Path,
    verification_round: usize,
    history_min_threshold: Option<usize>,
    impl_timeouts: Option<&HashMap<RiscVImpl, Duration>>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
    cleanup_history_candidates: bool,
) -> Result<(), DiffAnalysisError> {
    if diff.test_index == 0 {
        // No history instructions can be trimmed
        return Ok(());
    }

    if let Some(threshold) = history_min_threshold {
        let current_len = determine_test_length(impl_order, testcase)?;
        if current_len <= threshold {
            return Ok(());
        }
    }

    let target_index = diff.test_index;
    let minimize_root = iteration_dir.join(format!("write_history_min_{:03}", verification_round));
    fs::create_dir_all(&minimize_root)?;

    let mut cache: HashMap<usize, HistoryEvaluation> = HashMap::new();
    let mut evaluated_starts = BTreeSet::new();

    let initial_eval = evaluate_history_candidate(
        riscv_impls,
        testcase,
        impl_order,
        &minimize_root,
        0,
        target_index,
        impl_timeouts,
        emit_execution_output_json,
        emit_execution_report_md,
    )?;
    cache.insert(0, initial_eval.clone());
    evaluated_starts.insert(0);
    let mut best_start = 0usize;
    let mut best_eval = initial_eval.clone();

    match &initial_eval.status {
        HistoryCandidateStatus::TargetHasDifference => { /* continue searching */ }
        HistoryCandidateStatus::TargetExceptionMismatch
        | HistoryCandidateStatus::TargetExceptionAll => {
            return conclude_history_minimization(
                diff,
                &best_eval,
                best_start,
                target_index,
                impl_order,
                &minimize_root,
                emit_execution_output_json,
                emit_execution_report_md,
                cleanup_history_candidates,
                &evaluated_starts,
            );
        }
        HistoryCandidateStatus::TargetNoDifference => {
            return Err(DiffAnalysisError::HistoryMinimizationFailure {
                reason: "History-min start candidate did not retain the target instruction write difference".to_string(),
            });
        }
        HistoryCandidateStatus::NonTargetDifference { test_index } => {
            let original_index = initial_eval.start_index + test_index;
            return Err(DiffAnalysisError::HistoryMinimizationFailure {
                reason: format!(
                    "History-min start candidate still has write difference at instruction #{}",
                    original_index
                ),
            });
        }
        HistoryCandidateStatus::NonTargetException { test_index } => {
            let original_index = initial_eval.start_index + test_index;
            return Err(DiffAnalysisError::HistoryMinimizationFailure {
                reason: format!(
                    "History-min start candidate has an exception difference at instruction #{}",
                    original_index
                ),
            });
        }
    }

    let mut good = 0usize;
    let mut bad = target_index + 1usize;

    while good + 1 < bad {
        let mid = (good + bad) / 2;
        let eval = if let Some(existing) = cache.get(&mid) {
            evaluated_starts.insert(mid);
            existing.clone()
        } else {
            let evaluated = evaluate_history_candidate(
                riscv_impls,
                testcase,
                impl_order,
                &minimize_root,
                mid,
                target_index,
                impl_timeouts,
                emit_execution_output_json,
                emit_execution_report_md,
            )?;
            cache.insert(mid, evaluated.clone());
            evaluated_starts.insert(mid);
            evaluated
        };

        match &eval.status {
            HistoryCandidateStatus::TargetHasDifference => {
                good = mid;
                best_start = mid;
                best_eval = eval.clone();
            }
            HistoryCandidateStatus::TargetNoDifference => {
                bad = mid;
            }
            HistoryCandidateStatus::TargetExceptionMismatch
            | HistoryCandidateStatus::TargetExceptionAll => {
                if eval.difference.is_some() {
                    best_start = mid;
                    best_eval = eval.clone();
                }
                break;
            }
            HistoryCandidateStatus::NonTargetDifference { test_index } => {
                if best_eval.difference.is_some() {
                    return conclude_history_minimization(
                        diff,
                        &best_eval,
                        best_start,
                        target_index,
                        impl_order,
                        &minimize_root,
                        emit_execution_output_json,
                        emit_execution_report_md,
                        cleanup_history_candidates,
                        &evaluated_starts,
                    );
                } else {
                    let original_index = eval.start_index + test_index;
                    return Err(DiffAnalysisError::HistoryMinimizationFailure {
                        reason: format!(
                            "History minimization failed: instruction #{} shows extra write difference",
                            original_index
                        ),
                    });
                }
            }
            HistoryCandidateStatus::NonTargetException { test_index } => {
                if best_eval.difference.is_some() {
                    return conclude_history_minimization(
                        diff,
                        &best_eval,
                        best_start,
                        target_index,
                        impl_order,
                        &minimize_root,
                        emit_execution_output_json,
                        emit_execution_report_md,
                        cleanup_history_candidates,
                        &evaluated_starts,
                    );
                } else {
                    let original_index = eval.start_index + test_index;
                    return Err(DiffAnalysisError::HistoryMinimizationFailure {
                        reason: format!(
                            "History minimization failed: instruction #{} shows an exception difference",
                            original_index
                        ),
                    });
                }
            }
        }
    }

    if best_eval.difference.is_none() {
        return Err(DiffAnalysisError::HistoryMinimizationFailure {
            reason: "Unable to locate minimized result for the target instruction write difference"
                .to_string(),
        });
    }

    conclude_history_minimization(
        diff,
        &best_eval,
        best_start,
        target_index,
        impl_order,
        &minimize_root,
        emit_execution_output_json,
        emit_execution_report_md,
        cleanup_history_candidates,
        &evaluated_starts,
    )
}

fn finalize_history_minimization(
    diff: &WriteDifference,
    best_eval: &HistoryEvaluation,
    best_start: usize,
    target_index: usize,
    impl_order: &[RiscVImpl],
    minimize_root: &Path,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
) -> Result<(), DiffAnalysisError> {
    let minimized_dir = minimize_root.join("final");
    fs::create_dir_all(&minimized_dir)?;

    let testcase_file = File::create(minimized_dir.join("testcase.json"))?;
    serde_json::to_writer_pretty(testcase_file, &best_eval.testcase)?;

    for (impl_ref, output) in &best_eval.outputs {
        let impl_dir = minimized_dir.join(impl_ref.to_string());
        fs::create_dir_all(&impl_dir)?;
        if emit_execution_output_json {
            let output_file = File::create(impl_dir.join("execution_output.json"))?;
            serde_json::to_writer_pretty(output_file, output)?;
        }
        if emit_execution_report_md {
            let instructions = best_eval
                .testcase
                .combined_insts_of(impl_ref)
                .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                    impl_name: impl_ref.to_string(),
                })?;
            generate_execution_context_report(
                output,
                impl_dir.join("execution_report.md"),
                &instructions,
            )
            .map_err(|err| DiffAnalysisError::Report(err))?;
        }
    }

    let report_path = minimize_root.join("report_history_minimization.md");
    write_history_minimization_report(
        &report_path,
        diff,
        best_start,
        target_index,
        &best_eval.testcase,
        &best_eval.outputs,
        impl_order,
    )?;

    Ok(())
}

fn conclude_history_minimization(
    diff: &WriteDifference,
    best_eval: &HistoryEvaluation,
    best_start: usize,
    target_index: usize,
    impl_order: &[RiscVImpl],
    minimize_root: &Path,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
    cleanup_history_candidates: bool,
    evaluated_starts: &BTreeSet<usize>,
) -> Result<(), DiffAnalysisError> {
    finalize_history_minimization(
        diff,
        best_eval,
        best_start,
        target_index,
        impl_order,
        minimize_root,
        emit_execution_output_json,
        emit_execution_report_md,
    )?;

    if cleanup_history_candidates {
        cleanup_history_minimization_attempts(minimize_root, evaluated_starts, best_start)
            .map_err(DiffAnalysisError::Io)?;
    }

    Ok(())
}

fn cleanup_history_minimization_attempts(
    minimize_root: &Path,
    evaluated_starts: &BTreeSet<usize>,
    best_start: usize,
) -> std::io::Result<()> {
    for start in evaluated_starts {
        if *start == best_start {
            continue;
        }
        let candidate_dir = minimize_root.join(format!("candidate_{:03}", start));
        match fs::remove_dir_all(&candidate_dir) {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn evaluate_history_candidate(
    riscv_impls: &RiscVImplVec,
    testcase: &GeneratedTestCase,
    impl_order: &[RiscVImpl],
    minimize_root: &Path,
    start_index: usize,
    target_index: usize,
    impl_timeouts: Option<&HashMap<RiscVImpl, Duration>>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
) -> Result<HistoryEvaluation, DiffAnalysisError> {
    if start_index > target_index {
        return Err(DiffAnalysisError::TestInstructionCountMismatch {
            details: format!(
                "invalid history minimization range: start {} > target {}",
                start_index, target_index
            ),
        });
    }

    let trimmed = build_history_trimmed_testcase(testcase, impl_order, start_index, target_index)?;
    let candidate_dir = minimize_root.join(format!("candidate_{:03}", start_index));
    let outputs = run_iteration(
        riscv_impls,
        &trimmed,
        impl_order,
        &candidate_dir,
        impl_timeouts,
        emit_execution_output_json,
        emit_execution_report_md,
    )?;
    let summary_path = iteration_summary_path(&candidate_dir);
    write_execution_state_report(&summary_path, &trimmed, &outputs, impl_order)?;
    let new_target = target_index.checked_sub(start_index).ok_or_else(|| {
        DiffAnalysisError::TestInstructionCountMismatch {
            details: format!(
                "history minimization computed negative target: start={}, target={}",
                start_index, target_index
            ),
        }
    })?;
    let differences = detect_unique_writes(impl_order, &trimmed, &outputs)?;
    let mut target_difference: Option<WriteDifference> = None;
    let mut other_difference: Option<WriteDifference> = None;
    for diff in &differences {
        if diff.test_index == new_target && target_difference.is_none() {
            target_difference = Some(diff.clone());
        } else if other_difference.is_none() {
            other_difference = Some(diff.clone());
        }
    }

    let exceptions = detect_unique_exceptions(impl_order, &trimmed, &outputs)?;
    let mut target_exception_mismatch = false;
    let mut other_exception_index: Option<usize> = None;
    for exc in exceptions
        .trigger_mismatches
        .iter()
        .chain(exceptions.cause_mismatches.iter())
    {
        if exc.test_index == new_target {
            target_exception_mismatch = true;
        } else if other_exception_index.is_none() {
            other_exception_index = Some(exc.test_index);
        }
    }

    let init_len = determine_init_length(impl_order, &trimmed)?;
    let global_target = init_len + new_target;
    let mut target_exception_count = 0usize;
    for impl_ref in impl_order {
        let output =
            outputs
                .get(impl_ref)
                .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                    impl_name: impl_ref.to_string(),
                })?;
        if output
            .exceptions
            .iter()
            .any(|exc| exc.user_instruction_index == global_target)
        {
            target_exception_count += 1;
        }
    }
    let target_exception_all = target_exception_count == impl_order.len();

    let status = if let Some(diff) = other_difference.as_ref() {
        HistoryCandidateStatus::NonTargetDifference {
            test_index: diff.test_index,
        }
    } else if let Some(idx) = other_exception_index {
        HistoryCandidateStatus::NonTargetException { test_index: idx }
    } else if target_difference.is_some() {
        if target_exception_mismatch {
            HistoryCandidateStatus::TargetExceptionMismatch
        } else if target_exception_all {
            HistoryCandidateStatus::TargetExceptionAll
        } else {
            HistoryCandidateStatus::TargetHasDifference
        }
    } else {
        HistoryCandidateStatus::TargetNoDifference
    };

    let cause_detail = match &status {
        HistoryCandidateStatus::TargetHasDifference => {
            "Difference source: history instructions and current instruction jointly cause the difference".to_string()
        }
        HistoryCandidateStatus::TargetExceptionMismatch => {
            "Target instruction triggers exceptions inconsistently across implementations; stop minimization.".to_string()
        }
        HistoryCandidateStatus::TargetExceptionAll => {
            "Target instruction triggers exceptions on all implementations; stop minimization.".to_string()
        }
        HistoryCandidateStatus::TargetNoDifference => {
            "Target instruction no longer shows write differences.".to_string()
        }
        HistoryCandidateStatus::NonTargetDifference { test_index } => {
            let original_index = start_index + test_index;
            format!(
                "Non-target instruction #{} shows a write difference; history minimization stopped.",
                original_index
            )
        }
        HistoryCandidateStatus::NonTargetException { test_index } => {
            let original_index = start_index + test_index;
            format!(
                "Non-target instruction #{} shows an exception difference; history minimization stopped.",
                original_index
            )
        }
    };

    let report_difference = match &status {
        HistoryCandidateStatus::TargetHasDifference
        | HistoryCandidateStatus::TargetExceptionMismatch
        | HistoryCandidateStatus::TargetExceptionAll => target_difference.as_ref(),
        HistoryCandidateStatus::NonTargetDifference { .. } => other_difference.as_ref(),
        _ => None,
    };

    let report_path = candidate_dir.join("report_01_history_candidate.md");
    write_history_candidate_report(
        &report_path,
        start_index,
        target_index,
        report_difference,
        &trimmed,
        impl_order,
        &cause_detail,
    )?;

    Ok(HistoryEvaluation {
        start_index,
        status,
        difference: target_difference,
        testcase: trimmed,
        outputs,
    })
}

fn build_history_trimmed_testcase(
    testcase: &GeneratedTestCase,
    impl_order: &[RiscVImpl],
    start_index: usize,
    target_index: usize,
) -> Result<GeneratedTestCase, DiffAnalysisError> {
    if start_index > target_index {
        return Err(DiffAnalysisError::TestInstructionCountMismatch {
            details: format!(
                "invalid history minimization range: start {} > target {}",
                start_index, target_index
            ),
        });
    }

    let mut test_insts = HashMap::new();
    for impl_ref in impl_order {
        let block = testcase.test_insts.get(impl_ref).ok_or_else(|| {
            DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            }
        })?;
        if target_index >= block.len() {
            return Err(DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "implementation {} has only {} test instructions, target index {} out of range",
                    impl_ref,
                    block.len(),
                    target_index
                ),
            });
        }
        let lines = block.lines()[start_index..=target_index].to_vec();
        let offsets = block.offsets()[start_index..=target_index].to_vec();
        test_insts.insert(*impl_ref, InstructionBlock::from_parts(lines, offsets));
    }

    Ok(GeneratedTestCase {
        config: testcase.config.clone(),
        init_insts: testcase.init_insts.clone(),
        test_insts,
        mem_range: testcase.mem_range.clone(),
        extension_map: testcase.extension_map.clone(),
    })
}

fn build_replay_testcase(
    diff: &WriteDifference,
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    impl_order: &[RiscVImpl],
    replay_dir: &Path,
) -> Result<(GeneratedTestCase, usize), DiffAnalysisError> {
    if impl_order.is_empty() {
        return Err(DiffAnalysisError::TestInstructionCountMismatch {
            details: "no implementations available for write difference classification".to_string(),
        });
    }

    let mut replay_test_insts = HashMap::new();
    let mut expected_len = None;
    let temp_range = testcase.config.temp_register_range;

    for impl_ref in impl_order {
        let output =
            outputs
                .get(impl_ref)
                .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                    impl_name: impl_ref.to_string(),
                })?;

        let context = output.contexts.get(diff.global_index).ok_or_else(|| {
            DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "missing context for instruction {} in {}",
                    diff.global_index, impl_ref
                ),
            }
        })?;

        let mut block = InstructionBlock::new();

        let memory_restore = generate_memory_context_restore_instructions(
            &context.memory_before,
            output.mem_range().0,
            temp_range,
        )?;
        for inst in memory_restore {
            block.push(inst, None);
        }

        let register_restore = generate_register_context_restore_instructions(
            &context.registers_before,
            output.isa_base,
            temp_range,
        )?;
        for inst in register_restore {
            block.push(inst, None);
        }

        let original_block = testcase.test_insts.get(impl_ref).ok_or_else(|| {
            DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "missing test instruction {} for implementation {}",
                    diff.test_index, impl_ref
                ),
            }
        })?;
        let original_inst = original_block
            .lines()
            .get(diff.test_index)
            .cloned()
            .ok_or_else(|| DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "missing test instruction {} for implementation {}",
                    diff.test_index, impl_ref
                ),
            })?;
        let original_offset = original_block
            .offsets()
            .get(diff.test_index)
            .copied()
            .unwrap_or(None);
        block.push(original_inst, original_offset);

        let len = block.len();
        replay_test_insts.insert(*impl_ref, block);
        if let Some(expected) = expected_len {
            if len != expected {
                let partial_testcase = GeneratedTestCase {
                    config: testcase.config.clone(),
                    init_insts: testcase.init_insts.clone(),
                    test_insts: replay_test_insts.clone(),
                    mem_range: testcase.mem_range.clone(),
                    extension_map: testcase.extension_map.clone(),
                };
                let partial_path = replay_dir.join("testcase.json");
                write_partial_replay_testcase(&partial_path, &partial_testcase)?;

                return Err(DiffAnalysisError::TestInstructionCountMismatch {
                    details: format!(
                        "write replay produced {} instructions for {}, expected {}; partial replay testcase saved to {}",
                        len,
                        impl_ref,
                        expected,
                        partial_path.display()
                    ),
                });
            }
        } else {
            expected_len = Some(len);
        }
    }

    let total_len =
        expected_len.ok_or_else(|| DiffAnalysisError::TestInstructionCountMismatch {
            details: "write replay produced no test instructions".to_string(),
        })?;

    let target_index = total_len.checked_sub(1).ok_or_else(|| {
        DiffAnalysisError::TestInstructionCountMismatch {
            details: "write replay produced empty instruction list".to_string(),
        }
    })?;

    let replay_testcase = GeneratedTestCase {
        config: testcase.config.clone(),
        init_insts: testcase.init_insts.clone(),
        test_insts: replay_test_insts,
        mem_range: testcase.mem_range.clone(),
        extension_map: testcase.extension_map.clone(),
    };

    Ok((replay_testcase, target_index))
}

fn write_partial_replay_testcase(
    path: &Path,
    testcase: &GeneratedTestCase,
) -> Result<(), DiffAnalysisError> {
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, testcase)?;
    Ok(())
}

fn build_replay_difference_snapshot(
    replay_testcase: &GeneratedTestCase,
    replay_outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    impl_order: &[RiscVImpl],
    target_index: usize,
) -> Result<WriteDifference, DiffAnalysisError> {
    let init_len = determine_init_length(impl_order, replay_testcase)?;
    let global_index = init_len + target_index;

    let mut register_map = BTreeMap::new();
    let mut memory_map = BTreeMap::new();
    let mut register_contexts = BTreeMap::new();
    let mut memory_contexts = BTreeMap::new();

    for impl_ref in impl_order {
        let output =
            replay_outputs
                .get(impl_ref)
                .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                    impl_name: impl_ref.to_string(),
                })?;

        let regs = output.register_changes.get(global_index).ok_or_else(|| {
            DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "write replay missing register changes at index {} for {}",
                    global_index, impl_ref
                ),
            }
        })?;
        let mems = output.memory_changes.get(global_index).ok_or_else(|| {
            DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "write replay missing memory changes at index {} for {}",
                    global_index, impl_ref
                ),
            }
        })?;

        let register_entries = collect_register_changes(regs);
        let canonical_mems = canonicalize_memory_changes(mems);
        register_map.insert(*impl_ref, register_entries);
        memory_map.insert(*impl_ref, canonical_mems);
        let instruction_ctx = output.contexts.get(global_index).ok_or_else(|| {
            DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "write replay missing execution context at index {} for {}",
                    global_index, impl_ref
                ),
            }
        })?;
        register_contexts.insert(*impl_ref, instruction_ctx.register_context.clone());
        memory_contexts.insert(*impl_ref, instruction_ctx.memory_context.clone());
    }

    Ok(WriteDifference {
        test_index: target_index,
        global_index,
        register_changes: register_map,
        memory_changes: memory_map,
        register_contexts,
        memory_contexts,
    })
}

fn collect_register_changes(changes: &[RegisterValue]) -> Vec<(String, u64)> {
    let mut entries = changes
        .iter()
        .map(|entry| (entry.name.clone(), entry.value))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn run_iteration(
    riscv_impls: &RiscVImplVec,
    testcase: &GeneratedTestCase,
    impl_order: &[RiscVImpl],
    iteration_dir: &Path,
    impl_timeouts: Option<&HashMap<RiscVImpl, Duration>>,
    emit_execution_output_json: bool,
    emit_execution_report_md: bool,
) -> Result<HashMap<RiscVImpl, ExecutionContextOutput>, DiffAnalysisError> {
    fs::create_dir_all(iteration_dir)?;

    let testcase_file = File::create(iteration_dir.join("testcase.json"))?;
    serde_json::to_writer_pretty(testcase_file, testcase)?;

    let outputs = riscv_impls
        .execute(testcase, iteration_dir, impl_timeouts)
        .map_err(|err| DiffAnalysisError::Execution(err))?;

    for impl_ref in impl_order {
        let ctx = outputs
            .get(impl_ref)
            .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            })?;
        let impl_dir = iteration_dir.join(impl_ref.to_string());
        fs::create_dir_all(&impl_dir)?;

        if emit_execution_output_json {
            let output_file = File::create(impl_dir.join("execution_output.json"))?;
            serde_json::to_writer_pretty(output_file, ctx)?;
        }

        if emit_execution_report_md {
            let instructions = testcase.combined_insts_of(impl_ref).ok_or_else(|| {
                DiffAnalysisError::MissingInstructions {
                    impl_name: impl_ref.to_string(),
                }
            })?;

            generate_execution_context_report(
                ctx,
                impl_dir.join("execution_report.md"),
                &instructions,
            )
            .map_err(|err| DiffAnalysisError::Report(err))?;
        }
    }

    Ok(outputs)
}

fn iteration_directory(run_root: &Path, iteration_index: usize) -> PathBuf {
    run_root.join(format!("iter_{:03}", iteration_index))
}

fn iteration_summary_path(iteration_dir: &Path) -> PathBuf {
    iteration_dir.join("report_00_iter_summary.md")
}

fn next_report_path(
    report_counters: &mut HashMap<usize, usize>,
    iteration_index: usize,
    iteration_dir: &Path,
    label: &str,
) -> PathBuf {
    let counter = report_counters.entry(iteration_index).or_insert(1);
    let filename = format!("report_{:02}_{}.md", counter, label);
    *counter += 1;
    iteration_dir.join(filename)
}

fn cleanup_successful_iteration_outputs(
    iteration_dir: &Path,
    impl_order: &[RiscVImpl],
) -> std::io::Result<()> {
    for impl_ref in impl_order {
        let impl_dir = iteration_dir.join(impl_ref.to_string());
        match fs::remove_dir_all(&impl_dir) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
}
