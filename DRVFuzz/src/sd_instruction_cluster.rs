#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstructionCluster {
    BranchJump,
    Vector,
    Load,
    Store,
    Atomic,
    Csr,
    System,
    Fence,
    UpperImmediate,
    IntegerArithmetic,
    IntegerCompare,
    IntegerShift,
    IntegerLogic,
    IntegerBitmanip,
    IntegerMultiply,
    IntegerDivideRemainder,
    Crypto,
    FloatingLoad,
    FloatingStore,
    FloatingArithmetic,
    FloatingCompare,
    FloatingConvert,
    FloatingMove,
    FloatingClass,
    FloatingConstant,
    CompressedStack,
    OtherScalar,
}

impl InstructionCluster {
    pub fn sdmodel_supported(self) -> bool {
        !matches!(self, Self::BranchJump | Self::Vector)
    }

    pub fn is_memory(self) -> bool {
        matches!(
            self,
            Self::Load | Self::Store | Self::FloatingLoad | Self::FloatingStore | Self::Atomic
        )
    }

    pub fn is_store_like(self) -> bool {
        matches!(self, Self::Store | Self::FloatingStore | Self::Atomic)
    }

    pub fn uses_integer_boundary_operands(self) -> bool {
        matches!(
            self,
            Self::Load
                | Self::Store
                | Self::Atomic
                | Self::Csr
                | Self::UpperImmediate
                | Self::IntegerArithmetic
                | Self::IntegerCompare
                | Self::IntegerShift
                | Self::IntegerLogic
                | Self::IntegerBitmanip
                | Self::IntegerMultiply
                | Self::IntegerDivideRemainder
                | Self::Crypto
                | Self::FloatingLoad
                | Self::FloatingStore
                | Self::FloatingConvert
                | Self::FloatingMove
                | Self::CompressedStack
        )
    }

    pub fn is_exception_trigger_instruction(self) -> bool {
        matches!(self, Self::Csr | Self::System | Self::Fence)
    }
}

pub fn classify_mnemonic(mnemonic: &str) -> InstructionCluster {
    let mnemonic = mnemonic.trim().to_ascii_lowercase();
    let base = strip_ordering_suffix(&mnemonic);

    if is_vector_mnemonic(base) {
        return InstructionCluster::Vector;
    }
    if is_branch_jump_mnemonic(base) {
        return InstructionCluster::BranchJump;
    }
    if is_atomic_mnemonic(base) {
        return InstructionCluster::Atomic;
    }
    if is_floating_load_mnemonic(base) {
        return InstructionCluster::FloatingLoad;
    }
    if is_floating_store_mnemonic(base) {
        return InstructionCluster::FloatingStore;
    }
    if is_load_mnemonic(base) {
        return InstructionCluster::Load;
    }
    if is_store_mnemonic(base) {
        return InstructionCluster::Store;
    }
    if is_csr_mnemonic(base) {
        return InstructionCluster::Csr;
    }
    if is_fence_mnemonic(base) {
        return InstructionCluster::Fence;
    }
    if is_system_mnemonic(base) {
        return InstructionCluster::System;
    }
    if is_floating_class_mnemonic(base) {
        return InstructionCluster::FloatingClass;
    }
    if is_floating_constant_mnemonic(base) {
        return InstructionCluster::FloatingConstant;
    }
    if is_floating_convert_mnemonic(base) {
        return InstructionCluster::FloatingConvert;
    }
    if is_floating_move_mnemonic(base) {
        return InstructionCluster::FloatingMove;
    }
    if is_floating_compare_mnemonic(base) {
        return InstructionCluster::FloatingCompare;
    }
    if is_floating_arithmetic_mnemonic(base) {
        return InstructionCluster::FloatingArithmetic;
    }
    if is_div_rem_mnemonic(base) {
        return InstructionCluster::IntegerDivideRemainder;
    }
    if is_multiply_mnemonic(base) {
        return InstructionCluster::IntegerMultiply;
    }
    if is_crypto_mnemonic(base) {
        return InstructionCluster::Crypto;
    }
    if is_upper_immediate_mnemonic(base) {
        return InstructionCluster::UpperImmediate;
    }
    if is_shift_mnemonic(base) {
        return InstructionCluster::IntegerShift;
    }
    if is_compare_mnemonic(base) {
        return InstructionCluster::IntegerCompare;
    }
    if is_logic_mnemonic(base) {
        return InstructionCluster::IntegerLogic;
    }
    if is_bitmanip_mnemonic(base) {
        return InstructionCluster::IntegerBitmanip;
    }
    if is_compressed_stack_mnemonic(base) {
        return InstructionCluster::CompressedStack;
    }
    if is_integer_arithmetic_mnemonic(base) {
        return InstructionCluster::IntegerArithmetic;
    }

    InstructionCluster::OtherScalar
}

