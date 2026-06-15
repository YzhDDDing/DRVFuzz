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
            // Base ISA
            RV32Extensions::I, // Base integer ISA
            RV32Extensions::M, // Multiply/divide
            RV32Extensions::F, // Single-precision floating point
            RV32Extensions::C, // Compressed instructions
            // Atomics (A extension split into two parts)
            RV32Extensions::Zaamo, // Atomic memory operations (CVA6ConfigAExtEn=1)
            RV32Extensions::Zalrsc, // Load-Reserved/Store-Conditional (CVA6ConfigAExtEn=1)
            // Bit-manipulation (B extension)
            RV32Extensions::B,   // Top-level bit-manipulation (CVA6ConfigBExtEn=1)
            RV32Extensions::Zba, // Address-generation bit operations
            RV32Extensions::Zbb, // Basic bit operations
            RV32Extensions::Zbc, // Carry-related bit operations
            RV32Extensions::Zbs, // Single-bit operations
            // Compressed-related
            RV32Extensions::Zcb, // Base compressed extension (CVA6ConfigZcbExtEn=1)
            RV32Extensions::Zcf, // Compressed single-precision FP (C + F)
            // RV32Extensions::Zcmp, // Compressed pointer push/pop (CVA6ConfigZcmpExtEn=1)
            // Cryptography extensions (ZKN=1)
            RV32Extensions::Zbkb, // Bitmanip crypto basics
            RV32Extensions::Zbkx, // Bitmanip crypto cross-operations
            RV32Extensions::Zknd, // NIST AES decrypt
            RV32Extensions::Zkne, // NIST AES encrypt
            RV32Extensions::Zknh, // NIST SHA hash
            // Standard extensions
            RV32Extensions::Zicond, // Conditional operations (CVA6ConfigRVZiCond=1)
            RV32Extensions::Zicsr,  // Control/status registers
            RV32Extensions::Zifencei, // Instruction fence
        ],
        rv64: vec![
            // Base ISA
            RV64Extensions::I, // Base integer ISA
            RV64Extensions::M, // Multiply/divide
            RV64Extensions::F, // Single-precision floating point (CVA6ConfigRVF=1)
            RV64Extensions::D, // Double-precision floating point (CVA6ConfigRVD=1) - RV64 only
            RV64Extensions::C, // Compressed instructions
            RV64Extensions::H, // Hypervisor extension (CVA6ConfigHExtEn=1) - RV64 only
            // Atomics (A extension split)
            RV64Extensions::Zaamo, // Atomic memory operations (CVA6ConfigAExtEn=1)
            RV64Extensions::Zalrsc, // Load-Reserved/Store-Conditional (CVA6ConfigAExtEn=1)
            // Bit-manipulation (B extension)
            RV64Extensions::B,   // Top-level bit-manipulation (CVA6ConfigBExtEn=1)
            RV64Extensions::Zba, // Address-generation bit operations
            RV64Extensions::Zbb, // Basic bit operations
            RV64Extensions::Zbc, // Carry-related bit operations
            RV64Extensions::Zbs, // Single-bit operations
            // Compressed-related
            RV64Extensions::Zcb, // Base compressed extension (CVA6ConfigZcbExtEn=1)
            RV64Extensions::Zcd, // Compressed double-precision FP (C + D) - RV64 only
            // RV64Extensions::Zcmp, // Compressed pointer push/pop (CVA6ConfigZcmpExtEn=1)
            // Half-precision floating point
            RV64Extensions::Zfh, // Half-precision floating point (CVA6ConfigF16En=1)
            // Cryptography extensions (ZKN=1)
            RV64Extensions::Zkn,  // NIST crypto umbrella
            RV64Extensions::Zbkb, // Bitmanip crypto basics
            RV64Extensions::Zbkx, // Bitmanip crypto cross-operations
            RV64Extensions::Zknd, // NIST AES decrypt
            RV64Extensions::Zkne, // NIST AES encrypt
            RV64Extensions::Zknh, // NIST SHA hash
            // Standard extensions
            RV64Extensions::Zicond, // Conditional operations (CVA6ConfigRVZiCond=1)
            RV64Extensions::Zicsr,  // Control/status registers
            RV64Extensions::Zifencei, // Instruction fence
        ],
    }
}

pub(crate) fn linker_script_content() -> &'static str {
    r#"OUTPUT_ARCH("riscv")
ENTRY(_start)

SECTIONS {
  . = 0x80000000;
  .text : { *(.text*) }

  . = ALIGN(0x1000);
  .tohost : { *(.tohost) }

  . = ALIGN(0x1000);
  .data : { *(.data*) }
  .bss  : { *(.bss*) *(COMMON) }
}"#
}

pub(crate) fn user_mem_range() -> (u64, u64) {
    // Start at 0x80002000 to avoid .text and .tohost sections
    // .tohost is at 0x80001000, so we skip to next 4KB aligned boundary
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
        "    .section .text
    .globl   _start

_start:
    la       t0, trap_handler
    csrw     mtvec, t0

    csrr     t0, mstatus
    li       t1, 0x00003000         # enable FPU (FS = 0b11)
    or       t0, t0, t1
    csrw     mstatus, t0
    csrw     fcsr, x0               # clear floating-point status

    j        user_code

user_code:
{user_insts_string}

exit:
    li       t0, 1                  # always report success (exit code 0)
    la       t1, tohost
    {store_instr}       t0, 0(t1)
1:
    j        1b

    .align   2
trap_handler:
    csrr     t0, mepc               # offending PC
    csrr     t1, mcause             # trap cause
    slli     t5, t1, 1              # clear interrupt bit -> synchronous cause only
    srli     t1, t5, 1

    li       t2, 2                  # default length (compressed)
    li       t3, 2                  # illegal instruction -> mtval holds encoding
    beq      t1, t3, use_mtval
    li       t3, 1                  # instruction access fault
    beq      t1, t3, update_mepc
    li       t3, 12                 # instruction page fault
    beq      t1, t3, update_mepc

    lhu      t4, 0(t0)              # fetch lower half-word of instruction
    j        decode_length

use_mtval:
    csrr     t4, mtval              # mtval contains the faulting instruction

decode_length:
    andi     t4, t4, 3
    li       t3, 3
    bne      t4, t3, compressed_len
    li       t2, 4                  # standard 32-bit instruction
    j        update_mepc

compressed_len:
    li       t2, 2                  # compressed instruction

update_mepc:
    add      t0, t0, t2             # skip offending instruction
    csrw     mepc, t0
    csrw     mcause, x0
    csrw     mtval, x0
    csrw     mip, x0
    mret

    .section .tohost,\"aw\",@progbits
    .align   6
    .globl   tohost
    .globl   fromhost
tohost:
    {tohost_line}
fromhost:
    {tohost_line}
",
        store_instr = store_instr,
        tohost_line = tohost_line,
        user_insts_string = user_insts_string,
    ))
}
