use std::collections::HashSet;

use crate::{riscv_impls::RiscVImpl, riscv_impls_vec::GeneratedTestCase};

use super::detection::WriteDifference;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WriteRemovalTag {
    pub(crate) name: &'static str,
    pub(crate) is_known: bool,
}

impl WriteRemovalTag {
    pub(crate) fn unknown() -> Self {
        WriteRemovalTag {
            name: "Unknown",
            is_known: false,
        }
    }
}

pub(crate) fn classify_write_removal(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> WriteRemovalTag {
    if is_amo_difference(diff, impl_order, testcase) {
        return WriteRemovalTag {
            name: "AMO DIFFERENCE",
            is_known: true,
        };
    }

    if let Some(tag) = classify_cva6_fp_value_mismatch(
        diff,
        impl_order,
        testcase,
        &[
            ("fdiv.h", "CVA6 FDIV_H BUG"),
            ("fdiv.s", "CVA6 FDIV_S BUG"),
            ("fsqrt.h", "CVA6 FSQRT_H BUG"),
            ("fsqrt.s", "CVA6 FSQRT_S BUG"),
        ],
    ) {
        return tag;
    }

    if let Some(tag) = classify_rocket_csr_uninit(diff, impl_order, testcase) {
        return tag;
    }

    if let Some(tag) = classify_rocket_missing_log(diff, impl_order) {
        return tag;
    }

    if let Some(tag) = classify_csr_difference(diff, impl_order, testcase) {
        return tag;
    }

    if is_cva6_fpu_to_integer_trace_mislogging(diff, impl_order) {
        return WriteRemovalTag {
            name: "CVA6 FPU-to-Integer Trace Mislogging",
            is_known: true,
        };
    }

    WriteRemovalTag::unknown()
}

fn is_cva6_fpu_to_integer_trace_mislogging(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
) -> bool {
    if !impl_order.contains(&RiscVImpl::CVA6) {
        return false;
    }

    // Ensure all memory writes are identical across implementations.
    let mut baseline_mem = None;
    for impl_ref in impl_order {
        let mem_writes = diff
            .memory_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        match &mut baseline_mem {
            Some(existing) => {
                if *existing != mem_writes {
                    return false;
                }
            }
            None => {
                baseline_mem = Some(mem_writes);
            }
        }
    }

    // Collect register write sets for non-CVA6 implementations.
    let mut baseline_regs = None;
    let mut seen_non_cva6 = false;
    for impl_ref in impl_order {
        if *impl_ref == RiscVImpl::CVA6 {
            continue;
        }
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    return false;
                }
            }
            None => {
                baseline_regs = Some(regs);
            }
        }
        seen_non_cva6 = true;
    }

    if !seen_non_cva6 {
        return false;
    }

    let baseline_regs = match baseline_regs {
        Some(value) => value,
        None => return false,
    };

    let cva6_regs = diff
        .register_changes
        .get(&RiscVImpl::CVA6)
        .cloned()
        .unwrap_or_default();

    if baseline_regs.len() != cva6_regs.len() {
        return false;
    }

    // The CVA6 mislogging case expects identical register indices and values,
    // but register classes differ (xN vs fN). Order should match; if not, fall back to Unknown.
    for (baseline, cva6) in baseline_regs.iter().zip(cva6_regs.iter()) {
        let (baseline_reg, baseline_value) = baseline;
        let (cva6_reg, cva6_value) = cva6;
        if baseline_value != cva6_value {
            return false;
        }
        // Register names must share the same numeric suffix.
        let baseline_suffix = extract_reg_suffix(baseline_reg);
        let cva6_suffix = extract_reg_suffix(cva6_reg);
        if baseline_suffix.is_none() || cva6_suffix.is_none() {
            return false;
        }
        if baseline_suffix != cva6_suffix {
            return false;
        }
        // Baseline should be an integer register and CVA6 should be an FPU register.
        if !baseline_reg.starts_with('x') || !cva6_reg.starts_with('f') {
            return false;
        }
    }

    // Ensure only CVA6 deviates from the majority register naming.
    let mut differing_impls = HashSet::new();
    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if regs != baseline_regs {
            differing_impls.insert(*impl_ref);
        }
    }

    differing_impls.len() == 1 && differing_impls.contains(&RiscVImpl::CVA6)
}

fn extract_reg_suffix(reg_name: &str) -> Option<&str> {
    let digits_pos = reg_name.find(|c: char| c.is_ascii_digit())?;
    let suffix = &reg_name[digits_pos..];
    if suffix.chars().all(|c| c.is_ascii_digit()) {
        Some(suffix)
    } else {
        None
    }
}

