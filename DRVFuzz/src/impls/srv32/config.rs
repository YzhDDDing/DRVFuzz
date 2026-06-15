use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::RV32Extensions;

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![false]
}

pub(crate) fn extensions() -> ExtensionMap {
    // SRV32 ISS supports RV32IMC with CSR and B-extension subsets (Zba/Zbb/Zbc/Zbs)
    ExtensionMap {
        rv32: vec![
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::C,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::Zba,
            RV32Extensions::Zbb,
            RV32Extensions::Zbc,
            RV32Extensions::Zbs,
        ],
        rv64: vec![],
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    r#"OUTPUT_ARCH("riscv")
ENTRY(_start)

MEMORY {
  ram (rwx) : ORIGIN = 0x00000000, LENGTH = 0x00080000
}

SECTIONS
{
  .text ORIGIN(ram) : ALIGN(4)
  {
    *(.text.init)
    *(.text)
    *(.text.*)
  } > ram

  .data : ALIGN(4)
  {
    *(.data)
    *(.data.*)
  } > ram

  .bss (NOLOAD) : ALIGN(4)
  {
    *(.bss)
    *(.bss.*)
    *(COMMON)
  } > ram

  .tohost : ALIGN(4)
  {
    *(.tohost)
  } > ram

  PROVIDE(_stack_pointer = ORIGIN(ram) + LENGTH(ram));
}"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    // Keep user memory away from code/stack; simulator default memory is 512KB
    (0x00001000, 0x0007FFFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    _isa_base: ISABase,
) -> Result<String, ConfigError> {
    let user_insts_string = user_insts.join("\n");
    Ok(format!(
        r#"    .section .text
    .globl  _start

_start:
    la      sp, _stack_pointer
    la      t0, trap_handler
    csrw    mtvec, t0
    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 0
    lui     t1, %hi(0xa000002c)
    addi    t1, t1, %lo(0xa000002c)
    sw      t0, 0(t1)
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
    beq     t1, t3, update_mepc
    li      t3, 12
    beq     t1, t3, update_mepc
    lhu     t4, 0(t0)
    j       decode_length

use_mtval:
    beqz    t4, 1f              # mtval could be zero on illegal csr; fallback to mepc
    j       decode_length
1:
    lhu     t4, 0(t0)
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

    .section .tohost,"aw",@progbits
    .align  2
tohost:
    .word   0
fromhost:
    .word   0

    .section .bss
    .align 4
stack_area:
    .space 1024
_stack_pointer:
"#,
        user_insts_string = user_insts_string,
    ))
}
