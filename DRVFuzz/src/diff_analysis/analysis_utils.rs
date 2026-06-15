use crate::{
    execution_output::{MemValue, RegisterValue},
    riscv_impls::RiscVImpl,
    riscv_impls_vec::{GeneratedTestCase, TestCaseConfig},
};

use super::types::DiffAnalysisError;

pub(crate) fn determine_test_length(
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> Result<usize, DiffAnalysisError> {
    let mut length = None;
    for impl_ref in impl_order {
        let insts = testcase.test_insts.get(impl_ref).ok_or_else(|| {
            DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            }
        })?;
        let expected = *length.get_or_insert(insts.len());
        if insts.len() != expected {
            return Err(DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "implementation {} has {} test instructions, expected {}",
                    impl_ref,
                    insts.len(),
                    expected
                ),
            });
        }
    }
    Ok(length.unwrap_or(0))
}

pub(crate) fn determine_init_length(
    impl_order: &[RiscVImpl],
    testcase: &GeneratedTestCase,
) -> Result<usize, DiffAnalysisError> {
    let mut length = None;
    for impl_ref in impl_order {
        let insts = testcase.init_insts.get(impl_ref).ok_or_else(|| {
            DiffAnalysisError::MissingInstructions {
                impl_name: impl_ref.to_string(),
            }
        })?;
        let expected = *length.get_or_insert(insts.len());
        if insts.len() != expected {
            return Err(DiffAnalysisError::TestInstructionCountMismatch {
                details: format!(
                    "implementation {} has {} init instructions, expected {}",
                    impl_ref,
                    insts.len(),
                    expected
                ),
            });
        }
    }
    Ok(length.unwrap_or(0))
}

pub(crate) fn canonicalize_register_changes_for_diff(
    changes: &[RegisterValue],
    config: &TestCaseConfig,
) -> Vec<(String, u64)> {
    let mut entries = changes
        .iter()
        .filter(|entry| register_in_test_scope(&entry.name, config))
        .map(|entry| (entry.name.clone(), entry.value))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn register_in_test_scope(name: &str, config: &TestCaseConfig) -> bool {
    let mut chars = name.chars();
    let prefix = match chars.next() {
        Some(p) => p,
        None => return false,
    };
    let digits = chars.as_str();
    let idx: u8 = match digits.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };

    let register_config = &config.test_register_config;
    match prefix {
        'x' => {
            let (min, max) = register_config.integer_register_range;
            idx >= min && idx <= max
        }
        'f' => {
            let (min, max) = register_config.floating_point_register_range;
            idx >= min && idx <= max
        }
        'v' => {
            let (min, max) = register_config.vector_register_range;
            idx >= min && idx <= max
        }
        _ => false,
    }
}

pub(crate) fn canonicalize_memory_changes(changes: &[MemValue]) -> Vec<(u64, u8)> {
    let mut entries = changes
        .iter()
        .map(|entry| (entry.addr, entry.value))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}
