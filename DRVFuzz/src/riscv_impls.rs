use std::{collections::HashMap, fmt::Display, path::Path, str::FromStr, time::Duration};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use crate::{
    error::{ConfigError, ExecutionError},
    execution_output::ExecutionOutput,
    extension_map::ExtensionMap,
    impls,
    isa_base::ISABase,
};

#[derive(
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Hash,
    EnumIter,
    ValueEnum,
)]
pub enum RiscVImpl {
    Spike,
    Rocket,
    BoomV3,
    BoomV4,
    PicoRV32,
    CVA6,
    XiangShan,
    Ibex,
    Vex,
    Kronos,
    Srv32,
}

impl Display for RiscVImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            RiscVImpl::Spike => "Spike",
            RiscVImpl::Rocket => "Rocket",
            RiscVImpl::BoomV3 => "Boom_v3",
            RiscVImpl::BoomV4 => "Boom_v4",
            RiscVImpl::PicoRV32 => "PicoRV32",
            RiscVImpl::CVA6 => "CVA6",
            RiscVImpl::XiangShan => "XiangShan",
            RiscVImpl::Ibex => "Ibex",
            RiscVImpl::Vex => "Vex",
            RiscVImpl::Kronos => "Kronos",
            RiscVImpl::Srv32 => "Srv32",
        };
        write!(f, "{name}")
    }
}

impl FromStr for RiscVImpl {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "spike" => Ok(RiscVImpl::Spike),
            "rocket" => Ok(RiscVImpl::Rocket),
            "boom-v3" | "boomv3" | "boom_v3" => Ok(RiscVImpl::BoomV3),
            "boom-v4" | "boomv4" | "boom_v4" => Ok(RiscVImpl::BoomV4),
            "picorv32" => Ok(RiscVImpl::PicoRV32),
            "cva6" => Ok(RiscVImpl::CVA6),
            "xiangshan" => Ok(RiscVImpl::XiangShan),
            "ibex" => Ok(RiscVImpl::Ibex),
            "vex" => Ok(RiscVImpl::Vex),
            "kronos" => Ok(RiscVImpl::Kronos),
            "srv32" => Ok(RiscVImpl::Srv32),
            other => Err(format!("unknown implementation {other}")),
        }
    }
}

impl RiscVImpl {
    pub fn supported_isa_bases(&self) -> Vec<ISABase> {
        match self {
            RiscVImpl::Spike => impls::spike::supported_isa_bases(),
            RiscVImpl::Rocket => impls::rocket::supported_isa_bases(),
            RiscVImpl::BoomV3 => impls::boom::supported_isa_bases(),
            RiscVImpl::BoomV4 => impls::boom::supported_isa_bases(),
            RiscVImpl::PicoRV32 => impls::picorv32::supported_isa_bases(),
            RiscVImpl::CVA6 => impls::cva6::supported_isa_bases(),
            RiscVImpl::XiangShan => impls::xiangshan::supported_isa_bases(),
            RiscVImpl::Ibex => impls::ibex::supported_isa_bases(),
            RiscVImpl::Vex => impls::vex::supported_isa_bases(),
            RiscVImpl::Kronos => impls::kronos::supported_isa_bases(),
            RiscVImpl::Srv32 => impls::srv32::supported_isa_bases(),
        }
    }

    pub fn extension_map(&self) -> ExtensionMap {
        match self {
            RiscVImpl::Spike => impls::spike::extensions(),
            RiscVImpl::Rocket => impls::rocket::extensions(),
            RiscVImpl::BoomV3 => impls::boom::extensions_v3(),
            RiscVImpl::BoomV4 => impls::boom::extensions_v4(),
            RiscVImpl::PicoRV32 => impls::picorv32::extensions(),
            RiscVImpl::CVA6 => impls::cva6::extensions(),
            RiscVImpl::XiangShan => impls::xiangshan::extensions(),
            RiscVImpl::Ibex => impls::ibex::extensions(),
            RiscVImpl::Vex => impls::vex::extensions(),
            RiscVImpl::Kronos => impls::kronos::extensions(),
            RiscVImpl::Srv32 => impls::srv32::extensions(),
        }
    }