fn is_amo_difference(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> bool {
    let instruction = match instruction_for_diff(testcase, impl_order, diff.test_index) {
        Some(inst) => inst,
        None => return false,
    };
    let inst_lower = instruction.trim_start().to_ascii_lowercase();
    if !inst_lower.starts_with("amo") {
        return false;
    }

    let mut baseline_regs = None;
    let mut all_empty_regs = true;
    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if !regs.is_empty() {
            all_empty_regs = false;
        }
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    return false;
                }
            }
            None => baseline_regs = Some(regs),
        }
    }

    if baseline_regs.is_none() {
        return false;
    }

    if !all_empty_regs {
        if baseline_regs
            .as_ref()
            .map(|regs| regs.is_empty())
            .unwrap_or(true)
        {
            return false;
        }
    }

    let mut baseline_mem = None;
    let mut has_mem_difference = false;
    for impl_ref in impl_order {
        let mem = diff
            .memory_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        match &mut baseline_mem {
            Some(existing) => {
                if *existing != mem {
                    has_mem_difference = true;
                }
            }
            None => baseline_mem = Some(mem),
        }
    }

    has_mem_difference
}

fn instruction_for_diff<'a>(
    testcase: &'a GeneratedTestCase,
    impl_order: &[RiscVImpl],
    test_index: usize,
) -> Option<&'a str> {
    impl_order
        .iter()
        .filter_map(|impl_ref| {
            testcase
                .test_insts
                .get(impl_ref)
                .and_then(|block| block.lines().get(test_index))
                .map(|s| s.as_str())
        })
        .next()
}

fn classify_cva6_fp_value_mismatch(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
    prefixes: &[(&str, &'static str)],
) -> Option<WriteRemovalTag> {
    if !impl_order.contains(&RiscVImpl::CVA6) {
        return None;
    }

    let instruction = instruction_for_diff(testcase, impl_order, diff.test_index)?;
    let inst_lower = instruction.trim_start().to_ascii_lowercase();
    let Some((_, tag_name)) = prefixes
        .iter()
        .find(|(prefix, _)| inst_lower.starts_with(prefix))
    else {
        return None;
    };

    if !memory_writes_identical(diff, impl_order) {
        return None;
    }

    let (baseline_regs, cva6_regs) = match split_register_writes(diff, impl_order) {
        Some(value) => value,
        None => return None,
    };

    if baseline_regs.is_empty()
        || cva6_regs.is_empty()
        || baseline_regs.len() != cva6_regs.len()
        || !registers_are_float(&baseline_regs)
        || !registers_are_float(&cva6_regs)
    {
        return None;
    }

    let mut has_value_difference = false;
    for ((base_name, base_value), (cva6_name, cva6_value)) in
        baseline_regs.iter().zip(cva6_regs.iter())
    {
        if !base_name.eq_ignore_ascii_case(cva6_name) {
            return None;
        }
        if base_value != cva6_value {
            has_value_difference = true;
        }
    }

    if !has_value_difference {
        return None;
    }

    if !other_impls_match_baseline(diff, impl_order, &baseline_regs) {
        return None;
    }

    Some(WriteRemovalTag {
        name: tag_name,
        is_known: true,
    })
}

fn classify_rocket_csr_uninit(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> Option<WriteRemovalTag> {
    if !impl_order.contains(&RiscVImpl::Rocket) {
        return None;
    }

    let instruction = instruction_for_diff(testcase, impl_order, diff.test_index)?;
    let inst_lower = instruction.trim_start().to_ascii_lowercase();
    if !inst_lower.starts_with("csr") {
        return None;
    }

    if !memory_writes_identical(diff, impl_order) {
        return None;
    }

    let mut baseline_regs = None;
    let mut seen_non_rocket = false;
    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if regs.is_empty() {
            return None;
        }
        if *impl_ref == RiscVImpl::Rocket {
            continue;
        }
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    return None;
                }
            }
            None => baseline_regs = Some(regs),
        }
        seen_non_rocket = true;
    }

    if !seen_non_rocket {
        return None;
    }

    let baseline_regs = baseline_regs?;
    let rocket_regs = diff
        .register_changes
        .get(&RiscVImpl::Rocket)
        .cloned()
        .unwrap_or_default();

    if rocket_regs.is_empty() || rocket_regs == baseline_regs {
        return None;
    }

    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if *impl_ref == RiscVImpl::Rocket {
            if regs == baseline_regs {
                return None;
            }
        } else if regs != baseline_regs {
            return None;
        }
    }

    Some(WriteRemovalTag {
        name: "ROCKET CSR UN INIT",
        is_known: true,
    })
}

fn classify_csr_difference(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> Option<WriteRemovalTag> {
    let instruction = instruction_for_diff(testcase, impl_order, diff.test_index)?;
    let inst_lower = instruction.trim_start().to_ascii_lowercase();
    if !inst_lower.starts_with("csr") {
        return None;
    }

    let mut baseline_regs = None;
    let mut has_difference = false;
    let mut saw_writes = false;

    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if !regs.is_empty() {
            saw_writes = true;
        }
        match &baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    has_difference = true;
                }
            }
            None => baseline_regs = Some(regs),
        }
    }

    if !saw_writes || !has_difference {
        return None;
    }

    Some(WriteRemovalTag {
        name: "CSR DIFFERENCE",
        is_known: true,
    })
}

