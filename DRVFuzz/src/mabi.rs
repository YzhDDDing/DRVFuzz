//! Utilities for constructing canonical RISC-V `-mabi` strings from the same
//! extension enums used for building `-march` strings.
//!
//! Given the canonical extension lists defined in `riscv-march-builder`, the
//! helpers below derive the appropriate ABI name while enforcing the standard
//! relationships between base ISAs and floating-point register usage.

use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};

use crate::error::BuildMabiError;

/// Build an `-mabi` string for RV32 from the supplied components.
pub fn mabi_from_rv32_extensions(exts: &[RV32Extensions]) -> Result<String, BuildMabiError> {
    build_mabi(
        BaseFor::Rv32,
        exts.iter().map(|ext| match ext {
            RV32Extensions::I => MabiComponent::Base(BaseKind::Ilp32),
            RV32Extensions::F => MabiComponent::Float(FloatFlavor::F),
            RV32Extensions::D => MabiComponent::Float(FloatFlavor::D),
            RV32Extensions::Q => MabiComponent::Float(FloatFlavor::Q),
            _ => MabiComponent::Other,
        }),
    )
}

/// Build an `-mabi` string for RV64 from the supplied components.
pub fn mabi_from_rv64_extensions(exts: &[RV64Extensions]) -> Result<String, BuildMabiError> {
    build_mabi(
        BaseFor::Rv64,
        exts.iter().map(|ext| match ext {
            RV64Extensions::I => MabiComponent::Base(BaseKind::Lp64),
            RV64Extensions::F => MabiComponent::Float(FloatFlavor::F),
            RV64Extensions::D => MabiComponent::Float(FloatFlavor::D),
            RV64Extensions::Q => MabiComponent::Float(FloatFlavor::Q),
            _ => MabiComponent::Other,
        }),
    )
}

enum BaseFor {
    Rv32,
    Rv64,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum BaseKind {
    Ilp32,
    Lp64,
}

impl BaseKind {
    fn as_str(self) -> &'static str {
        match self {
            BaseKind::Ilp32 => "ilp32",
            BaseKind::Lp64 => "lp64",
        }
    }
}

enum MabiComponent {
    Base(BaseKind),
    Float(FloatFlavor),
    Other,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum FloatFlavor {
    F,
    D,
    Q,
}

#[derive(Default)]
struct FloatPresence {
    has_f: bool,
    has_d: bool,
    has_q: bool,
}

impl FloatPresence {
    fn record(&mut self, flavor: FloatFlavor) {
        match flavor {
            FloatFlavor::F => self.has_f = true,
            FloatFlavor::D => self.has_d = true,
            FloatFlavor::Q => self.has_q = true,
        }
    }

    fn validate(&self) -> Result<(), BuildMabiError> {
        if self.has_q && !self.has_d {
            return Err(BuildMabiError::FloatRequires {
                ext: "q",
                required: "d",
            });
        }

        if self.has_d && !self.has_f {
            return Err(BuildMabiError::FloatRequires {
                ext: "d",
                required: "f",
            });
        }

        Ok(())
    }

    fn highest_suffix(&self) -> Option<char> {
        if self.has_q {
            Some('q')
        } else if self.has_d {
            Some('d')
        } else if self.has_f {
            Some('f')
        } else {
            None
        }
    }
}

fn build_mabi<I>(base_for: BaseFor, components: I) -> Result<String, BuildMabiError>
where
    I: IntoIterator<Item = MabiComponent>,
{
    let mut base: Option<BaseKind> = None;
    let mut floats = FloatPresence::default();

    for component in components {
        match component {
            MabiComponent::Base(candidate) => {
                if let Some(existing) = base {
                    if existing != candidate {
                        return Err(BuildMabiError::ConflictingBase {
                            existing: existing.as_str().to_string(),
                            requested: candidate.as_str().to_string(),
                        });
                    }
                } else {
                    base = Some(candidate);
                }
            }
            MabiComponent::Float(flavor) => {
                floats.record(flavor);
            }
            MabiComponent::Other => {}
        }
    }

    let base = match base {
        Some(base) => base,
        None => match base_for {
            BaseFor::Rv32 => return Err(BuildMabiError::MissingBase),
            BaseFor::Rv64 => return Err(BuildMabiError::MissingBase),
        },
    };

    floats.validate()?;

    let mut mabi = base.as_str().to_string();
    if let Some(suffix) = floats.highest_suffix() {
        mabi.push(suffix);
    }

    Ok(mabi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn rv32_ilp32d() {
        let mabi =
            mabi_from_rv32_extensions(&[RV32Extensions::I, RV32Extensions::F, RV32Extensions::D])
                .unwrap();
        assert_eq!(mabi, "ilp32d");
    }

    #[test]
    fn rv64_lp64q() {
        let mabi = mabi_from_rv64_extensions(&[
            RV64Extensions::I,
            RV64Extensions::F,
            RV64Extensions::D,
            RV64Extensions::Q,
        ])
        .unwrap();
        assert_eq!(mabi, "lp64q");
    }

    #[test]
    fn requires_base() {
        let err = mabi_from_rv32_extensions(&[RV32Extensions::F]).unwrap_err();
        assert_eq!(err, BuildMabiError::MissingBase);
    }

    #[test]
    fn float_suffix_order() {
        let mabi =
            mabi_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::F, RV64Extensions::D])
                .unwrap();
        assert_eq!(mabi, "lp64d");
    }

    #[test]
    fn d_requires_f_for_abi() {
        let err = mabi_from_rv32_extensions(&[RV32Extensions::I, RV32Extensions::D]).unwrap_err();
        assert_eq!(
            err,
            BuildMabiError::FloatRequires {
                ext: "d",
                required: "f",
            }
        );
    }

    #[test]
    fn q_requires_d_for_abi() {
        let err = mabi_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::Q]).unwrap_err();
        assert_eq!(
            err,
            BuildMabiError::FloatRequires {
                ext: "q",
                required: "d",
            }
        );
    }
}
