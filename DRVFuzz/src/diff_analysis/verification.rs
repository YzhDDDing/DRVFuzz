use std::collections::{BTreeMap, HashMap};

use crate::{
    execution_output::ExecutionContextOutput, riscv_impls::RiscVImpl,
    riscv_impls_vec::GeneratedTestCase,
};

use super::analysis_utils::{canonicalize_memory_changes, canonicalize_register_changes_for_diff};

#[derive(Clone)]
pub(crate) struct InitializationSummary {
    pub(crate) init_instruction_count: usize,
}

pub(crate) struct InitializationMismatchData {
    pub(crate) instruction_index: usize,
    pub(crate) reason: String,
    pub(crate) per_impl_registers: BTreeMap<RiscVImpl, Vec<(String, u64)>>,
    pub(crate) per_impl_memory: BTreeMap<RiscVImpl, Vec<(u64, u8)>>,
}

#[derive(Clone)]
struct InitEffect {
    registers: Vec<(String, u64)>,
    memory: Vec<(u64, u8)>,
}

pub(crate) fn verify_initialization(
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
    outputs: &HashMap<RiscVImpl, ExecutionContextOutput>,
) -> Result<InitializationSummary, InitializationMismatchData> {
    let mut init_len = None;
    let mut effects_per_impl: Vec<(RiscVImpl, Vec<InitEffect>)> = Vec::new();

    for impl_ref in impl_order {
        let init_insts =
            testcase
                .init_insts
                .get(impl_ref)
                .ok_or_else(|| InitializationMismatchData {
                    instruction_index: 0,
                    reason: format!("Missing initialization instructions for {}", impl_ref),
                    per_impl_registers: BTreeMap::new(),
                    per_impl_memory: BTreeMap::new(),
                })?;
        let expected_len = *init_len.get_or_insert(init_insts.len());
        if init_insts.len() != expected_len {
            return Err(InitializationMismatchData {
                instruction_index: 0,
                reason: format!(
                    "{} has {} initialization instructions, but expected {}",
                    impl_ref,
                    init_insts.len(),
                    expected_len
                ),
                per_impl_registers: BTreeMap::new(),
                per_impl_memory: BTreeMap::new(),
            });
        }

        let ctx = outputs
            .get(impl_ref)
            .ok_or_else(|| InitializationMismatchData {
                instruction_index: 0,
                reason: format!("Missing execution output for {}", impl_ref),
                per_impl_registers: BTreeMap::new(),
                per_impl_memory: BTreeMap::new(),
            })?;

        if ctx.register_changes.len() < expected_len || ctx.memory_changes.len() < expected_len {
            return Err(InitializationMismatchData {
                instruction_index: ctx.register_changes.len().min(ctx.memory_changes.len()),
                reason: format!(
                    "{} has insufficient initialization execution data length",
                    impl_ref
                ),
                per_impl_registers: BTreeMap::new(),
                per_impl_memory: BTreeMap::new(),
            });
        }

        let mut effects = Vec::with_capacity(expected_len);
        for idx in 0..expected_len {
            let registers = canonicalize_register_changes_for_diff(
                &ctx.register_changes[idx],
                &testcase.config,
            );
            let memory = canonicalize_memory_changes(&ctx.memory_changes[idx]);
            effects.push(InitEffect { registers, memory });
        }

        effects_per_impl.push((*impl_ref, effects));
    }

    if effects_per_impl.is_empty() {
        return Ok(InitializationSummary {
            init_instruction_count: 0,
        });
    }

    let (_, reference_effects) = &effects_per_impl[0];
    for (impl_ref, effects) in effects_per_impl.iter().skip(1) {
        for (idx, effect) in effects.iter().enumerate() {
            let reference_effect = &reference_effects[idx];
            let registers_diff = effect.registers != reference_effect.registers;
            let memory_diff = effect.memory != reference_effect.memory;
            if registers_diff || memory_diff {
                let mut per_impl_registers = BTreeMap::new();
                let mut per_impl_memory = BTreeMap::new();
                for (impl_item, item_effects) in &effects_per_impl {
                    per_impl_registers.insert(*impl_item, item_effects[idx].registers.clone());
                    per_impl_memory.insert(*impl_item, item_effects[idx].memory.clone());
                }

                let reason = match (registers_diff, memory_diff) {
                    (true, true) => format!(
                        "Implementation {} has mismatching register and memory writes",
                        impl_ref
                    ),
                    (true, false) => format!(
                        "Implementation {} has mismatching register writes",
                        impl_ref
                    ),
                    (false, true) => {
                        format!("Implementation {} has mismatching memory writes", impl_ref)
                    }
                    _ => unreachable!(),
                };

                return Err(InitializationMismatchData {
                    instruction_index: idx,
                    reason,
                    per_impl_registers,
                    per_impl_memory,
                });
            }
        }
    }

    Ok(InitializationSummary {
        init_instruction_count: init_len.unwrap_or(0),
    })
}
