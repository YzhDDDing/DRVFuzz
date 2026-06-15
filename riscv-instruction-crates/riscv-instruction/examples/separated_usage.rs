use riscv_instruction::separated_instructions::*;
use riscv_instruction_types::{
    InstructionSequence, MemConfig, MemoryAccessInstruction, RandomConfig,
    RandomInstructionSequence, RegisterConfig,
};
use serde_json::{from_str as json_from_str, to_string_pretty as json_to_string_pretty};

fn sample_random_config() -> RandomConfig {
    let mem_config = MemConfig::new()
        .with_register_number_range(8, 12)
        .with_immediate_ranges(0, 16);
    let register_config = RegisterConfig::new().with_integer_register_range(13, 20);

    RandomConfig::new()
        .with_mem_config(mem_config)
        .with_register_config(register_config)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = rand::rng();
    let config = sample_random_config();

    // Generate a random instruction sequence
    let random_inst_sequence = RiscvInstruction::random_sequence_with_rng(&mut rng, &config)?;
    println!(
        "Random Separated Instruction Sequence: {}",
        random_inst_sequence
    );

    let serialized = json_to_string_pretty(&random_inst_sequence)?;
    println!("Serialized Sequence JSON:\n{}", serialized);
    let restored: InstructionSequence<RiscvInstruction> = json_from_str(&serialized)?;
    assert_eq!(restored, random_inst_sequence);

    println!(
        "RV32I exposes {} instructions; RV64V exposes {} instructions",
        RV32Extensions::I.instruction_count(),
        RV64Extensions::V.instruction_count()
    );

    // Create registers
    let xd = IntegerRegister::new(5)?;
    let xs1 = IntegerRegister::new(6)?;
    let xs2 = IntegerRegister::new(7)?;

    // Create an immediate (assume a 12-bit immediate type)
    let imm12 = riscv_instruction_types::Immediate::<12>::new(200)?;

    // Build an RV32I instruction (e.g., ADDI)
    let addi_rv32_inst =
        RiscvInstruction::RV32(RV32Instruction::I(RV32IInstructions::ADDI(RV32_I_ADDI {
            imm: imm12,
            xs1,
            xd,
        })));
    println!("Separated ADDI (RV32): {}", addi_rv32_inst); // Output: addi x5, x6, 200

    // Build an RV64M instruction (e.g., MULW)
    let mulw_rv64_inst =
        RiscvInstruction::RV64(RV64Instruction::M(RV64MInstructions::MULW(RV64_M_MULW {
            xs2,
            xs1,
            xd,
        })));
    println!("Separated MULW (RV64): {}", mulw_rv64_inst); // Output: mulw x5, x6, x7

    // Build an instruction with a memory offset and read the offset at runtime.
    let base = riscv_instruction_types::BaseAddressRegister::new(8)?;
    let lw_offset = riscv_instruction_types::Immediate::<12, true>::new(16)?;
    let lw_struct = RV32_I_LW {
        imm: lw_offset,
        xs1: base,
        xd,
    };
    if let Some(offset) = lw_struct.offset_operand_value() {
        println!("LW offset operand value (struct): {}", offset);
    }

    let lw_instruction =
        RiscvInstruction::RV32(RV32Instruction::I(RV32IInstructions::LW(lw_struct.clone())));
    if let Some(offset) = lw_instruction.offset_operand_value() {
        println!("LW offset operand value (enum): {}", offset);
    }

    Ok(())
}
