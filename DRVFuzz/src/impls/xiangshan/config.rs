use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::RV64Extensions;

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv64]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![true, false]
}

pub(crate) fn extensions() -> ExtensionMap {
    ExtensionMap {
        rv32: Vec::new(),
        rv64: vec![
            RV64Extensions::I,
            RV64Extensions::M,
            RV64Extensions::F,
            RV64Extensions::D,
            RV64Extensions::C,
            RV64Extensions::Zicsr,
            RV64Extensions::Zifencei,
            RV64Extensions::Zba,
            RV64Extensions::Zbb,
            RV64Extensions::Zbs,
            RV64Extensions::Zfh,
            RV64Extensions::H,
            RV64Extensions::Zaamo,
            RV64Extensions::Zalrsc,
            RV64Extensions::Zicond,
        ],
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    r#"OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
  . = 0x80000000;

  .text.init : {
    KEEP(*(.text.init))
    *(.text)
    *(.text.*)
  }

  . = ALIGN(0x1000);
  .tohost : {
    KEEP(*(.tohost))
  }

  .data : {
    *(.data)
    *(.data.*)
  }

  .bss (NOLOAD) : ALIGN(16) {
    *(.bss)
    *(.bss.*)
    *(COMMON)
  }

  PROVIDE(_end = .);
  PROVIDE(__stack = stack_top);
}"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    (0x8000_2000, 0x8FFF_FFFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    isa_base: ISABase,
) -> Result<String, ConfigError> {
    if isa_base != ISABase::Rv64 {
        return Err(ConfigError::UnsupportedIsaBase {
            impl_name: "XiangShan".to_string(),
            isa_base: format!("{isa_base:?}"),
        });
    }

    let user_insts_string = if user_insts.is_empty() {
        "    nop".to_string()
    } else {
        user_insts.join("\n")
    };

    Ok(format!(
        r#"    .section .text
    .globl  _start

_start:
    la      t0, trap_handler
    csrw    mtvec, t0

    csrr    t0, mstatus
    li      t1, 0x00003000       # enable FPU (FS = Dirty)
    or      t0, t0, t1
    csrw    mstatus, t0
    csrw    fcsr, x0

    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 1
    li      t0, 1
    li      t0, 1
    li      t0, 1
    li      t0, 1
    li      t0, 1
    li      t0, 1
    la      t1, skiptrap_store_buf
    sd      t0, 0(t1)

    # extra padding to keep commit trace alive
    li      t0, 0                # DiffTest STATE_GOODTRAP
    li      t0, 0
    li      t0, 0
    .insn   i 0x6b, 0, x0, t0, 0
exit_spin:
1:
    j       1b

    .align  2
trap_handler:
    csrr    t0, mepc
    csrr    t1, mcause
    csrr    t4, mtval

    slli    t5, t1, 1
    srli    t1, t5, 1

    li      t2, 2
    li      t3, 2
    beq     t1, t3, use_mtval
    li      t3, 1
    beq     t1, t3, fetch_inst
    li      t3, 12
    beq     t1, t3, fetch_inst

fetch_inst:
    lhu     t4, 0(t0)
    j       decode_length

use_mtval:
    j       decode_length

decode_length:
    andi    t4, t4, 3
    li      t3, 3
    bne     t4, t3, compressed_len
    li      t2, 4
    j       update_mepc

compressed_len:
    li      t2, 2

update_mepc:
    add     t0, t0, t2
    csrw    mepc, t0
    csrw    mcause, x0
    csrw    mtval, x0
    mret

    .section .data
    .align  3
skiptrap_store_buf:
    .dword  0
"#,
        user_insts_string = indent_user_code(&user_insts_string),
    ))
}

fn indent_user_code(code: &str) -> String {
    code.lines()
        .map(|line| {
            if line.trim().is_empty() {
                "    ".to_string()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