    pub fn linker_script_content(&self) -> &'static str {
        match self {
            RiscVImpl::Spike => impls::spike::linker_script_content(),
            RiscVImpl::Rocket => impls::rocket::linker_script_content(),
            RiscVImpl::BoomV3 => impls::boom::linker_script_content(),
            RiscVImpl::BoomV4 => impls::boom::linker_script_content(),
            RiscVImpl::PicoRV32 => impls::picorv32::linker_script_content(),
            RiscVImpl::CVA6 => impls::cva6::linker_script_content(),
            RiscVImpl::XiangShan => impls::xiangshan::linker_script_content(),
            RiscVImpl::Ibex => impls::ibex::linker_script_content(),
            RiscVImpl::Vex => impls::vex::linker_script_content(),
            RiscVImpl::Kronos => impls::kronos::linker_script_content(),
            RiscVImpl::Srv32 => impls::srv32::linker_script_content(),
        }
    }

    pub fn user_mem_range(&self) -> (u64, u64) {
        match self {
            RiscVImpl::Spike => impls::spike::user_mem_range(),
            RiscVImpl::Rocket => impls::rocket::user_mem_range(),
            RiscVImpl::BoomV3 => impls::boom::user_mem_range(),
            RiscVImpl::BoomV4 => impls::boom::user_mem_range(),
            RiscVImpl::PicoRV32 => impls::picorv32::user_mem_range(),
            RiscVImpl::CVA6 => impls::cva6::user_mem_range(),
            RiscVImpl::XiangShan => impls::xiangshan::user_mem_range(),
            RiscVImpl::Ibex => impls::ibex::user_mem_range(),
            RiscVImpl::Vex => impls::vex::user_mem_range(),
            RiscVImpl::Kronos => impls::kronos::user_mem_range(),
            RiscVImpl::Srv32 => impls::srv32::user_mem_range(),
        }
    }

    pub fn user_mem_start(&self) -> u64 {
        self.user_mem_range().0
    }

    pub fn supported_unaligned_access_modes(&self) -> Vec<bool> {
        match self {
            RiscVImpl::Spike => impls::spike::supported_unaligned_access_modes(),
            RiscVImpl::Rocket => impls::rocket::supported_unaligned_access_modes(),
            RiscVImpl::BoomV3 => impls::boom::supported_unaligned_access_modes(),
            RiscVImpl::BoomV4 => impls::boom::supported_unaligned_access_modes(),
            RiscVImpl::PicoRV32 => impls::picorv32::supported_unaligned_access_modes(),
            RiscVImpl::CVA6 => impls::cva6::supported_unaligned_access_modes(),
            RiscVImpl::XiangShan => impls::xiangshan::supported_unaligned_access_modes(),
            RiscVImpl::Ibex => impls::ibex::supported_unaligned_access_modes(),
            RiscVImpl::Vex => impls::vex::supported_unaligned_access_modes(),
            RiscVImpl::Kronos => impls::kronos::supported_unaligned_access_modes(),
            RiscVImpl::Srv32 => impls::srv32::supported_unaligned_access_modes(),
        }
    }

    pub fn default_unaligned_access_support(&self) -> bool {
        self.supported_unaligned_access_modes()
            .into_iter()
            .next()
            .unwrap_or(false)
    }

    pub fn supported_unaligned_access_modes_with_overrides(
        &self,
        overrides: &HashMap<RiscVImpl, bool>,
    ) -> Vec<bool> {
        if let Some(&forced) = overrides.get(self) {
            let base = self.supported_unaligned_access_modes();
            if base.contains(&forced) {
                return vec![forced];
            }
        }
        self.supported_unaligned_access_modes()
    }

    pub fn supports_unaligned_requirement(
        &self,
        requirement: bool,
        overrides: &HashMap<RiscVImpl, bool>,
    ) -> bool {
        self.supported_unaligned_access_modes_with_overrides(overrides)
            .contains(&requirement)
    }

    pub fn build_asm_content(
        &self,
        user_insts: &[String],
        isa_base: ISABase,
    ) -> Result<String, ConfigError> {
        match self {
            RiscVImpl::Spike => impls::spike::build_asm_content(user_insts, isa_base),
            RiscVImpl::Rocket => impls::rocket::build_asm_content(user_insts, isa_base),
            RiscVImpl::BoomV3 => impls::boom::build_asm_content(user_insts, isa_base),
            RiscVImpl::BoomV4 => impls::boom::build_asm_content(user_insts, isa_base),
            RiscVImpl::PicoRV32 => impls::picorv32::build_asm_content(user_insts, isa_base),
            RiscVImpl::CVA6 => impls::cva6::build_asm_content(user_insts, isa_base),
            RiscVImpl::XiangShan => impls::xiangshan::build_asm_content(user_insts, isa_base),
            RiscVImpl::Ibex => impls::ibex::build_asm_content(user_insts, isa_base),
            RiscVImpl::Vex => impls::vex::build_asm_content(user_insts, isa_base),
            RiscVImpl::Kronos => impls::kronos::build_asm_content(user_insts, isa_base),
            RiscVImpl::Srv32 => impls::srv32::build_asm_content(user_insts, isa_base),
        }
    }

    pub fn execute<P: AsRef<Path>>(
        &self,
        run_root: P,
        isa_base: ISABase,
        user_insts: &[String],
        timeout: Option<Duration>,
        allow_unaligned: bool,
    ) -> Result<ExecutionOutput, ExecutionError> {
        self.execute_with_extension_override(
            run_root,
            isa_base,
            user_insts,
            timeout,
            None,
            allow_unaligned,
        )
    }

    pub fn execute_with_extension_override<P: AsRef<Path>>(
        &self,
        run_root: P,
        isa_base: ISABase,
        user_insts: &[String],
        timeout: Option<Duration>,
        extension_override: Option<&ExtensionMap>,
        allow_unaligned: bool,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let run_root = run_root.as_ref();
        let impl_name = format!("{:?}", self);
        let isa_base_name = format!("{:?}", isa_base);

        if !self.supported_isa_bases().contains(&isa_base) {
            return Err(ExecutionError::UnsupportedIsaBase {
                impl_name: impl_name.clone(),
                isa_base: isa_base_name.clone(),
            });
        }

        if !self
            .supported_unaligned_access_modes()
            .contains(&allow_unaligned)
        {
            return Err(ExecutionError::UnsupportedAlignmentMode {
                impl_name: impl_name.clone(),
                allow_unaligned,
            });
        }

        match self {
            RiscVImpl::Spike => impls::spike::execute_with_extension_override(
                run_root,
                isa_base,
                user_insts,
                timeout,
                extension_override,
                allow_unaligned,
            )
            .map_err(|e| ExecutionError::ImplementationFailed {
                impl_name: impl_name.clone(),
                isa_base: isa_base_name.clone(),
                source: e,
            }),
            RiscVImpl::Rocket => impls::rocket::execute_with_extension_override(
                run_root,
                isa_base,
                user_insts,
                timeout,
                extension_override,
                allow_unaligned,
            )
            .map_err(|e| ExecutionError::ImplementationFailed {
                impl_name: impl_name.clone(),
                isa_base: isa_base_name.clone(),
                source: e,
            }),
            RiscVImpl::BoomV3 | RiscVImpl::BoomV4 => {
                impls::boom::execute_with_extension_override_for_impl(
                    *self,
                    run_root,
                    isa_base,
                    user_insts,
                    timeout,
                    extension_override,
                    allow_unaligned,
                )
                .map_err(|e| ExecutionError::ImplementationFailed {
                    impl_name: impl_name.clone(),
                    isa_base: isa_base_name.clone(),
                    source: e,
                })
            }
            RiscVImpl::PicoRV32 => {
                impls::picorv32::execute(run_root, isa_base, user_insts, timeout, allow_unaligned)
                    .map_err(|e| ExecutionError::ImplementationFailed {
                        impl_name: impl_name.clone(),
                        isa_base: isa_base_name.clone(),
                        source: e,
                    })
            }
            RiscVImpl::CVA6 => {
                impls::cva6::execute(run_root, isa_base, user_insts, timeout, allow_unaligned)
                    .map_err(|e| ExecutionError::ImplementationFailed {
                        impl_name: impl_name.clone(),
                        isa_base: isa_base_name.clone(),
                        source: e,
                    })
            }
            RiscVImpl::XiangShan => impls::xiangshan::execute_with_extension_override(
                run_root,
                isa_base,
                user_insts,
                timeout,
                extension_override,
                allow_unaligned,
            )
            .map_err(|e| ExecutionError::ImplementationFailed {
                impl_name: impl_name.clone(),
                isa_base: isa_base_name.clone(),
                source: e,
            }),
            RiscVImpl::Ibex => {
                impls::ibex::execute(run_root, isa_base, user_insts, timeout, allow_unaligned)
                    .map_err(|e| ExecutionError::ImplementationFailed {
                        impl_name: impl_name.clone(),
                        isa_base: isa_base_name.clone(),
                        source: e,
                    })
            }
            RiscVImpl::Vex => impls::vex::execute_with_extension_override(
                run_root,
                isa_base,
                user_insts,
                timeout,
                extension_override,
                allow_unaligned,
            )
            .map_err(|e| ExecutionError::ImplementationFailed {
                impl_name: impl_name.clone(),
                isa_base: isa_base_name.clone(),
                source: e,
            }),
            RiscVImpl::Kronos => {
                impls::kronos::execute(run_root, isa_base, user_insts, timeout, allow_unaligned)
                    .map_err(|e| ExecutionError::ImplementationFailed {
                        impl_name: impl_name.clone(),
                        isa_base: isa_base_name.clone(),
                        source: e,
                    })
            }
            RiscVImpl::Srv32 => {
                impls::srv32::execute(run_root, isa_base, user_insts, timeout, allow_unaligned)
                    .map_err(|e| ExecutionError::ImplementationFailed {
                        impl_name: impl_name.clone(),
                        isa_base: isa_base_name.clone(),
                        source: e,
                    })
            }
        }
    }
}
