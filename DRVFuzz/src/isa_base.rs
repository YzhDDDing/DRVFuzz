use std::fmt::Display;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum)]
pub enum ISABase {
    Rv32,
    Rv64,
}

impl Display for ISABase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ISABase::Rv32 => "RV32",
            ISABase::Rv64 => "RV64",
        };
        write!(f, "{name}")
    }
}

impl ISABase {
    pub fn to_str(&self) -> &'static str {
        match self {
            ISABase::Rv32 => "rv32",
            ISABase::Rv64 => "rv64",
        }
    }

    // Natural word size in bytes
    pub fn word_size(&self) -> usize {
        match self {
            ISABase::Rv32 => 4,
            ISABase::Rv64 => 8,
        }
    }

    // Maximum memory access width for an instruction, in bytes
    pub fn instruction_max_access_width(&self) -> usize {
        match self {
            ISABase::Rv32 => 4, // LW is the largest
            ISABase::Rv64 => 8, // LD is the largest
        }
    }
}
