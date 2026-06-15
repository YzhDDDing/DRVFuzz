#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn rv64_implied_atomic() {
        let march = march_from_rv64_extensions(&[
            RV64Extensions::I,
            RV64Extensions::M,
            RV64Extensions::F,
            RV64Extensions::D,
            RV64Extensions::C,
            RV64Extensions::Zaamo,
            RV64Extensions::Zalrsc,
            RV64Extensions::Zicsr,
        ])
        .unwrap();
        assert_eq!(march, "rv64imafdc_zicsr_zmmul_zaamo_zalrsc_zfa_zca_zcd");
    }

    #[test]
    fn rv32_vector_subset_contains_expected_extensions() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::F,
            RV32Extensions::C,
            RV32Extensions::Zfbfmin,
            RV32Extensions::V,
            RV32Extensions::Zvfbfmin,
            RV32Extensions::Zvbb,
            RV32Extensions::Zvfbfwma,
            RV32Extensions::Zvknha,
            RV32Extensions::Zicsr,
        ])
        .unwrap();

        assert!(march.starts_with("rv32imfdc"));
        assert!(march.contains("_zmmul"));
        assert!(march.contains("_zfbfmin"));
        assert!(march.contains("_zfhmin"));
        assert!(march.contains("_zvfbfmin"));
        assert!(march.contains("_zvfbfwma"));
        assert!(march.contains("_zvbb"));
        assert!(march.contains("_zvknha"));
        assert!(march.contains("_zicsr"));
    }

    #[test]
    fn d_implies_f_and_zicsr() {
        let march = march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::D]).unwrap();
        assert_eq!(march, "rv64ifd_zicsr_zfa");
    }

    #[test]
    fn f_implies_zicsr() {
        let march = march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::F]).unwrap();
        assert_eq!(march, "rv64if_zicsr_zfa");
    }

    #[test]
    fn q_is_unsupported() {
        let err = march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::Q]).unwrap_err();
        match err {
            BuildMarchError::UnsupportedExtension { ext } => assert_eq!(ext, "q"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn c_implies_zca() {
        let march = march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::C]).unwrap();
        assert_eq!(march, "rv64ic_zicsr_zca");
    }

    #[test]
    fn rv32_c_with_f_implies_zcf() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::F,
            RV32Extensions::C,
            RV32Extensions::Zicsr,
        ])
        .unwrap();
        assert_eq!(march, "rv32ifc_zicsr_zfa_zca_zcf");
    }

    #[test]
    fn rv32_float_without_double_does_not_include_d() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::F,
            RV32Extensions::B,
            RV32Extensions::C,
            RV32Extensions::Zaamo,
            RV32Extensions::Zalrsc,
            RV32Extensions::Zba,
            RV32Extensions::Zbb,
            RV32Extensions::Zbc,
            RV32Extensions::Zbkb,
            RV32Extensions::Zbkx,
            RV32Extensions::Zbs,
            RV32Extensions::Zcb,
            RV32Extensions::Zicond,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::Zknd,
            RV32Extensions::Zkne,
            RV32Extensions::Zknh,
        ])
        .unwrap();

        let base = march.split('_').next().unwrap();
        assert!(base.contains('f'));
        assert!(!base.contains('d'));
    }

    #[test]
    fn c_with_d_implies_zcd() {
        let march = march_from_rv64_extensions(&[
            RV64Extensions::I,
            RV64Extensions::F,
            RV64Extensions::D,
            RV64Extensions::C,
            RV64Extensions::Zicsr,
        ])
        .unwrap();
        assert_eq!(march, "rv64ifdc_zicsr_zfa_zca_zcd");
    }

    #[test]
    fn zcmop_implies_zca() {
        let march = march_from_rv64_extensions(&[
            RV64Extensions::I,
            RV64Extensions::C,
            RV64Extensions::Zcmop,
        ])
        .unwrap();
        assert!(march.contains("_zca"));
    }

    #[test]
    fn unsupported_sdext_returns_error() {
        let err =
            march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::Sdext]).unwrap_err();
        match err {
            BuildMarchError::UnsupportedExtension { ext } => assert_eq!(ext, "sdext"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn b_implies_basic_bitmanip_extensions() {
        let march = march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::B]).unwrap();
        assert!(march.contains("_zba"));
        assert!(march.contains("_zbb"));
        assert!(march.contains("_zbs"));
    }

    #[test]
    fn zvfbfmin_implies_f() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::V,
            RV32Extensions::Zvfbfmin,
        ])
        .unwrap();
        let base = march.split('_').next().unwrap();
        assert!(base.starts_with("rv32i"), "unexpected base `{base}`");
        for required in ['f', 'd', 'v'] {
            assert!(
                base.contains(required),
                "missing `{required}` in base `{base}`"
            );
        }
        assert!(march.contains("_zvfbfmin"));
        assert!(march.contains("_zve32f"));
    }

    #[test]
    fn rejects_missing_base() {
        let err = march_from_rv32_extensions(&[RV32Extensions::M]).unwrap_err();
        assert_eq!(err, BuildMarchError::MissingBaseIsa);
    }

    #[test]
    fn rejects_conflicting_base() {
        let err = build_march(32, ["i".to_string(), "e".to_string()].into_iter()).unwrap_err();
        assert_eq!(err, BuildMarchError::ConflictingBaseIsa);
    }

    #[test]
    fn rv32_h_requires_i() {
        let mut exts = HashSet::new();
        exts.insert("h".to_string());
        let err = validate_extensions(32, &exts).unwrap_err();
        match err {
            BuildMarchError::ExtensionRequires { ext, required } => {
                assert_eq!(ext, "h");
                assert_eq!(required, "i");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rv64_rejects_zcf() {
        let err = build_march(64, ["i".to_string(), "zcf".to_string()].into_iter()).unwrap_err();
        match err {
            BuildMarchError::ExtensionOnlyForXlen { ext, required_xlen } => {
                assert_eq!(ext, "zcf");
                assert_eq!(required_xlen, 32);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn zcd_conflicts_with_zcmp() {
        let err = march_from_rv64_extensions(&[
            RV64Extensions::I,
            RV64Extensions::Zcd,
            RV64Extensions::Zcmp,
        ])
        .unwrap_err();
        match err {
            BuildMarchError::ExtensionsConflict { left, right } => {
                assert_eq!(left, "zcd");
                assert_eq!(right, "zcmp");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn zabha_implies_zaamo() {
        let march =
            march_from_rv64_extensions(&[RV64Extensions::I, RV64Extensions::Zabha]).unwrap();
        assert!(march.contains("_zaamo"));
    }

    #[test]
    fn zvfbfwma_implies_zvfbfmin() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::V,
            RV32Extensions::Zvfbfwma,
        ])
        .unwrap();
        assert!(march.contains("_zvfbfmin"));
    }

    #[test]
    fn zvfbfwma_implies_zfbfmin() {
        let march = march_from_rv32_extensions(&[
            RV32Extensions::I,
            RV32Extensions::F,
            RV32Extensions::V,
            RV32Extensions::Zvfbfmin,
            RV32Extensions::Zvfbfwma,
            RV32Extensions::Zicsr,
        ])
        .unwrap();
        assert!(march.contains("_zfbfmin"));
    }

    #[test]
    fn zfinx_conflicts_with_f() {
        let mut exts = HashSet::new();
        exts.insert("zfinx".to_string());
        exts.insert("f".to_string());
        let err = validate_extensions(32, &exts).unwrap_err();
        match err {
            BuildMarchError::ExtensionsConflict { left, right } => {
                assert_eq!(left, "zfinx");
                assert_eq!(right, "f");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn xtheadvector_conflicts_with_vector_extensions() {
        let mut exts = HashSet::new();
        exts.insert("xtheadvector".to_string());
        exts.insert("v".to_string());
        let err = validate_extensions(64, &exts).unwrap_err();
        match err {
            BuildMarchError::ExtensionsConflict { left, right } => {
                assert_eq!(left, "xtheadvector");
                assert_eq!(right, "v");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn supported_extension_list_matches_gnu_definition() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let gnu_source =
            Path::new(manifest_dir).join("../../gcc/gcc/common/config/riscv/riscv-common.cc");

        if !gnu_source.exists() {
            log::warn!(
                "skipping comparison against gnu sources: `{}` missing",
                gnu_source.display()
            );
            return;
        }

        let content = std::fs::read_to_string(&gnu_source).expect("failed to read riscv-common.cc");

        let mut gnu_names = HashSet::new();
        let mut in_table = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed
                .starts_with("static const struct riscv_ext_version riscv_ext_version_table[] =")
            {
                in_table = true;
                continue;
            }

            if in_table {
                if trimmed.starts_with("};") {
                    break;
                }

                if let Some(rest) = trimmed.strip_prefix("{\"") {
                    if let Some(end) = rest.find('\"') {
                        let name = &rest[..end];
                        if name != "NULL" {
                            gnu_names.insert(name.to_string());
                        }
                    }
                }
            }
        }

        let builder_names: HashSet<String> = SUPPORTED_EXTENSIONS
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        assert_eq!(
            builder_names, gnu_names,
            "supported extension list diverges from gnu riscv-common.cc"
        );
    }
}
// Utilities for canonicalizing RISC-V ISA strings (aka `-march` values).
//
// The logic implemented in this module mirrors the canonical ordering rules
// used by GCC/binutils, which can be found under
// `gcc/gcc/common/config/riscv/riscv-common.cc` in this repository.  In
// particular, single-letter extensions appear in the order defined by
// `riscv_supported_std_ext`, while multi-letter extensions are grouped by
// their prefix (`z`, `s`, `x`) and then ordered alphabetically.  This matches
// the behaviour of `riscv_subset_list::to_string` that eventually feeds the
// assembler.

use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};
use std::collections::HashSet;

use crate::error::BuildMarchError;

#[derive(Copy, Clone)]
struct ImpliedExt {
    base: &'static str,
    implied: &'static str,
    condition: ImpliedCondition,
}

#[derive(Copy, Clone)]
enum ImpliedCondition {
    Always,
    RequiresAll(&'static [&'static str]),
    RequiresAllWithXlen {
        xlen: u32,
        requires: &'static [&'static str],
    },
}

impl ImpliedCondition {
    fn is_met<F>(self, xlen: u32, has: F) -> bool
    where
        F: Fn(&str) -> bool,
    {
        match self {
            ImpliedCondition::Always => true,
            ImpliedCondition::RequiresAll(required) => required.iter().all(|&name| has(name)),
            ImpliedCondition::RequiresAllWithXlen {
                xlen: required_xlen,
                requires,
            } => xlen == required_xlen && requires.iter().all(|&name| has(name)),
        }
    }
}

// The GNU toolchain currently rejects the standard `Q` extension, so we leave
// it out of the supported set even though the spec defines it.
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "i",
    "e",
    "m",
    "a",
    "f",
    "d",
    "c",
    "b",
    "h",
    "v",
    "zicsr",
    "zifencei",
    "zicond",
    "za64rs",
    "za128rs",
    "zabha",
    "zacas",
    "zaamo",
    "zalrsc",
    "zawrs",
    "zba",
    "zbb",
    "zbc",
    "zbs",
    "zfinx",
    "zdinx",
    "zhinx",
    "zhinxmin",
    "zbkb",
    "zbkc",
    "zbkx",
    "zkne",
    "zknd",
    "zknh",
    "zkr",
    "zksed",
    "zksh",
    "zkt",
    "zihintntl",
    "zihintpause",
    "zicboz",
    "zicbom",
    "zicbop",
    "zic64b",
    "ziccamoa",
    "ziccif",
    "zicclsm",
    "ziccrse",
    "zicfiss",
    "zicfilp",
    "zimop",
    "zcmop",
    "zicntr",
    "zihpm",
    "zk",
    "zkn",
    "zks",
    "ztso",
    "zve32x",
    "zve32f",
    "zve64x",
    "zve64f",
    "zve64d",
    "zvbb",
    "zvbc",
    "zvkb",
    "zvkg",
    "zvkn",
    "zvknc",
    "zvkng",
    "zvkned",
    "zvknha",
    "zvknhb",
    "zvks",
    "zvksc",
    "zvksg",
    "zvksed",
    "zvksh",
    "zvkt",
    "zvl32b",
    "zvl64b",
    "zvl128b",
    "zvl256b",
    "zvl512b",
    "zvl1024b",
    "zvl2048b",
    "zvl4096b",
    "zvl8192b",
    "zvl16384b",
    "zvl32768b",
    "zvl65536b",
    "zfbfmin",
    "zfh",
    "zfhmin",
    "zvfbfmin",
    "zvfbfwma",
    "zvfhmin",
    "zvfh",
    "zfa",
    "zmmul",
    "zca",
    "zcb",
    "zce",
    "zcf",
    "zcd",
    "zcmp",
    "zcmt",
    "smaia",
    "smepmp",
    "smstateen",
    "ssaia",
    "sscofpmf",
    "ssstateen",
    "sstc",
    "svinval",
    "svnapot",
    "svpbmt",
    "svvptc",
    "xcvmac",
    "xcvalu",
    "xcvelw",
    "xcvsimd",
    "xcvbi",
    "xtheadba",
    "xtheadbb",
    "xtheadbs",
    "xtheadcmo",
    "xtheadcondmov",
    "xtheadfmemidx",
    "xtheadfmv",
    "xtheadint",
    "xtheadmac",
    "xtheadmemidx",
    "xtheadmempair",
    "xtheadsync",
    "xtheadvector",
    "xventanacondops",
    "xsfvcp",
    "xsfcease",
    "xsfvqmaccqoq",
    "xsfvqmaccdod",
    "xsfvfnrclipxfqf",
];

const IMPLIED_EXTENSIONS: &[ImpliedExt] = &[
    ImpliedExt {
        base: "m",
        implied: "zmmul",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "d",
        implied: "f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "f",
        implied: "zfa",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "f",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "d",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "a",
        implied: "zaamo",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "a",
        implied: "zalrsc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "c",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "c",
        implied: "zcf",
        condition: ImpliedCondition::RequiresAllWithXlen {
            xlen: 32,
            requires: &["f"],
        },
    },
    ImpliedExt {
        base: "c",
        implied: "zcd",
        condition: ImpliedCondition::RequiresAll(&["d"]),
    },
    ImpliedExt {
        base: "zabha",
        implied: "zaamo",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zacas",
        implied: "zaamo",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zawrs",
        implied: "zalrsc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zcmop",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "b",
        implied: "zba",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "b",
        implied: "zbb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "b",
        implied: "zbs",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zdinx",
        implied: "zfinx",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zfinx",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zdinx",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zicfiss",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zicfiss",
        implied: "zimop",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zicfilp",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zk",
        implied: "zkn",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zk",
        implied: "zkr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zk",
        implied: "zkt",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zbkb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zbkc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zbkx",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zkne",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zknd",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zkn",
        implied: "zknh",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zks",
        implied: "zbkb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zks",
        implied: "zbkc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zks",
        implied: "zbkx",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zks",
        implied: "zksed",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zks",
        implied: "zksh",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "v",
        implied: "zvl128b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "v",
        implied: "zve64d",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve32f",
        implied: "f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64f",
        implied: "f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64d",
        implied: "d",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve32x",
        implied: "zvl32b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve32f",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve32f",
        implied: "zvl32b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64x",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64x",
        implied: "zvl64b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64f",
        implied: "zve32f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64f",
        implied: "zve64x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64f",
        implied: "zvl64b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64d",
        implied: "zve64f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zve64d",
        implied: "zvl64b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl64b",
        implied: "zvl32b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl128b",
        implied: "zvl64b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl256b",
        implied: "zvl128b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl512b",
        implied: "zvl256b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl1024b",
        implied: "zvl512b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl2048b",
        implied: "zvl1024b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl4096b",
        implied: "zvl2048b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl8192b",
        implied: "zvl4096b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl16384b",
        implied: "zvl8192b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl32768b",
        implied: "zvl16384b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvl65536b",
        implied: "zvl32768b",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkn",
        implied: "zvkned",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkn",
        implied: "zvknhb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkn",
        implied: "zvkb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkn",
        implied: "zvkt",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvknc",
        implied: "zvkn",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvknc",
        implied: "zvbc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkng",
        implied: "zvkn",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkng",
        implied: "zvkg",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvks",
        implied: "zvksed",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvks",
        implied: "zvksh",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvks",
        implied: "zvkb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvks",
        implied: "zvkt",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksc",
        implied: "zvks",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksc",
        implied: "zvbc",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksg",
        implied: "zvks",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksg",
        implied: "zvkg",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvbb",
        implied: "zvkb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvbc",
        implied: "zve64x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkb",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkg",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvkned",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvknha",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvknhb",
        implied: "zve64x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksed",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvksh",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zfbfmin",
        implied: "zfhmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zfh",
        implied: "zfhmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zfhmin",
        implied: "f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zfa",
        implied: "f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfbfmin",
        implied: "zve32f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfbfwma",
        implied: "zvfbfmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfbfwma",
        implied: "zfbfmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfhmin",
        implied: "zve32f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfh",
        implied: "zve32f",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zvfh",
        implied: "zfhmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zhinx",
        implied: "zhinxmin",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zhinxmin",
        implied: "zfinx",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zce",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zce",
        implied: "zcb",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zce",
        implied: "zcmp",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zce",
        implied: "zcmt",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zce",
        implied: "zcf",
        condition: ImpliedCondition::RequiresAllWithXlen {
            xlen: 32,
            requires: &["f"],
        },
    },
    ImpliedExt {
        base: "zcd",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zcb",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zcmp",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zcmt",
        implied: "zca",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "zcmt",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "smaia",
        implied: "ssaia",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "smstateen",
        implied: "ssstateen",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "smepmp",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "ssaia",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "sscofpmf",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "ssstateen",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "sstc",
        implied: "zicsr",
        condition: ImpliedCondition::Always,
    },
    ImpliedExt {
        base: "xsfvcp",
        implied: "zve32x",
        condition: ImpliedCondition::Always,
    },
];

const COMBINED_EXTENSIONS: &[(&str, &[&str])] = &[
    ("a", &["zaamo", "zalrsc"]),
    ("b", &["zba", "zbb", "zbs"]),
    ("zk", &["zkn", "zkr", "zkt"]),
    ("zkn", &["zbkb", "zbkc", "zbkx", "zkne", "zknd", "zknh"]),
    ("zks", &["zbkb", "zbkc", "zbkx", "zksed", "zksh"]),
    ("zvkn", &["zvkned", "zvknhb", "zvkb", "zvkt"]),
    ("zvknc", &["zvkn", "zvbc"]),
    ("zvkng", &["zvkn", "zvkg"]),
    ("zvks", &["zvksed", "zvksh", "zvkb", "zvkt"]),
    ("zvksc", &["zvks", "zvbc"]),
    ("zvksg", &["zvks", "zvkg"]),
];

fn is_supported_extension(name: &str) -> bool {
    SUPPORTED_EXTENSIONS
        .iter()
        .any(|candidate| *candidate == name)
}

/// Build a canonical `-march` string for RV32 from the supplied extensions.
pub fn march_from_rv32_extensions(exts: &[RV32Extensions]) -> Result<String, BuildMarchError> {
    build_march(
        32,
        exts.iter().map(|ext| format!("{:?}", ext).to_lowercase()),
    )
}

/// Build a canonical `-march` string for RV64 from the supplied extensions.
pub fn march_from_rv64_extensions(exts: &[RV64Extensions]) -> Result<String, BuildMarchError> {
    build_march(
        64,
        exts.iter().map(|ext| format!("{:?}", ext).to_lowercase()),
    )
}

fn build_march<I>(xlen: u32, names: I) -> Result<String, BuildMarchError>
where
    I: IntoIterator<Item = String>,
{
    let mut unique: HashSet<String> = HashSet::new();
    for name in names {
        if !is_supported_extension(&name) {
            return Err(BuildMarchError::UnsupportedExtension { ext: name });
        }
        unique.insert(name);
    }

    let has_i = unique.contains("i");
    let has_e = unique.contains("e");

    match (has_i, has_e) {
        (false, false) => return Err(BuildMarchError::MissingBaseIsa),
        (true, true) => return Err(BuildMarchError::ConflictingBaseIsa),
        _ => {}
    }

    // Always include CSR support so assemblers accept CSR instructions.
    unique.insert("zicsr".to_string());

    apply_implied_extensions(xlen, &mut unique);
    apply_combined_extensions(&mut unique);
    validate_extensions(xlen, &unique)?;

    let mut list: Vec<String> = unique.into_iter().collect();
    list.sort_by(|a, b| canonical_cmp(a, b));

    let mut march = format!("rv{}", xlen);
    let mut first = true;
    for name in list {
        if first {
            march.push_str(&name);
            first = false;
            continue;
        }

        if name.len() == 1 {
            march.push_str(&name);
        } else {
            march.push('_');
            march.push_str(&name);
        }
    }

    Ok(march)
}

fn apply_implied_extensions(xlen: u32, exts: &mut HashSet<String>) {
    loop {
        let mut changed = false;
        for implied in IMPLIED_EXTENSIONS {
            if !exts.contains(implied.base) {
                continue;
            }

            let has = |name: &str| exts.contains(name);
            if !implied.condition.is_met(xlen, has) {
                continue;
            }

            if exts.insert(implied.implied.to_string()) {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

fn apply_combined_extensions(exts: &mut HashSet<String>) {
    loop {
        let mut changed = false;
        for (ext, deps) in COMBINED_EXTENSIONS {
            if exts.contains(*ext) {
                continue;
            }

            if deps.iter().all(|dep| exts.contains(*dep)) {
                if exts.insert((*ext).to_string()) {
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }
}

fn validate_extensions(xlen: u32, exts: &HashSet<String>) -> Result<(), BuildMarchError> {
    let has = |name: &str| exts.contains(name);

    if has("h") && !has("i") {
        return Err(BuildMarchError::ExtensionRequires {
            ext: "h".into(),
            required: "i".into(),
        });
    }

    require_all("q", &["d", "f"], &has)?;

    if xlen == 64 && has("zcf") {
        return Err(BuildMarchError::ExtensionOnlyForXlen {
            ext: "zcf".into(),
            required_xlen: 32,
        });
    }

    if has("c") {
        if !has("zca") {
            return Err(BuildMarchError::ExtensionRequires {
                ext: "c".into(),
                required: "zca".into(),
            });
        }

        if xlen == 32 && has("f") && !has("zcf") {
            return Err(BuildMarchError::ExtensionRequires {
                ext: "c".into(),
                required: "zcf".into(),
            });
        }

        if has("d") && !has("zcd") {
            return Err(BuildMarchError::ExtensionRequires {
                ext: "c".into(),
                required: "zcd".into(),
            });
        }
    }

    if has("zcd") {
        if has("zcmp") {
            return Err(BuildMarchError::ExtensionsConflict {
                left: "zcd".into(),
                right: "zcmp".into(),
            });
        }
        if has("zcmt") {
            return Err(BuildMarchError::ExtensionsConflict {
                left: "zcd".into(),
                right: "zcmt".into(),
            });
        }
    }

    if has("zfinx") && has("f") {
        return Err(BuildMarchError::ExtensionsConflict {
            left: "zfinx".into(),
            right: "f".into(),
        });
    }

    if has("xtheadvector") {
        const VECTOR_FAMILY: &[&str] = &[
            "v", "zve32x", "zve64x", "zve32f", "zve64f", "zve64d", "zvl32b", "zvl64b", "zvl128b",
            "zvfh",
        ];
        if let Some(conflict) = VECTOR_FAMILY.iter().find(|&&name| has(name)) {
            return Err(BuildMarchError::ExtensionsConflict {
                left: "xtheadvector".into(),
                right: (*conflict).into(),
            });
        }
    }

    Ok(())
}

fn require_all<F>(ext: &str, requirements: &[&str], has: F) -> Result<(), BuildMarchError>
where
    F: Fn(&str) -> bool,
{
    if !has(ext) {
        return Ok(());
    }

    for req in requirements {
        if !has(req) {
            return Err(BuildMarchError::ExtensionRequires {
                ext: ext.into(),
                required: (*req).into(),
            });
        }
    }

    Ok(())
}

fn canonical_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a == b {
        return Ordering::Equal;
    }

    let a_single = a.len() == 1;
    let b_single = b.len() == 1;

    match (a_single, b_single) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (true, true) => {
            if let (Some(first_a), Some(first_b)) = (a.chars().next(), b.chars().next()) {
                let rank_a = single_letter_rank(first_a);
                let rank_b = single_letter_rank(first_b);
                return rank_a.cmp(&rank_b);
            }
            return a.cmp(b);
        }
        (false, false) => {
            let rank_a = multi_letter_rank(a);
            let rank_b = multi_letter_rank(b);
            if rank_a == rank_b {
                return a.cmp(b);
            }
            return rank_a.cmp(&rank_b);
        }
    }
}

fn single_letter_rank(ext: char) -> i32 {
    const STANDARD_ORDER: &str = "mafdqlcbkjtpvnh";

    match ext {
        'i' => 0,
        'e' => 1,
        _ => {
            if let Some(pos) = STANDARD_ORDER.find(ext) {
                (pos as i32) + 2
            } else {
                (STANDARD_ORDER.len() as i32) + 2 + (ext as i32 - 'a' as i32)
            }
        }
    }
}

fn multi_letter_rank(name: &str) -> (i32, i32) {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return (3, 0);
    };
    let high = match first {
        'z' => 0,
        's' => 1,
        'x' => 2,
        _ => 3,
    };

    let low = if first == 'z' {
        chars.next().map(single_letter_rank).unwrap_or(i32::MAX)
    } else {
        0
    };

    (high, low)
}
