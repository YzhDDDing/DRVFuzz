use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};
use serde::{Deserialize, Serialize};

use crate::{
    error::{BuildMabiError, BuildMarchError},
    isa_base::ISABase,
    mabi::{mabi_from_rv32_extensions, mabi_from_rv64_extensions},
    march::{march_from_rv32_extensions, march_from_rv64_extensions},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMap {
    pub rv32: Vec<RV32Extensions>,
    pub rv64: Vec<RV64Extensions>,
}

impl ExtensionMap {
    pub fn support_float(&self, isa_base: &ISABase) -> bool {
        match isa_base {
            ISABase::Rv32 => {
                self.rv32.contains(&RV32Extensions::F)
                    || self.rv32.contains(&RV32Extensions::D)
                    || self.rv32.contains(&RV32Extensions::Q)
            }
            ISABase::Rv64 => {
                self.rv64.contains(&RV64Extensions::F)
                    || self.rv64.contains(&RV64Extensions::D)
                    || self.rv64.contains(&RV64Extensions::Q)
            }
        }
    }

    // Build a -march string from extensions
    pub fn build_march(&self, isa_base: &ISABase) -> Result<String, BuildMarchError> {
        match isa_base {
            ISABase::Rv32 => {
                if self.rv32.is_empty() {
                    return Err(BuildMarchError::UnsupportedIsaBase);
                }
                march_from_rv32_extensions(&self.rv32)
            }
            ISABase::Rv64 => {
                if self.rv64.is_empty() {
                    return Err(BuildMarchError::UnsupportedIsaBase);
                }
                march_from_rv64_extensions(&self.rv64)
            }
        }
    }
    // Build a -mabi string from extensions
    pub fn build_mabi(&self, isa_base: &ISABase) -> Result<String, BuildMabiError> {
        match isa_base {
            ISABase::Rv32 => {
                if self.rv32.is_empty() {
                    return Err(BuildMabiError::UnsupportedIsaBase);
                }
                mabi_from_rv32_extensions(&self.rv32)
            }
            ISABase::Rv64 => {
                if self.rv64.is_empty() {
                    return Err(BuildMabiError::UnsupportedIsaBase);
                }
                mabi_from_rv64_extensions(&self.rv64)
            }
        }
    }
}
