use riscv_instruction::merged_instructions::*;
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
        "Random Merged Instruction Sequence: {}",
        random_inst_sequence
    );

    let serialized = json_to_string_pretty(&random_inst_sequence)?;
    println!("Serialized Sequence JSON:\n{}", serialized);
    let restored: InstructionSequence<RiscvInstruction> = json_from_str(&serialized)?;
    assert_eq!(restored, random_inst_sequence);

    println!(
        "RV32M exposes {} instructions; RV64V exposes {} instructions",
        RV32Extensions::M.instruction_count(),
        RV64Extensions::V.instruction_count()
    );

    // Create registers
    let xd = IntegerRegister::new(1)?;
    let xs1 = IntegerRegister::new(2)?;
    let xs2 = IntegerRegister::new(3)?;

    // Build a shared instruction (e.g., ADD)
    let add_inst = RiscvInstruction::Shared(SharedInstruction::I(ISharedInstructions::ADD(
        I_Shared_ADD { xs2, xs1, xd },
    )));
    println!("Merged ADD: {}", add_inst); // Output: add x1, x2, x3

    // Build an RV64-specific instruction (e.g., ADDW)
    let addw_inst =
        RiscvInstruction::Specific(SpecificInstruction::RV64(RV64SpecificInstruction::I(
            RV64ISpecificInstructions::ADDW(RV64_I_ADDW { xs2, xs1, xd }),
        )));
    println!("Merged ADDW (RV64): {}", addw_inst); // Output: addw x1, x2, x3

    // Build a shared LW instruction and read the runtime offset.
    let base = riscv_instruction_types::BaseAddressRegister::new(8)?;
    let lw_offset = riscv_instruction_types::Immediate::<12, true>::new(32)?;
    let lw = I_Shared_LW {
        imm: lw_offset,
        xs1: base,
        xd,
    };
    if let Some(offset) = lw.offset_operand_value() {
        println!("LW offset operand value (struct): {}", offset);
    }

    let lw_instruction =
        RiscvInstruction::Shared(SharedInstruction::I(ISharedInstructions::LW(lw.clone())));
    if let Some(offset) = lw_instruction.offset_operand_value() {
        println!("LW offset operand value (enum): {}", offset);
    }

    Ok(())
}
