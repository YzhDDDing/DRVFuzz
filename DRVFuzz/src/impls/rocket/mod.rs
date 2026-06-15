use std::path::Path;
use std::time::Duration;

use crate::{
    error::ImplExecutionError, execution_output::ExecutionOutput, extension_map::ExtensionMap,
    isa_base::ISABase, riscv_impls::RiscVImpl,
};

mod config;
mod execute;

pub(crate) use config::{
    build_asm_content, extensions, linker_script_content, supported_isa_bases,
    supported_unaligned_access_modes, user_mem_range,
};

pub use execute::{
    ExceptionEvent, ExecutorConfig, MemoryWrite, RegisterWrite, RocketExecutor, RocketRunResult,
    RocketTrace, parse_rocket_log,
};

pub fn execute_with_extension_override<P: AsRef<Path>>(
    run_root: P,
    isa_base: ISABase,
    user_insts: &[String],
    timeout: Option<Duration>,
    extension_override: Option<&ExtensionMap>,
    _allow_unaligned: bool,
) -> Result<ExecutionOutput, ImplExecutionError> {
    let mut config =
        ExecutorConfig::new(run_root.as_ref().to_path_buf(), isa_base, RiscVImpl::Rocket);
    if let Some(map) = extension_override {
        config.set_extension_override(map.clone());
    }
    config.timeout = timeout;
    let executor = RocketExecutor::new(config);
    executor.execute(run_root, user_insts).map_err(Into::into)
}

pub fn execute<P: AsRef<Path>>(
    run_root: P,
    isa_base: ISABase,
    user_insts: &[String],
    timeout: Option<Duration>,
    allow_unaligned: bool,
) -> Result<ExecutionOutput, ImplExecutionError> {
    execute_with_extension_override(
        run_root,
        isa_base,
        user_insts,
        timeout,
        None,
        allow_unaligned,
    )
}
