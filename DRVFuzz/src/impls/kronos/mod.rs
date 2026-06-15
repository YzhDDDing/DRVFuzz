use std::path::Path;
use std::time::Duration;

use crate::{
    error::ImplExecutionError, execution_output::ExecutionOutput, isa_base::ISABase,
    riscv_impls::RiscVImpl,
};

mod config;
mod execute;

pub(crate) use config::{
    build_asm_content, extensions, linker_script_content, supported_isa_bases,
    supported_unaligned_access_modes, user_mem_range,
};

pub use execute::{ExceptionEvent, ExecutorConfig, KronosExecutor, MemoryWrite, RegisterWrite};

pub fn execute<P: AsRef<Path>>(
    run_root: P,
    isa_base: ISABase,
    user_insts: &[String],
    timeout: Option<Duration>,
    _allow_unaligned: bool,
) -> Result<ExecutionOutput, ImplExecutionError> {
    let mut config =
        ExecutorConfig::new(run_root.as_ref().to_path_buf(), isa_base, RiscVImpl::Kronos)
            .map_err(ImplExecutionError::from)?;
    config.timeout = timeout;

    let executor = KronosExecutor::new(config);
    executor.execute(run_root, user_insts).map_err(Into::into)
}