pub fn memory_access_width(mnemonic: &str) -> Option<u64> {
    let lower = mnemonic.trim().to_ascii_lowercase();
    let base = strip_ordering_suffix(&lower);

    match base {
        "lb" | "lbu" | "sb" | "c.lbu" | "c.sb" | "hlv.b" | "hlv.bu" | "hsv.b" => Some(1),
        "lh" | "lhu" | "sh" | "c.lh" | "c.lhu" | "c.sh" | "flh" | "fsh" | "c.flh" | "c.fsh"
        | "hlv.h" | "hlv.hu" | "hlvx.hu" | "hsv.h" => Some(2),
        "lw" | "lwu" | "sw" | "c.lw" | "c.sw" | "c.lwsp" | "c.swsp" | "flw" | "fsw" | "c.flw"
        | "c.fsw" | "c.flwsp" | "c.fswsp" | "hlv.w" | "hlv.wu" | "hlvx.wu" | "hsv.w" => Some(4),
        "ld" | "sd" | "c.ld" | "c.sd" | "c.ldsp" | "c.sdsp" | "fld" | "fsd" | "c.fld" | "c.fsd"
        | "c.fldsp" | "c.fsdsp" | "hlv.d" | "hsv.d" => Some(8),
        "flq" | "fsq" => Some(16),
        _ if is_atomic_mnemonic(base) => atomic_width(base),
        _ => None,
    }
}

pub fn is_div_rem_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "div" | "divu" | "rem" | "remu" | "divw" | "divuw" | "remw" | "remuw"
    )
}

pub fn is_signed_div_rem_mnemonic(mnemonic: &str) -> bool {
    matches!(mnemonic, "div" | "rem" | "divw" | "remw")
}

pub fn is_branch_jump_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "beq"
            | "bne"
            | "blt"
            | "bge"
            | "bltu"
            | "bgeu"
            | "jal"
            | "jalr"
            | "c.beqz"
            | "c.bnez"
            | "c.j"
            | "c.jal"
            | "c.jalr"
            | "c.jr"
            | "cm.popret"
            | "cm.popretz"
    )
}

pub fn is_vector_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with('v')
}

fn strip_ordering_suffix(mnemonic: &str) -> &str {
    mnemonic
        .strip_suffix(".aq")
        .or_else(|| mnemonic.strip_suffix(".rl"))
        .unwrap_or(mnemonic)
}

fn is_atomic_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("amo")
        || mnemonic.starts_with("lr.")
        || mnemonic.starts_with("sc.")
        || mnemonic.starts_with("ssamoswap.")
}

fn atomic_width(mnemonic: &str) -> Option<u64> {
    if mnemonic.ends_with(".b") {
        Some(1)
    } else if mnemonic.ends_with(".h") {
        Some(2)
    } else if mnemonic.ends_with(".w") {
        Some(4)
    } else if mnemonic.ends_with(".d") {
        Some(8)
    } else if mnemonic.ends_with(".q") {
        Some(16)
    } else {
        None
    }
}

fn is_load_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "lb" | "lbu"
            | "lh"
            | "lhu"
            | "lw"
            | "lwu"
            | "ld"
            | "c.lbu"
            | "c.lh"
            | "c.lhu"
            | "c.lw"
            | "c.lwsp"
            | "c.ld"
            | "c.ldsp"
            | "hlv.b"
            | "hlv.bu"
            | "hlv.h"
            | "hlv.hu"
            | "hlv.w"
            | "hlv.wu"
            | "hlv.d"
            | "hlvx.hu"
            | "hlvx.wu"
    )
}

fn is_store_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "sb" | "sh"
            | "sw"
            | "sd"
            | "c.sb"
            | "c.sh"
            | "c.sw"
            | "c.swsp"
            | "c.sd"
            | "c.sdsp"
            | "hsv.b"
            | "hsv.h"
            | "hsv.w"
            | "hsv.d"
    )
}

fn is_floating_load_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "flw"
            | "fld"
            | "flh"
            | "flq"
            | "c.flw"
            | "c.flwsp"
            | "c.fld"
            | "c.fldsp"
            | "c.flh"
            | "c.flhsp"
    )
}

fn is_floating_store_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "fsw"
            | "fsd"
            | "fsh"
            | "fsq"
            | "c.fsw"
            | "c.fswsp"
            | "c.fsd"
            | "c.fsdsp"
            | "c.fsh"
            | "c.fshsp"
    )
}

fn is_csr_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "csrrw"
            | "csrrs"
            | "csrrc"
            | "csrrwi"
            | "csrrsi"
            | "csrrci"
            | "frcsr"
            | "frflags"
            | "frrm"
            | "fsflags"
            | "fsflagsi"
            | "fsrm"
            | "fsrmi"
    )
}

fn is_fence_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("fence")
        || mnemonic.starts_with("sfence")
        || mnemonic.starts_with("hfence")
        || mnemonic.starts_with("hinval")
        || mnemonic.starts_with("sinval")
        || mnemonic.starts_with("cbo.")
}

