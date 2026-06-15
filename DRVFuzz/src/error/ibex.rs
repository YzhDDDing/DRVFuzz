use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum IbexError {
    #[error("config error: {0}")]
    Config(String),

    #[error("ibex binary env var not set: {var}")]
    EnvVarNotSet { var: String },

    #[error("ibex binary not found at {path}")]
    BinaryNotFound { path: PathBuf },

    #[error(transparent)]
    Elf(#[from] crate::error::BuildElfError),

    #[error(transparent)]
    Process(#[from] crate::error::ProcessError),

    #[error(transparent)]
    Parse(#[from] crate::error::ParseError),

    #[error(transparent)]
    LogParse(#[from] crate::error::LogParseError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

