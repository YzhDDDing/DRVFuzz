use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::RV32Extensions;

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![true]
}

pub(crate) fn extensions() -> ExtensionMap {
    // Typical Ibex configuration: RV32I + M + C + Zicsr + Zifencei (with optional B subset)
    ExtensionMap {
        rv32: vec![
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::C,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::B,
            RV32Extensions::Zbc,
        ],
        rv64: Vec::new(),
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    // Ibex Simple System memory layout (minimal script aligned with examples/sw/simple_system):
    // - Reset PC: 0x0010_0080
    // - Trap vector base: 0x0010_0000 (.vectors)
    // - tohost: 0x0002_0008 (write 1 to end simulation)
    r#"/* Copyright lowRISC contributors.
   Licensed under the Apache License, Version 2.0, see LICENSE for details.
   SPDX-License-Identifier: Apache-2.0 */

OUTPUT_ARCH(riscv)

MEMORY
{
/* Change this if you'd like different sizes. Arty A7-100(35) has a maximum of 607.5KB(225KB)
   BRAM space. Configuration below is for maximum BRAM capacity with Arty A7-35 while letting
   CoreMark run (.vmem of 152.8KB).
*/
    ram         : ORIGIN = 0x00100000, LENGTH = 0x30000 /* 192 kB */
    stack       : ORIGIN = 0x00130000, LENGTH = 0x8000  /* 32 kB */
}

/* Stack information variables */
_min_stack      = 0x2000;   /* 8K - minimum stack space to reserve */
_stack_len     = LENGTH(stack);
_stack_start   = ORIGIN(stack) + LENGTH(stack);

_entry_point = _vectors_start + 0x80;
ENTRY(_entry_point)

/* The tohost address is used by Spike for a magic "stop me now" message. This
   is set to equal SIM_CTRL_CTRL (see simple_system_regs.h), which has that
   effect in simple_system simulations. Note that it must be 8-byte aligned.

   We don't read data back from Spike, so fromhost is set to some dummy value:
   we place it just above the top of the stack.
 */
tohost   = 0x20008;
fromhost = _stack_start + 0x10;

SECTIONS
{
    .vectors :
    {
        . = ALIGN(4);
		_vectors_start = .;
        KEEP(*(.vectors))
		_vectors_end = .;
    } > ram

    .text : {
        . = ALIGN(4);
        *(.text)
        *(.text.*)
    }  > ram

    .rodata : {
        . = ALIGN(4);
        /* Small RO data before large RO data */
        *(.srodata)
        *(.srodata.*)
        *(.rodata);
        *(.rodata.*)
    } > ram

    .data : {
        . = ALIGN(4);
        /* Small data before large data */
        *(.sdata)
        *(.sdata.*)
        *(.data);
        *(.data.*)
    } > ram

    .bss :
    {
        . = ALIGN(4);
        _bss_start = .;
        /* Small BSS before large BSS */
        *(.sbss)
        *(.sbss.*)
        *(.bss)
        *(.bss.*)
        *(COMMON)
        _bss_end = .;
    } > ram

    /* ensure there is enough room for stack */
    .stack (NOLOAD): {
        . = ALIGN(4);
        . = . + _min_stack ;
        . = ALIGN(4);
        stack = . ;
        _stack = . ;
    } > stack
}
"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    // Choose a user memory range that avoids .text (inclusive range semantics per caller).
    // Start at 0x00100200 and extend to the end of RAM.
    (0x00100200, 0x001FFFFF)
}

pub(crate) fn build_asm_content(
    user_insts: &[String],
    _isa_base: ISABase,
) -> Result<String, ConfigError> {
    let user_insts_string = user_insts.join("\n");
    Ok(format!(
        r#"    .section .vectors,"ax",@progbits
    .align  4
    /* Simple System resets mtvec to the start of .vectors (0x0010_0000).
     * Place a trampoline here that jumps to the real trap handler in .text.
     * Pad to 0x80 bytes so .text starts at 0x0010_0080 as expected.
     */
    j       trap_handler
    .balign 128, 0

    .section .text
    .globl  _start

/* Minimal skip-on-trap test for RV32IMC (Ibex).
 * - Installs an mtvec trap handler that advances mepc by 2 or 4 bytes
 *   depending on the trapped instruction length (compressed or not).
 * - Executes a compressed breakpoint and a 32-bit illegal instruction
 *   to exercise both paths, then reports pass via tohost (0x20008) and spins.
 */
_start:
    /* mtvec already points to .vectors (0x0010_0000) in Simple System. */
    la      t0, trap_handler
    csrw    mtvec, t0
    /* Allow FP regs (if present): set mstatus.FS=Dirty and clear fcsr. */
    csrr    t0, mstatus
    li      t1, 0x00003000
    or      t0, t0, t1
    csrw    mstatus, t0
    csrw    fcsr, x0
    j       user_code

user_code:
{user_insts_string}

exit:
    /* Report success (1) via HTIF tohost then spin */
    li      t0, 1
    la      t1, tohost
    sw      t0, 0(t1)
1:
    j       1b

    .align  2
trap_handler:
    csrr    t0, mepc             /* faulting PC */
    csrr    t1, mcause           /* trap cause */
    csrr    t4, mtval

    /* If it's an interrupt (mcause[31]=1), guard against returning into the
     * second half of a 32-bit instruction (would cause endless re-trap).
     * If so, advance mepc by 2 to realign to the next instruction boundary.
     */
    bltz    t1, intr_path

    /* strip interrupt bit */
    slli    t5, t1, 1
    srli    t1, t5, 1

    /* default length assumes compressed (2 bytes) */
    li      t2, 2

    /* For these causes, mtval may hold the instruction encoding */
    li      t3, 2                /* illegal instruction */
    beq     t1, t3, use_mtval
    li      t3, 1                /* instruction access fault */
    beq     t1, t3, update_mepc
    li      t3, 12               /* instruction page fault */
    beq     t1, t3, update_mepc

    /* Otherwise, peek halfword at mepc to determine length */
    lhu     t4, 0(t0)
    j       decode_length

use_mtval:
    /* t4 already holds mtval */
    j       decode_length

decode_length:
    andi    t4, t4, 3
    li      t3, 3
    bne     t4, t3, compressed_len
    li      t2, 4                /* standard 32-bit instruction */
    j       update_mepc

compressed_len:
    li      t2, 2                /* compressed instruction */

update_mepc:
    add     t0, t0, t2           /* skip offending instruction */
    csrw    mepc, t0
    csrw    mcause, x0
    csrw    mtval, x0
    mret

/* Interrupt path: normally do not skip. But if mepc points at the second
 * halfword of a 32-bit instruction, bump by 2 to avoid livelock.
 */
intr_path:
    lhu     t4, 0(t0)            /* halfword at mepc */
    andi    t4, t4, 3
    li      t3, 3
    beq     t4, t3, intr_return  /* already at start of 32b insn */
    addi    t5, t0, -2
    lhu     t4, 0(t5)            /* check previous halfword */
    andi    t4, t4, 3
    li      t3, 3
    bne     t4, t3, intr_return  /* not in middle of a 32b insn */
    addi    t0, t0, 2            /* was middle of 32b insn -> step to end */
    csrw    mepc, t0
intr_return:
    csrw    mcause, x0
    csrw    mtval, x0
    mret
    /* No explicit tohost/fromhost symbols here; provided by linker script. */

    .section .data
    .align 4
amo_loc:
    .word 0x12345678

    .section .bss
    .align 4
mem_buf:
    .space 16
"#
    ))
}