fn is_system_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "ecall"
            | "ebreak"
            | "c.ebreak"
            | "mret"
            | "sret"
            | "dret"
            | "mnret"
            | "wfi"
            | "wrs.sto"
            | "wrs.nto"
            | "sctrclr"
            | "sspush.x1"
            | "sspush.x5"
            | "sspopchk.x1"
            | "sspopchk.x5"
            | "ssrdp"
            | "lpad"
            | "mop.r.n"
            | "mop.rr.n"
            | "c.mop.n"
    )
}

fn is_floating_class_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("fclass.")
}

fn is_floating_constant_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("fli.")
}

fn is_floating_convert_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("fcvt") || mnemonic.starts_with("fround")
}

fn is_floating_move_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("fmv") || mnemonic.starts_with("fmvh") || mnemonic.starts_with("fmvp")
}

fn is_floating_compare_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("feq.")
        || mnemonic.starts_with("flt.")
        || mnemonic.starts_with("fle.")
        || mnemonic.starts_with("fltq.")
        || mnemonic.starts_with("fleq.")
}

fn is_floating_arithmetic_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic.split('.').next().unwrap_or(""),
        "fadd"
            | "fsub"
            | "fmul"
            | "fdiv"
            | "fsqrt"
            | "fmadd"
            | "fmsub"
            | "fnmadd"
            | "fnmsub"
            | "fmin"
            | "fmax"
            | "fminm"
            | "fmaxm"
            | "fsgnj"
            | "fsgnjn"
            | "fsgnjx"
    )
}

fn is_multiply_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "mul" | "mulh" | "mulhu" | "mulhsu" | "mulw" | "clmul" | "clmulh" | "clmulr" | "c.mul"
    )
}

fn is_crypto_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("aes")
        || mnemonic.starts_with("sha")
        || mnemonic.starts_with("sm3")
        || mnemonic.starts_with("sm4")
}

fn is_upper_immediate_mnemonic(mnemonic: &str) -> bool {
    matches!(mnemonic, "li" | "lui" | "auipc" | "c.lui" | "c.li")
}

fn is_shift_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "sll"
            | "slli"
            | "sllw"
            | "slliw"
            | "slli.uw"
            | "srl"
            | "srli"
            | "srlw"
            | "srliw"
            | "sra"
            | "srai"
            | "sraw"
            | "sraiw"
            | "c.slli"
            | "c.srli"
            | "c.srai"
    )
}

fn is_compare_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "slt" | "sltu" | "slti" | "sltiu" | "min" | "minu" | "max" | "maxu"
    )
}

fn is_logic_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "and"
            | "andi"
            | "or"
            | "ori"
            | "xor"
            | "xori"
            | "andn"
            | "orn"
            | "c.and"
            | "c.andi"
            | "c.or"
            | "c.xor"
            | "c.not"
    )
}

fn is_bitmanip_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "add.uw"
            | "bclr"
            | "bclri"
            | "bext"
            | "bexti"
            | "binv"
            | "binvi"
            | "brev8"
            | "bset"
            | "bseti"
            | "clz"
            | "clzw"
            | "ctz"
            | "ctzw"
            | "cpop"
            | "cpopw"
            | "orc.b"
            | "pack"
            | "packh"
            | "packw"
            | "rev8"
            | "rol"
            | "rolw"
            | "ror"
            | "rori"
            | "roriw"
            | "rorw"
            | "sext.b"
            | "sext.h"
            | "sh1add"
            | "sh1add.uw"
            | "sh2add"
            | "sh2add.uw"
            | "sh3add"
            | "sh3add.uw"
            | "unzip"
            | "xperm4"
            | "xperm8"
            | "zip"
            | "xnor"
            | "zext.h"
            | "czero.eqz"
            | "czero.nez"
            | "c.sext.b"
            | "c.sext.h"
            | "c.zext.b"
            | "c.zext.h"
            | "c.zext.w"
    )
}

fn is_compressed_stack_mnemonic(mnemonic: &str) -> bool {
    matches!(mnemonic, "cm.push" | "cm.pop" | "cm.mvsa01" | "cm.mva01s")
}

fn is_integer_arithmetic_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "add"
            | "addi"
            | "addw"
            | "addiw"
            | "sub"
            | "subw"
            | "c.add"
            | "c.addi"
            | "c.addi16sp"
            | "c.addi4spn"
            | "c.addiw"
            | "c.addw"
            | "c.mv"
            | "c.nop"
            | "c.sub"
            | "c.subw"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct InstructionSpec {
        name: String,
    }

    #[test]
    fn all_non_vector_non_branch_instructions_have_sdmodel_cluster() {
        let specs: Vec<InstructionSpec> = serde_json::from_str(include_str!(
            "../../riscv-instruction-crates/assets/riscv_instructions.json"
        ))
        .expect("instruction asset should parse");

        let mut missing = specs
            .into_iter()
            .map(|spec| spec.name)
            .filter(|name| {
                !is_vector_mnemonic(name)
                    && !is_branch_jump_mnemonic(name)
                    && classify_mnemonic(name) == InstructionCluster::OtherScalar
            })
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();

        assert!(
            missing.is_empty(),
            "missing SDModel instruction clusters: {missing:?}"
        );
    }
}
