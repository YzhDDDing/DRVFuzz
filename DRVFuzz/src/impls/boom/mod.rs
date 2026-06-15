use std::path::Path;
use std::time::Duration;

use crate::{
    error::ImplExecutionError, execution_output::ExecutionOutput, extension_map::ExtensionMap,
    isa_base::ISABase, riscv_impls::RiscVImpl,
};

mod config;
mod execute;

pub(crate) use config::{
    build_asm_content, extensions_v3, extensions_v4, linker_script_content, supported_isa_bases,
    supported_unaligned_access_modes, user_mem_range,
};

pub use execute::{
    BoomExecutor, BoomRunResult, BoomTrace, ExceptionEvent, ExecutorConfig, MemoryWrite,
    RegisterWrite, parse_boom_log,
};

pub fn execute_with_extension_override_for_impl<P: AsRef<Path>>(
    riscv_impl: RiscVImpl,
    run_root: P,
    isa_base: ISABase,
    user_insts: &[String],
    timeout: Option<Duration>,
    extension_override: Option<&ExtensionMap>,
    _allow_unaligned: bool,
) -> Result<ExecutionOutput, ImplExecutionError> {
    let mut config = ExecutorConfig::new(run_root.as_ref().to_path_buf(), isa_base, riscv_impl);
    if let Some(map) = extension_override {
        config.set_extension_override(map.clone());
    }
    config.timeout = timeout;
    let executor = BoomExecutor::new(config);
    executor.execute(run_root, user_insts).map_err(Into::into)
}
