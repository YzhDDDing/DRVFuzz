use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::RV32Extensions;

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![false]
}

pub(crate) fn extensions() -> ExtensionMap {
    // Kronos: RV32I + Zicsr + Zifencei
    ExtensionMap {
        rv32: vec![
            RV32Extensions::I,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
        ],
        rv64: Vec::new(),
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    // Kronos verilator top uses unified SRAM from 0x0000_0000; keep small footprint
    // Place .tohost at 0x00000100 for runner '--tohost 0x100'
    r#"/* Minimal linker script for unified IMEM/DMEM in Verilator top
   Memory size matches kronos_compliance_top generic_spram default (8KB) */
OUTPUT_ARCH("riscv")
ENTRY(_start)

MEMORY {
  ram (rwx) : ORIGIN = 0x00000000, LENGTH = 0x00002000
}

SECTIONS
{
  .text ORIGIN(ram) : ALIGN(4)
  {
    *(.text.init)
    *(.init)
    *(.text*)
    *(.rodata*)
  } > ram

  .data : ALIGN(4)
  {
    PROVIDE(_sdata = .);
    *(.data*)
    . = ALIGN(4);
    PROVIDE(_edata = .);
  } > ram

  .tohost : ALIGN(64)
  {
    *(.tohost)
  } > ram

  .bss (NOLOAD) : ALIGN(4)
  {
    PROVIDE(_sbss = .);
    *(.bss*)
    *(COMMON)
    . = ALIGN(4);
    PROVIDE(_ebss = .);
  } > ram

  /* Stack Pointer at end of RAM */
  PROVIDE(_stack_pointer = ORIGIN(ram) + LENGTH(ram));
}"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    // Pick a safe RAM window away from text start; closed interval
    (0x00000100, 0x00001FFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    _isa_base: ISABase,
) -> Result<String, ConfigError> {
    let user_insts_string = user_insts.join("\n");
    Ok(format!(
        r#"    .section .text
    .globl  _start

/* Kronos skip-on-trap skeleton (no F ext):
 * - mtvec -> trap_handler
 * - executes user_code then write PASS (1) to .tohost (0x100) and spin
 * - trap handler increments mepc by 2/4 based on instruction length
 */
_start:
    la      sp, _stack_pointer
    la      t0, trap_handler
    csrw    mtvec, t0
    csrr    t0, mstatus
    csrw    mstatus, t0
    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 1
    la      t1, tohost
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
    .align 4
amo_loc:
    .word 0x12345678

    .section .bss
    .align 4
mem_buf:
    .space 16

    .section .tohost,"aw",@progbits
    .align  2
    .globl  tohost
    .globl  fromhost
tohost:
    .word 0
fromhost:
    .word 0
"#
    ))
}
