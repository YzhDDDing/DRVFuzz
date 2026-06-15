use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{
    execution_output::ExecutionContextOutput,
    riscv_impls::RiscVImpl,
    riscv_impls_vec::GeneratedTestCase,
    utils::{MemoryContext, RegisterContext},
};

use super::{
    analysis_utils::{
        canonicalize_memory_changes, canonicalize_register_changes_for_diff, determine_init_length,
        determine_test_length,
    },
    types::DiffAnalysisError,
};

#[derive(Clone)]
pub(crate) struct ExceptionDifference {
    pub(crate) test_index: usize,
    pub(crate) global_index: usize,
    pub(crate) per_impl: BTreeMap<RiscVImpl, Vec<String>>,
    pub(crate) register_contexts: BTreeMap<RiscVImpl, RegisterContext>,
    pub(crate) memory_contexts: BTreeMap<RiscVImpl, MemoryContext>,
}

#[derive(Clone, Default)]
pub(crate) struct ExceptionDetectionResult {
    pub(crate) trigger_mismatches: Vec<ExceptionDifference>,
    pub(crate) cause_mismatches: Vec<ExceptionDifference>,
}

#[derive(Clone)]
pub(crate) struct WriteDifference {
    pub(crate) test_index: usize,
    pub(crate) global_index: usize,
    pub(crate) register_changes: BTreeMap<RiscVImpl, Vec<(String, u64)>>,
    pub(crate) memory_changes: BTreeMap<RiscVImpl, Vec<(u64, u8)>>,
    pub(crate) register_contexts: BTreeMap<RiscVImpl, RegisterContext>,
    pub(crate) memory_contexts: BTreeMap<RiscVImpl, MemoryContext>,
}

pub(crate) fn detect_unique_exceptions(
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
) -> Result<ExceptionDetectionResult, DiffAnalysisError> {
    let test_len = determine_test_length(impl_order, testcase)?;
    if test_len == 0 {
        return Ok(ExceptionDetectionResult::default());
    }
    let init_len = determine_init_length(impl_order, testcase)?;

    let mut result = ExceptionDetectionResult::default();
    let mut per_impl_maps = HashMap::new();

    for impl_ref in impl_order {
        let ctx = outputs
            .get(impl_ref)
            .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            })?;
        let mut map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for exc in &ctx.exceptions {
            map.entry(exc.user_instruction_index)
                .or_default()
                .push(exc.cause.clone());
        }
        for values in map.values_mut() {
            values.sort();
            values.dedup();
        }
        per_impl_maps.insert(*impl_ref, map);
    }

    for test_idx in 0..test_len {
        let global_idx = init_len + test_idx;
        let mut per_impl = BTreeMap::new();
        let mut unique_patterns = BTreeSet::new();
        let mut saw_empty = false;
        let mut saw_non_empty = false;
        let mut register_contexts = BTreeMap::new();
        let mut memory_contexts = BTreeMap::new();
        for impl_ref in impl_order {
            let causes = per_impl_maps
                .get(impl_ref)
                .and_then(|m| m.get(&global_idx))
                .cloned()
                .unwrap_or_default();
            if causes.is_empty() {
                saw_empty = true;
            } else {
                saw_non_empty = true;
            }
            unique_patterns.insert(causes.clone());
            let output =
                outputs
                    .get(impl_ref)
                    .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                        impl_name: impl_ref.to_string(),
                    })?;
            let instruction_ctx = output.contexts.get(global_idx).ok_or_else(|| {
                DiffAnalysisError::TestInstructionCountMismatch {
                    details: format!(
                        "missing execution context for instruction {} in {}",
                        global_idx, impl_ref
                    ),
                }
            })?;
            register_contexts.insert(*impl_ref, instruction_ctx.register_context.clone());
            memory_contexts.insert(*impl_ref, instruction_ctx.memory_context.clone());
            per_impl.insert(*impl_ref, causes);
        }

        if saw_non_empty && unique_patterns.len() > 1 {
            let diff = ExceptionDifference {
                test_index: test_idx,
                global_index: global_idx,
                per_impl,
                register_contexts,
                memory_contexts,
            };
            if saw_empty {
                result.trigger_mismatches.push(diff);
            } else {
                result.cause_mismatches.push(diff);
            }
        }
    }

    Ok(result)
}

pub(crate) fn detect_unique_writes(
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
) -> Result<Vec<WriteDifference>, DiffAnalysisError> {
    let test_len = determine_test_length(impl_order, testcase)?;
    if test_len == 0 {
        return Ok(Vec::new());
    }
    let init_len = determine_init_length(impl_order, testcase)?;

    let mut result = Vec::new();
    let register_scope = &testcase.config;

    for test_idx in 0..test_len {
        let global_idx = init_len + test_idx;
        let mut variations = BTreeSet::new();
        let mut register_map = BTreeMap::new();
        let mut memory_map = BTreeMap::new();
        let mut register_contexts = BTreeMap::new();
        let mut memory_contexts = BTreeMap::new();

        for impl_ref in impl_order {
            let ctx =
                outputs
                    .get(impl_ref)
                    .ok_or_else(|| DiffAnalysisError::MissingInstructions {
                        impl_name: impl_ref.to_string(),
                    })?;
            let regs = canonicalize_register_changes_for_diff(
                &ctx.register_changes[global_idx],
                register_scope,
            );
            let mems = canonicalize_memory_changes(&ctx.memory_changes[global_idx]);
            register_map.insert(*impl_ref, regs.clone());
            memory_map.insert(*impl_ref, mems.clone());
            let instruction_ctx = ctx.contexts.get(global_idx).ok_or_else(|| {
                DiffAnalysisError::TestInstructionCountMismatch {
                    details: format!(
                        "missing execution context for instruction {} in {}",
                        global_idx, impl_ref
                    ),
                }
            })?;
            register_contexts.insert(*impl_ref, instruction_ctx.register_context.clone());
            memory_contexts.insert(*impl_ref, instruction_ctx.memory_context.clone());
            variations.insert((regs, mems));
        }

        if variations.len() > 1 {
            result.push(WriteDifference {
                test_index: test_idx,
                global_index: global_idx,
                register_changes: register_map,
                memory_changes: memory_map,
                register_contexts,
                memory_contexts,
            });
        }
    }

    Ok(result)
}
