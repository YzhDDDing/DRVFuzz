use crate::{error::ConfigError, extension_map::ExtensionMap, isa_base::ISABase};
use riscv_instruction::separated_instructions::{RV32Extensions, RV64Extensions};

pub(crate) fn supported_isa_bases() -> Vec<ISABase> {
    vec![ISABase::Rv32, ISABase::Rv64]
}

pub(crate) fn supported_unaligned_access_modes() -> Vec<bool> {
    vec![false, true]
}

pub(crate) fn extensions() -> ExtensionMap {
    ExtensionMap {
        rv32: vec![
            RV32Extensions::B,
            RV32Extensions::C,
            RV32Extensions::D,
            RV32Extensions::F,
            RV32Extensions::H,
            RV32Extensions::I,
            RV32Extensions::M,
            RV32Extensions::Svinval,
            RV32Extensions::V,
            RV32Extensions::Zaamo,
            RV32Extensions::Zabha,
            RV32Extensions::Zacas,
            RV32Extensions::Zalrsc,
            RV32Extensions::Zawrs,
            RV32Extensions::Zba,
            RV32Extensions::Zbb,
            RV32Extensions::Zbc,
            RV32Extensions::Zbkb,
            RV32Extensions::Zbkx,
            RV32Extensions::Zbs,
            RV32Extensions::Zcb,
            RV32Extensions::Zcd,
            RV32Extensions::Zfbfmin,
            RV32Extensions::Zfh,
            RV32Extensions::Zicbom,
            RV32Extensions::Zicboz,
            RV32Extensions::Zicfilp,
            RV32Extensions::Zicond,
            RV32Extensions::Zicsr,
            RV32Extensions::Zifencei,
            RV32Extensions::Zimop,
            RV32Extensions::Zknd,
            RV32Extensions::Zkne,
            RV32Extensions::Zknh,
            RV32Extensions::Zks,
            RV32Extensions::Zvbb,
            RV32Extensions::Zvbc,
            RV32Extensions::Zvfbfmin,
            RV32Extensions::Zvfbfwma,
            RV32Extensions::Zvkg,
            RV32Extensions::Zvkned,
            RV32Extensions::Zvknha,
            RV32Extensions::Zvks,
        ],
        rv64: vec![
            RV64Extensions::B,
            RV64Extensions::C,
            RV64Extensions::D,
            RV64Extensions::F,
            RV64Extensions::H,
            RV64Extensions::I,
            RV64Extensions::M,
            RV64Extensions::Svinval,
            RV64Extensions::V,
            RV64Extensions::Zaamo,
            RV64Extensions::Zabha,
            RV64Extensions::Zacas,
            RV64Extensions::Zalrsc,
            RV64Extensions::Zawrs,
            RV64Extensions::Zba,
            RV64Extensions::Zbb,
            RV64Extensions::Zbc,
            RV64Extensions::Zbkb,
            RV64Extensions::Zbkx,
            RV64Extensions::Zbs,
            RV64Extensions::Zcb,
            RV64Extensions::Zcd,
            RV64Extensions::Zfbfmin,
            RV64Extensions::Zfh,
            RV64Extensions::Zicbom,
            RV64Extensions::Zicboz,
            RV64Extensions::Zicfilp,
            RV64Extensions::Zicond,
            RV64Extensions::Zicsr,
            RV64Extensions::Zifencei,
            RV64Extensions::Zimop,
            RV64Extensions::Zkn,
            RV64Extensions::Zknd,
            RV64Extensions::Zkne,
            RV64Extensions::Zknh,
            RV64Extensions::Zks,
            RV64Extensions::Zvbb,
            RV64Extensions::Zvbc,
            RV64Extensions::Zvfbfmin,
            RV64Extensions::Zvfbfwma,
            RV64Extensions::Zvkg,
            RV64Extensions::Zvkned,
            RV64Extensions::Zvknha,
            RV64Extensions::Zvks,
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
    let (store_instr, tohost_line, s_mode_block) = match isa_base {
        ISABase::Rv32 => ("sw", ".word   0", "".to_string()),
        ISABase::Rv64 => {
            let block = r#"
.align 2
switch_to_s_mode:
    # Set up the page table: identity-map the 1GB region containing 0x80000000
    la      t0, pgtbl
    li      t1, (0x80000 << 10) | 0xCF # PPN=0x80000, full permissions
    sd      t1, 16(t0)

    # Write satp to enable paging (Sv39)
    li      t2, 8
    slli    t2, t2, 60
    srli    t1, t0, 12
    or      t2, t2, t1
    csrw    satp, t2
    sfence.vma

    # Set MPP to S-mode (1) and prepare for mret
    li      t0, ~(3 << 11)
    csrr    t1, mstatus
    and     t1, t1, t0
    li      t2, (1 << 11)
    or      t1, t1, t2
    csrw    mstatus, t1

    csrw    mepc, ra
    mret

.align 2
exit_s_mode:
    ecall                        # Trigger Environment Call (Cause 9)
    ret                          # Handler will actually adjust mepc to skip this spot
"#
            .to_string();
            ("sd", ".dword  0", block)
        }
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

    # Relax PMP: one NAPOT entry covers the full address space to allow S-mode fetch/load/store
    li      t0, -1
    csrw    pmpaddr0, t0
    li      t0, 0x1f             # R|W|X | A=NAPOT
    csrw    pmpcfg0, t0

    j       user_code

user_code:
{user_insts_string}

exit:
    li      t0, 1                # report success
    la      t1, tohost
    {store_instr}      t0, 0(t1)
1:
    j       1b

{s_mode_block}

    .align  2
trap_handler:
    csrr    t0, mepc             # faulting PC
    csrr    t1, mcause           # trap cause
    csrr    t4, mtval

    # Extract the exception code
    slli    t5, t1, 1
    srli    t1, t5, 1

    # --- Added: handle requests to return to M-mode ---
    li      t3, 9                # Ecall from S-mode
    bne     t1, t3, check_page_fault
    # If it is an ecall, set MPP so mret returns to M-mode
    li      t2, 3 << 11
    csrs    mstatus, t2          # Set MPP to 3 (Machine)
    li      t2, 4                # Skip the ecall instruction
    j       update_mepc

check_page_fault:
    # --- Added: detect page faults (13 or 15) ---
    li      t3, 13
    beq     t1, t3, handle_as_normal_fault
    li      t3, 15
    beq     t1, t3, handle_as_normal_fault
    
    # If it's neither a page fault nor an ecall, fall back to the original logic
    j       original_logic

handle_as_normal_fault:
    # Page faults still need instruction-length decoding and skipping; reuse the original logic
    j       original_logic

original_logic:
    # Original logic: decode instruction length
    li      t2, 2                
    li      t3, 2                
    beq     t1, t3, illegal_len
    li      t3, 1                
    beq     t1, t3, instr_fault_len
    li      t3, 12               
    beq     t1, t3, instr_fault_len

    lhu     t4, 0(t0)
    j       decode_length

other_len:
    lhu     t4, 0(t0)
    j       decode_length

illegal_len:
    beqz    t4, other_len
    j       decode_length
    
instr_fault_len:
    li      t2, 4
    j       update_mepc

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
    .balign 4096
pgtbl:
    .zero 4096
    

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
        s_mode_block = s_mode_block,
    ))
}
