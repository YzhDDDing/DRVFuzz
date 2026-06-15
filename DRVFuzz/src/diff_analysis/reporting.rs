use std::{collections::HashMap, fmt::Write as _, fs, path::Path};

use crate::{
    execution_output::ExecutionContextOutput,
    riscv_impls::RiscVImpl,
    riscv_impls_vec::GeneratedTestCase,
    utils::{MemoryContext, MemoryValueWidth, RegisterContext},
};

use super::{
    analysis_utils::{determine_init_length, determine_test_length},
    detection::{ExceptionDifference, WriteDifference, detect_unique_writes},
    types::DiffAnalysisError,
    verification::{InitializationMismatchData, InitializationSummary},
    write_tags::classify_write_removal,
};

pub(crate) fn write_initial_report(
    path: &Path,
    summary: &InitializationSummary,
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    writeln!(
        &mut content,
        "# Initialization Verification Report\n\n- Initialization instruction count: {}\n- Register and memory writes are consistent across all implementations.",
        summary.init_instruction_count
    )
    .unwrap();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_initial_failure_report(
    path: &Path,
    testcase: &GeneratedTestCase,
    impl_order: &[RiscVImpl],
    mismatch: &InitializationMismatchData,
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    writeln!(
        &mut content,
        "# Initialization Verification Failed\n\n- Failing instruction index: {}\n- Reason: {}\n",
        mismatch.instruction_index, mismatch.reason
    )
    .unwrap();
    writeln!(
        &mut content,
        "| Implementation | Instruction | Register Writes | Memory Writes |"
    )
    .unwrap();
    writeln!(&mut content, "|------|------|------------|----------|").unwrap();

    for impl_ref in impl_order {
        let instruction = testcase
            .init_insts
            .get(impl_ref)
            .and_then(|block| block.lines().get(mismatch.instruction_index))
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        let regs = mismatch
            .per_impl_registers
            .get(impl_ref)
            .map(|entries| format_register_list(entries))
            .unwrap_or_else(|| "-".to_string());
        let mems = mismatch
            .per_impl_memory
            .get(impl_ref)
            .map(|entries| format_memory_list(entries))
            .unwrap_or_else(|| "-".to_string());
        writeln!(
            &mut content,
            "| {} | `{}` | {} | {} |",
            impl_ref, instruction, regs, mems
        )
        .unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_exception_report(
    path: &Path,
    previous_testcase: &GeneratedTestCase,
    current_testcase: &GeneratedTestCase,
    differences: &[ExceptionDifference],
    impl_order: &[RiscVImpl],
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    let old_len = determine_test_length(impl_order, previous_testcase)?;
    let new_len = determine_test_length(impl_order, current_testcase)?;

    writeln!(
        &mut content,
        "# Exception Difference Removal Report\n\n- Original test instruction count: {}\n- Removed: {}\n- Remaining test instruction count: {}\n",
        old_len,
        differences.len(),
        new_len
    )
    .unwrap();

    for diff in differences {
        writeln!(
            &mut content,
            "## Test Instruction #{} (global index {})\n",
            diff.test_index, diff.global_index
        )
        .unwrap();
        writeln!(
            &mut content,
            "| Implementation | Instruction | Register Context | Memory Context | Exception |\n|------|------|------------|------------|------|"
        )
        .unwrap();
        for impl_ref in impl_order {
            let inst = previous_testcase
                .test_insts
                .get(impl_ref)
                .and_then(|block| block.lines().get(diff.test_index))
                .cloned()
                .unwrap_or_else(|| "-".to_string());
            let register_context = diff
                .register_contexts
                .get(impl_ref)
                .map(|ctx| format_register_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let memory_context = diff
                .memory_contexts
                .get(impl_ref)
                .map(|ctx| format_memory_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let causes = diff
                .per_impl
                .get(impl_ref)
                .map(|c| {
                    if c.is_empty() {
                        "-".to_string()
                    } else {
                        c.join("; ")
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            writeln!(
                &mut content,
                "| {} | `{}` | {} | {} | {} |",
                impl_ref, inst, register_context, memory_context, causes
            )
            .unwrap();
        }
        writeln!(&mut content).unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_exception_cause_report(
    path: &Path,
    testcase: &GeneratedTestCase,
    differences: &[ExceptionDifference],
    impl_order: &[RiscVImpl],
) -> Result<(), DiffAnalysisError> {
    let test_len = determine_test_length(impl_order, testcase)?;
    let mut content = String::new();
    writeln!(
        &mut content,
        "# Exception Cause Difference Report\n\n- Test instruction count: {}\n- Exception-cause differences: {}\n",
        test_len,
        differences.len()
    )
    .unwrap();

    for diff in differences {
        writeln!(
            &mut content,
            "## Test Instruction #{} (global index {})\n",
            diff.test_index, diff.global_index
        )
        .unwrap();
        writeln!(
            &mut content,
            "| Implementation | Instruction | Register Context | Memory Context | Exception |\n|------|------|------------|------------|------|"
        )
        .unwrap();
        for impl_ref in impl_order {
            let inst = testcase
                .test_insts
                .get(impl_ref)
                .and_then(|block| block.lines().get(diff.test_index))
                .cloned()
                .unwrap_or_else(|| "-".to_string());
            let register_context = diff
                .register_contexts
                .get(impl_ref)
                .map(|ctx| format_register_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let memory_context = diff
                .memory_contexts
                .get(impl_ref)
                .map(|ctx| format_memory_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let causes = diff
                .per_impl
                .get(impl_ref)
                .map(|c| {
                    if c.is_empty() {
                        "-".to_string()
                    } else {
                        c.join("; ")
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            writeln!(
                &mut content,
                "| {} | `{}` | {} | {} | {} |",
                impl_ref, inst, register_context, memory_context, causes
            )
            .unwrap();
        }
        writeln!(&mut content).unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_write_report(
    path: &Path,
    previous_testcase: &GeneratedTestCase,
    current_testcase: &GeneratedTestCase,
    differences: &[(WriteDifference, String)],
    impl_order: &[RiscVImpl],
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    let old_len = determine_test_length(impl_order, previous_testcase)?;
    let new_len = determine_test_length(impl_order, current_testcase)?;
    let removed_count = old_len.saturating_sub(new_len);
    let extra_removed = removed_count.saturating_sub(differences.len());

    writeln!(
        &mut content,
        "# Write Difference Removal Report\n\n- Original test instruction count: {}\n- Removed: {}\n- Remaining test instruction count: {}\n",
        old_len, removed_count, new_len
    )
    .unwrap();
    if extra_removed > 0 {
        writeln!(
            &mut content,
            "- Extra history instructions removed: {}\n",
            extra_removed
        )
        .unwrap();
    }

    for (diff, cause_detail) in differences.iter() {
        writeln!(
            &mut content,
            "## Test Instruction #{}, global index {}\n",
            diff.test_index, diff.global_index
        )
        .unwrap();
        let classification = describe_write_difference_pattern(diff, impl_order);
        let tag = classify_write_removal(diff, impl_order, previous_testcase);
        let tag_text = if tag.is_known {
            format!("Known - {}", tag.name)
        } else {
            tag.name.to_string()
        };
        writeln!(&mut content, "- Classification: {}\n", classification).unwrap();
        writeln!(&mut content, "- Tag: {}\n", tag_text).unwrap();
        writeln!(&mut content, "- {}\n", cause_detail).unwrap();
        writeln!(
            &mut content,
            "| Implementation | Instruction | Register Context | Memory Context | Register Writes | Memory Writes |\n|------|------|------------|------------|------------|----------|"
        )
        .unwrap();
        for impl_ref in impl_order {
            let inst = previous_testcase
                .test_insts
                .get(impl_ref)
                .and_then(|block| block.lines().get(diff.test_index))
                .cloned()
                .unwrap_or_else(|| "-".to_string());
            let register_context = diff
                .register_contexts
                .get(impl_ref)
                .map(|ctx| format_register_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let memory_context = diff
                .memory_contexts
                .get(impl_ref)
                .map(|ctx| format_memory_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let regs = diff
                .register_changes
                .get(impl_ref)
                .map(|entries| format_register_list(entries))
                .unwrap_or_else(|| "-".to_string());
            let mems = diff
                .memory_changes
                .get(impl_ref)
                .map(|entries| format_memory_list(entries))
                .unwrap_or_else(|| "-".to_string());

            writeln!(
                &mut content,
                "| {} | `{}` | {} | {} | {} | {} |",
                impl_ref, inst, register_context, memory_context, regs, mems
            )
            .unwrap();
        }
        writeln!(&mut content).unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_replay_write_report(
    path: &Path,
    testcase: &GeneratedTestCase,
    difference: &WriteDifference,
    impl_order: &[RiscVImpl],
    cause_detail: &str,
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    writeln!(
        &mut content,
        "# Write Difference Replay Report\n\n- Difference source: {}\n- Test instruction index: {}\n- Global index: {}\n",
        cause_detail, difference.test_index, difference.global_index
    )
    .unwrap();

    writeln!(
        &mut content,
        "| Implementation | Instruction | Register Context | Memory Context | Register Writes | Memory Writes |\n|------|------|------------|------------|------------|----------|"
    )
    .unwrap();

    for impl_ref in impl_order {
        let inst = testcase
            .test_insts
            .get(impl_ref)
            .and_then(|block| block.lines().get(difference.test_index))
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        let register_context = difference
            .register_contexts
            .get(impl_ref)
            .map(|ctx| format_register_context_display(ctx))
            .unwrap_or_else(|| "-".to_string());
        let memory_context = difference
            .memory_contexts
            .get(impl_ref)
            .map(|ctx| format_memory_context_display(ctx))
            .unwrap_or_else(|| "-".to_string());
        let regs = difference
            .register_changes
            .get(impl_ref)
            .map(|entries| format_register_list(entries))
            .unwrap_or_else(|| "-".to_string());
        let mems = difference
            .memory_changes
            .get(impl_ref)
            .map(|entries| format_memory_list(entries))
            .unwrap_or_else(|| "-".to_string());

        writeln!(
            &mut content,
            "| {} | `{}` | {} | {} | {} | {} |",
            impl_ref, inst, register_context, memory_context, regs, mems
        )
        .unwrap();
    }
    writeln!(&mut content).unwrap();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_replay_write_failure_report(
    path: &Path,
    testcase: &GeneratedTestCase,
    difference: &WriteDifference,
    impl_order: &[RiscVImpl],
    expected_index: usize,
    failure_reason: &str,
) -> Result<(), DiffAnalysisError> {
    let init_len = determine_init_length(impl_order, testcase)?;
    let actual_global = difference.global_index;
    let actual_index = difference.test_index;

    let mut content = String::new();
    writeln!(
        &mut content,
        "# Write Difference Replay Report (Failure)\n\n- Target test instruction index: {}\n- Actual first difference index: {}\n- Target global index: {}\n- Actual global index: {}\n- Failure reason: {}\n",
        expected_index,
        actual_index,
        init_len + expected_index,
        actual_global,
        failure_reason
    )
    .unwrap();

    writeln!(
        &mut content,
        "- Next step: because write replay failed, analysis falls back to 'single test instruction causes the difference' and will directly remove the target instruction.\n"
    )
    .unwrap();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_history_minimization_error_report(
    path: &Path,
    difference: &WriteDifference,
    reason: &str,
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    writeln!(
        &mut content,
        "# History Minimization Report (Failure)\n\n- Test instruction index: {}\n- Global index: {}\n- Failure reason: {}\n",
        difference.test_index, difference.global_index, reason
    )
    .unwrap();

    writeln!(
        &mut content,
        "| Implementation | Register Writes | Memory Writes |\n|------|------------|----------|"
    )
    .unwrap();

    for (impl_ref, regs) in &difference.register_changes {
        let mems = difference
            .memory_changes
            .get(impl_ref)
            .map(|entries| format_memory_list(entries))
            .unwrap_or_else(|| "-".to_string());
        let regs_fmt = format_register_list(regs);
        writeln!(
            &mut content,
            "| {} | {} | {} |",
            impl_ref,
            if regs_fmt.is_empty() {
                "-".to_string()
            } else {
                regs_fmt
            },
            if mems.is_empty() {
                "-".to_string()
            } else {
                mems
            }
        )
        .unwrap();
    }
    writeln!(&mut content).unwrap();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_history_minimization_report(
    path: &Path,
    original_difference: &WriteDifference,
    minimized_start: usize,
    original_target_index: usize,
    minimized_testcase: &GeneratedTestCase,
    minimized_outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    impl_order: &[RiscVImpl],
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    let new_target_index = original_target_index
        .checked_sub(minimized_start)
        .ok_or_else(|| DiffAnalysisError::TestInstructionCountMismatch {
            details: format!(
                "history minimization produced invalid index: start={}, target={}",
                minimized_start, original_target_index
            ),
        })?;

    writeln!(
        &mut content,
        "# History Difference Minimization Report\n\n- Original global index: {}\n- Original test instruction index: {}\n- Minimized history start: {}\n- History instruction count: {}\n",
        original_difference.global_index,
        original_target_index,
        minimized_start,
        original_target_index.saturating_sub(minimized_start)
    )
    .unwrap();

    let minimized_differences =
        detect_unique_writes(impl_order, minimized_testcase, minimized_outputs)?;
    if let Some(diff) = minimized_differences
        .into_iter()
        .find(|d| d.test_index == new_target_index)
    {
        let classification = describe_write_difference_pattern(&diff, impl_order);
        writeln!(&mut content, "- Classification: {}\n", classification).unwrap();
        writeln!(
            &mut content,
            "| Implementation | Instruction | Register Context | Memory Context | Register Writes | Memory Writes |\n|------|------|------------|------------|------------|----------|"
        )
        .unwrap();
        for impl_ref in impl_order {
            let inst = minimized_testcase
                .test_insts
                .get(impl_ref)
                .and_then(|block| block.lines().get(new_target_index))
                .cloned()
                .unwrap_or_else(|| "-".to_string());
            let register_context = diff
                .register_contexts
                .get(impl_ref)
                .map(|ctx| format_register_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let memory_context = diff
                .memory_contexts
                .get(impl_ref)
                .map(|ctx| format_memory_context_display(ctx))
                .unwrap_or_else(|| "-".to_string());
            let regs = diff
                .register_changes
                .get(impl_ref)
                .map(|entries| format_register_list(entries))
                .unwrap_or_else(|| "-".to_string());
            let mems = diff
                .memory_changes
                .get(impl_ref)
                .map(|entries| format_memory_list(entries))
                .unwrap_or_else(|| "-".to_string());
            writeln!(
                &mut content,
                "| {} | `{}` | {} | {} | {} | {} |",
                impl_ref, inst, register_context, memory_context, regs, mems
            )
            .unwrap();
        }
    } else {
        writeln!(
            &mut content,
            "> Warning: the minimized case did not show a write difference at index #{}, manual inspection may be needed.",
            new_target_index
        )
        .unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_history_candidate_report(
    path: &Path,
    original_start: usize,
    original_target_index: usize,
    difference: Option<&WriteDifference>,
    testcase: &GeneratedTestCase,
    impl_order: &[RiscVImpl],
    cause_detail: &str,
) -> Result<(), DiffAnalysisError> {
    let mut content = String::new();
    let init_len = determine_init_length(impl_order, testcase)?;
    let display_global = init_len + original_target_index;

    match difference {
        Some(diff) => {
            let display_index = original_start + diff.test_index;
            writeln!(
                &mut content,
                "## Test Instruction #{}, global index {}\n",
                display_index, display_global
            )
            .unwrap();
            let classification = describe_write_difference_pattern(diff, impl_order);
            writeln!(&mut content, "- Classification: {}\n", classification).unwrap();
            writeln!(&mut content, "- {}\n", cause_detail).unwrap();
            writeln!(
                &mut content,
                "| Implementation | Instruction | Register Context | Memory Context | Register Writes | Memory Writes |\n|------|------|------------|------------|------------|----------|"
            )
            .unwrap();

            for impl_ref in impl_order {
                let inst = testcase
                    .test_insts
                    .get(impl_ref)
                    .and_then(|block| block.lines().get(diff.test_index))
                    .cloned()
                    .unwrap_or_else(|| "-".to_string());
                let register_context = diff
                    .register_contexts
                    .get(impl_ref)
                    .map(|ctx| format_register_context_display(ctx))
                    .unwrap_or_else(|| "-".to_string());
                let memory_context = diff
                    .memory_contexts
                    .get(impl_ref)
                    .map(|ctx| format_memory_context_display(ctx))
                    .unwrap_or_else(|| "-".to_string());
                let regs = diff
                    .register_changes
                    .get(impl_ref)
                    .map(|entries| format_register_list(entries))
                    .unwrap_or_else(|| "-".to_string());
                let mems = diff
                    .memory_changes
                    .get(impl_ref)
                    .map(|entries| format_memory_list(entries))
                    .unwrap_or_else(|| "-".to_string());
                writeln!(
                    &mut content,
                    "| {} | `{}` | {} | {} | {} | {} |",
                    impl_ref, inst, register_context, memory_context, regs, mems
                )
                .unwrap();
            }
        }
        None => {
            writeln!(
                &mut content,
                "## Test Instruction #{}, global index {}\n",
                original_target_index, display_global
            )
            .unwrap();
            if !cause_detail.is_empty() {
                writeln!(&mut content, "- {}\n", cause_detail).unwrap();
            }
            writeln!(
                &mut content,
                "> No historical difference reproduced in this candidate."
            )
            .unwrap();
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_execution_state_report(
    path: &Path,
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
    impl_order: &[RiscVImpl],
) -> Result<(), DiffAnalysisError> {
    let test_len = determine_test_length(impl_order, testcase)?;

    let mut content = String::new();
    writeln!(
        &mut content,
        "# Execution Result Summary\n\n- Test instruction count: {}\n",
        test_len
    )
    .unwrap();

    writeln!(
        &mut content,
        "| Implementation | Exception Count | Instructions with Writes |\n|------|----------|----------------|"
    )
    .unwrap();
    for impl_ref in impl_order {
        let ctx = outputs
            .get(impl_ref)
            .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            })?;
        let exception_count = ctx.exceptions.len();
        let write_count = ctx
            .register_changes
            .iter()
            .zip(ctx.memory_changes.iter())
            .filter(|(regs, mems)| !regs.is_empty() || !mems.is_empty())
            .count();
        writeln!(
            &mut content,
            "| {} | {} | {} |",
            impl_ref, exception_count, write_count
        )
        .unwrap();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub(crate) fn write_final_summary(
    path: &Path,
    initial_testcase: &GeneratedTestCase,
    final_testcase: &GeneratedTestCase,
    exception_rounds: usize,
    write_rounds: usize,
) -> Result<(), DiffAnalysisError> {
    let initial_len = initial_testcase
        .test_insts
        .values()
        .next()
        .map(|block| block.len())
        .unwrap_or(0);
    let final_len = final_testcase
        .test_insts
        .values()
        .next()
        .map(|block| block.len())
        .unwrap_or(0);

    let mut content = String::new();
    writeln!(
        &mut content,
        "# Diff Analysis Final Report\n\n- Initial test instruction count: {}\n- Final test instruction count: {}\n- Exception-difference removal rounds: {}\n- Write-difference removal rounds: {}\n",
        initial_len, final_len, exception_rounds, write_rounds
    )
    .unwrap();
    fs::write(path, content)?;
    Ok(())
}

fn describe_write_difference_pattern(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
) -> &'static str {
    let mut signature_counts: HashMap<(Vec<(String, u64)>, Vec<(u64, u8)>), usize> = HashMap::new();

    for impl_ref in impl_order {
        let register_changes = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        let memory_changes = diff
            .memory_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        *signature_counts
            .entry((register_changes, memory_changes))
            .or_insert(0) += 1;
    }

    if signature_counts.values().all(|count| *count == 1) {
        "Each implementation's writes are different"
    } else {
        "Majority vs minority"
    }
}

pub(crate) fn format_register_list(entries: &[(String, u64)]) -> String {
    if entries.is_empty() {
        "-".to_string()
    } else {
        entries
            .iter()
            .map(|(name, value)| format!("{}=0x{:x}", name, value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(crate) fn format_memory_list(entries: &[(u64, u8)]) -> String {
    if entries.is_empty() {
        "-".to_string()
    } else {
        entries
            .iter()
            .map(|(addr, value)| format!("0x{:x}=0x{:02x}", addr, value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_register_context_display(context: &RegisterContext) -> String {
    if context.entries.is_empty() {
        "-".to_string()
    } else {
        context
            .entries
            .iter()
            .map(|entry| format!("{}=0x{:x}", entry.name, entry.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_memory_context_display(context: &MemoryContext) -> String {
    if context.base_register.is_empty() || context.entries.is_empty() {
        "-".to_string()
    } else {
        let mut parts = Vec::new();
        parts.push(format!(
            "{}=0x{:x}",
            context.base_register, context.base_offset
        ));

        for entry in &context.entries {
            let label = match entry.width {
                MemoryValueWidth::Byte => "byte",
                MemoryValueWidth::Half => "half",
                MemoryValueWidth::Word => "word",
                MemoryValueWidth::Dword => "dword",
            };
            let addresses = entry
                .addresses
                .iter()
                .map(|addr| format!("0x{:x}", addr))
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("{} [{}]=0x{:x}", label, addresses, entry.value));
        }

        parts.join("; ")
    }
}
