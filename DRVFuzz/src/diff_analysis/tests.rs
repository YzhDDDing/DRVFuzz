use std::{collections::HashMap, env::current_dir};

use super::*;
use crate::{
    isa_base::ISABase,
    riscv_impls_vec::{RandomConfigOverrides, RiscVImplVec, TestCaseConfig},
};
use riscv_instruction_types::RegisterConfig;

#[test]
fn diff_analysis_all_rv32_completes() {
    let riscv_impl_vec = RiscVImplVec::all(ISABase::Rv32)
        .filter_by_unaligned_access_requirement(Some(false), &HashMap::new());
    let temp = current_dir().unwrap().join("temp");
    let run_root = temp.join("diff_rv32");

    let config = DiffAnalysisConfig {
        testcase_config: TestCaseConfig {
            isa_base: ISABase::Rv32,
            extension_insts_count: 100,
            extension_inst_scaling: None,
            mem_size: 32,
            mem_access_offset: (0, 0),
            test_register_config: RegisterConfig {
                integer_register_range: (16, 25),
                floating_point_register_range: (16, 25),
                vector_register_range: (16, 25),
            },
            temp_register_range: (10, 15),
            data_sensitive_mode: false,
            data_sensitive_probability: 1.0,
            unaligned_access_required: Some(false),
            random_config: RandomConfigOverrides::default(),
        },
        run_root: run_root.clone(),
        max_iterations: None,
        history_min_test_threshold: None,
        impl_timeouts: HashMap::new(),
        emit_execution_output_json: false,
        emit_execution_report_md: false,
        cleanup_successful_iteration_artifacts: false,
        cleanup_successful_diff_run: false,
        guidance_strategy: None,
        transition_seed_pool_limit: 64,
        transition_seed_window: 16,
    };

    let result =
        run_diff_analysis(&riscv_impl_vec, config).expect("diff analysis rv32 should succeed");
    assert!(
        run_root.join("final_report.md").exists(),
        "final report should be generated"
    );
    assert_eq!(
        result.final_outputs.len(),
        riscv_impl_vec.iter().count(),
        "should retain outputs for all implementations"
    );
}

#[test]
fn diff_analysis_all_rv64_completes() {
    let riscv_impl_vec = RiscVImplVec::all(ISABase::Rv64)
        .filter_by_unaligned_access_requirement(Some(false), &HashMap::new());
    let temp = current_dir().unwrap().join("temp");
    let run_root = temp.join("diff_rv64");

    let config = DiffAnalysisConfig {
        testcase_config: TestCaseConfig {
            isa_base: ISABase::Rv64,
            extension_insts_count: 100,
            extension_inst_scaling: None,
            mem_size: 64,
            mem_access_offset: (0, 0),
            test_register_config: RegisterConfig {
                integer_register_range: (16, 25),
                floating_point_register_range: (16, 25),
                vector_register_range: (16, 25),
            },
            temp_register_range: (10, 15),
            data_sensitive_mode: false,
            data_sensitive_probability: 1.0,
            unaligned_access_required: Some(false),
            random_config: RandomConfigOverrides::default(),
        },
        run_root: run_root.clone(),
        max_iterations: None,
        history_min_test_threshold: None,
        impl_timeouts: HashMap::new(),
        emit_execution_output_json: false,
        emit_execution_report_md: false,
        cleanup_successful_iteration_artifacts: false,
        cleanup_successful_diff_run: false,
        guidance_strategy: None,
        transition_seed_pool_limit: 64,
        transition_seed_window: 16,
    };

    let result =
        run_diff_analysis(&riscv_impl_vec, config).expect("diff analysis rv64 should succeed");
    assert!(
        run_root.join("final_report.md").exists(),
        "final report should be generated"
    );
    assert_eq!(
        result.final_outputs.len(),
        riscv_impl_vec.iter().count(),
        "should retain outputs for all implementations"
    );
}

#[test]

fn test_save() {
    let _analysis_config = DiffAnalysisConfig {
        testcase_config: TestCaseConfig {
            isa_base: ISABase::Rv32,
            extension_insts_count: 100,
            extension_inst_scaling: None,
            mem_size: 32,
            mem_access_offset: (0, 0),
            test_register_config: RegisterConfig {
                integer_register_range: (16, 25),
                floating_point_register_range: (16, 25),
                vector_register_range: (16, 25),
            },
            temp_register_range: (10, 15),
            data_sensitive_mode: false,
            data_sensitive_probability: 1.0,
            unaligned_access_required: Some(false),
            random_config: RandomConfigOverrides::default(),
        },
        run_root: "".to_string().into(),
        max_iterations: None,
        history_min_test_threshold: None,
        impl_timeouts: HashMap::new(),
        emit_execution_output_json: false,
        emit_execution_report_md: false,
        cleanup_successful_iteration_artifacts: false,
        cleanup_successful_diff_run: false,
        guidance_strategy: None,
        transition_seed_pool_limit: 64,
        transition_seed_window: 16,
    };
}
