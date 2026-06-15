use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::RV32Extensions;

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![false]
}

pub(crate) fn extensions() -> ExtensionMap {
    // The VexRiscv emulator is currently configured as RV32 with I M A F D C + Zicsr + Zifencei enabled by default.
    // Whether D is enabled is controlled by the outer extension_override, allowing the emulator binary to switch between RV32F-only
    // and RV32D.
    ExtensionMap {
        rv32: vec![
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::F,
            // RV32Extensions::D,
            RV32Extensions::C,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::Zaamo,
            RV32Extensions::Zalrsc,
        ],
        rv64: Vec::new(),
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    // VexRiscv simulation memory layout (matching the upstream VexRiscv harness):
    // - .text at 0x8000_0000
    // - .tohost at 0xF00F_FF20 (writing 0 is treated as PASS)
    r#"
OUTPUT_ARCH("riscv")
ENTRY(_start)

MEMORY
{
  onChipRam (W!RX) : ORIGIN = 0x80000000, LENGTH = 128K
}

SECTIONS
{
  .text : {
    . = ALIGN(4);
    KEEP(*(.text.start))
    *(.text*)
    *(.rodata*)
  } > onChipRam

  .data : {
    . = ALIGN(4);
    *(.data*)
  } > onChipRam

  .bss (NOLOAD) : {
    . = ALIGN(4);
    __bss_start = .;
    *(.bss*)
    *(COMMON)
    __bss_end = .;
  } > onChipRam

  /* Place the tohost region at VexRiscv harness MMIO pass/fail addresses */
  . = 0xF00FFF20;
  .tohost 0xF00FFF20 : {
    KEEP(*(.tohost))
  }
}
"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    // Choose a user RAM window that avoids the start of .text; range is inclusive [start, end]
    (0x80000100, 0x8001FFFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    _isa_base: ISABase,
) -> Result<String, ConfigError> {
    let user_insts_string = user_insts.join("\n");
    Ok(format!(
        r#"    .section .text
    .globl  _start

/* VexRiscv skip-on-trap skeleton:
 * - mtvec -> trap_handler
 * - mstatus.FS=Dirty, fcsr=0
 * - executes user_code then write PASS (0) to .tohost (0xF00FFF20) and spin
 * - trap handler increments mepc by 2/4 based on instruction length
 */
.section .text.start
_start:
    la      t0, trap_handler
    csrw    mtvec, t0
    csrr    t0, mstatus
    li      t1, 0x00003000
    or      t0, t0, t1
    csrw    mstatus, t0
    csrw    fcsr, x0
    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    li      t0, 0
    la      t1, tohost
    sw      t0, 0(t1)
1:
    j       1b

.align  2
trap_handler:
    csrr    t0, mepc
    csrr    t1, mcause
    csrr    t4, mtval

    /* strip interrupt bit */
    slli    t5, t1, 1
    srli    t1, t5, 1

    li      t2, 2
    li      t3, 2
    beq     t1, t3, decode_length
    li      t3, 1
    beq     t1, t3, update_mepc
    li      t3, 12
    beq     t1, t3, update_mepc
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

.section .data
.align 4
amo_loc:
    .word 0x12345678

.section .bss
.align 4
mem_buf:
    .space 16

/* PASS/FAIL region */
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
