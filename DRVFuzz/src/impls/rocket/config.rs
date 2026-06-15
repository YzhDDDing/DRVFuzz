use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32, ISABase::Rv64]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![false]
}

pub(crate) fn extensions() -> ExtensionMap {
    ExtensionMap {
        rv32: vec![
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::F,
            RV32Extensions::D,
            RV32Extensions::C,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::Zba,
            RV32Extensions::Zbb,
            RV32Extensions::Zbs,
            RV32Extensions::Zfh,
            RV32Extensions::Zaamo,
            RV32Extensions::Zalrsc,
            RV32Extensions::Zicond,
        ],
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
    // Start at 0x80002000 to avoid .text and .tohost sections
    // Use 256MB range (0x8000_0000 to 0x8FFF_FFFF) which is reasonable for both RV32 and RV64
    // This ensures user memory only contains data, not code or peripherals
    (0x80002000, 0x8FFFFFFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    isa_base: ISABase,
) -> Result<String, ConfigError> {
    let user_insts_string = user_insts.join("\n");
    let (store_instr, tohost_line) = match isa_base {
        ISABase::Rv32 => ("sw", ".word   0"),
        ISABase::Rv64 => ("sd", ".dword  0"),
    };

    Ok(format!(
        r#"    .section .text
    .globl  _start

_start:
    la      t0, trap_handler
    csrw    mtvec, t0

    csrr    t0, mstatus
    li      t1, 0x00003000       # set FS field to Dirty (0b11) so FP regs usable
    or      t0, t0, t1
    csrw    mstatus, t0
    csrw    fcsr, x0             # clear floating-point status

    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 1                # report success
    la      t1, tohost
    {store_instr}      t0, 0(t1)
1:
    j       1b

    .align  2
trap_handler:
    csrr    t0, mepc             # faulting PC
    csrr    t1, mcause           # trap cause
    csrr    t4, mtval

    slli    t5, t1, 1            # strip interrupt bit
    srli    t1, t5, 1

    li      t2, 2                # default length assumes compressed
    li      t3, 2                # illegal instruction -> mtval holds encoding
    beq     t1, t3, use_mtval
    li      t3, 1                # instruction access fault
    beq     t1, t3, update_mepc
    li      t3, 12               # instruction page fault
    beq     t1, t3, update_mepc

    lhu     t4, 0(t0)
    j       decode_length

use_mtval:
    j       decode_length

decode_length:
    andi    t4, t4, 3
    li      t3, 3
    bne     t4, t3, compressed_len
    li      t2, 4                # standard 32-bit instruction
    j       update_mepc

compressed_len:
    li      t2, 2                # compressed instruction

update_mepc:
    add     t0, t0, t2           # skip offending instruction
    csrw    mepc, t0
    csrw    mcause, x0
    csrw    mtval, x0
    mret

    .section .tohost,"aw",@progbits
    .align  6
    .globl  tohost
    .globl  fromhost
tohost:
    {tohost_line}
fromhost:
    {tohost_line}"#,
        store_instr = store_instr,
        tohost_line = tohost_line,
        user_insts_string = user_insts_string,
    ))
}