fn classify_rocket_missing_log(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
) -> Option<WriteRemovalTag> {
    if !impl_order.contains(&RiscVImpl::Rocket) {
        return None;
    }

    let has_non_rocket = impl_order
        .iter()
        .any(|impl_ref| *impl_ref != RiscVImpl::Rocket);
    if !has_non_rocket {
        return None;
    }

    let rocket_regs = diff
        .register_changes
        .get(&RiscVImpl::Rocket)
        .cloned()
        .unwrap_or_default();
    let rocket_mem = diff
        .memory_changes
        .get(&RiscVImpl::Rocket)
        .cloned()
        .unwrap_or_default();

    let mut baseline_regs = None;
    let mut non_rocket_regs_identical = true;
    let mut non_rocket_regs_non_empty = false;
    for impl_ref in impl_order {
        if *impl_ref == RiscVImpl::Rocket {
            continue;
        }
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if !regs.is_empty() {
            non_rocket_regs_non_empty = true;
        }
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    non_rocket_regs_identical = false;
                    break;
                }
            }
            None => baseline_regs = Some(regs),
        }
    }

    let mut baseline_mem = None;
    let mut non_rocket_mem_identical = true;
    let mut non_rocket_mem_non_empty = false;
    for impl_ref in impl_order {
        if *impl_ref == RiscVImpl::Rocket {
            continue;
        }
        let mems = diff
            .memory_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if !mems.is_empty() {
            non_rocket_mem_non_empty = true;
        }
        match &mut baseline_mem {
            Some(existing) => {
                if *existing != mems {
                    non_rocket_mem_identical = false;
                    break;
                }
            }
            None => baseline_mem = Some(mems),
        }
    }

    let rocket_missing_regs =
        non_rocket_regs_identical && non_rocket_regs_non_empty && rocket_regs.is_empty();
    let rocket_missing_mem =
        non_rocket_mem_identical && non_rocket_mem_non_empty && rocket_mem.is_empty();

    if !rocket_missing_regs && !rocket_missing_mem {
        return None;
    }

    let registers_identical = register_writes_identical(diff, impl_order);
    let memory_identical = memory_writes_identical(diff, impl_order);

    let register_diff_explained = registers_identical || rocket_missing_regs;
    let memory_diff_explained = memory_identical || rocket_missing_mem;

    if register_diff_explained && memory_diff_explained {
        return Some(WriteRemovalTag {
            name: "ROCKET MISSING LOG",
            is_known: true,
        });
    }

    None
}

fn memory_writes_identical(diff: &WriteDifference, impl_order: &[RiscVImpl]) -> bool {
    let mut baseline_mem = None;
    for impl_ref in impl_order {
        let mem_writes = diff
            .memory_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        match &mut baseline_mem {
            Some(existing) => {
                if *existing != mem_writes {
                    return false;
                }
            }
            None => baseline_mem = Some(mem_writes),
        }
    }
    true
}

fn register_writes_identical(diff: &WriteDifference, impl_order: &[RiscVImpl]) -> bool {
    let mut baseline_regs = None;
    for impl_ref in impl_order {
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    return false;
                }
            }
            None => baseline_regs = Some(regs),
        }
    }
    true
}

fn split_register_writes(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
) -> Option<(Vec<(String, u64)>, Vec<(String, u64)>)> {
    let mut baseline_regs = None;
    let mut found_non_cva6 = false;

    for impl_ref in impl_order {
        if *impl_ref == RiscVImpl::CVA6 {
            continue;
        }
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if regs.is_empty() {
            return None;
        }
        match &mut baseline_regs {
            Some(existing) => {
                if *existing != regs {
                    return None;
                }
            }
            None => baseline_regs = Some(regs),
        }
        found_non_cva6 = true;
    }

    if !found_non_cva6 {
        return None;
    }

    let baseline_regs = baseline_regs?;
    let cva6_regs = diff
        .register_changes
        .get(&RiscVImpl::CVA6)
        .cloned()
        .unwrap_or_default();
    Some((baseline_regs, cva6_regs))
}

fn registers_are_float(registers: &[(String, u64)]) -> bool {
    registers
        .iter()
        .all(|(name, _)| name.to_ascii_lowercase().starts_with('f'))
}

fn other_impls_match_baseline(
    diff: &WriteDifference,
    impl_order: &[RiscVImpl],
    baseline: &[(String, u64)],
) -> bool {
    for impl_ref in impl_order {
        if *impl_ref == RiscVImpl::CVA6 {
            continue;
        }
        let regs = diff
            .register_changes
            .get(impl_ref)
            .cloned()
            .unwrap_or_default();
        if regs != *baseline {
            return false;
        }
    }
    true
}
