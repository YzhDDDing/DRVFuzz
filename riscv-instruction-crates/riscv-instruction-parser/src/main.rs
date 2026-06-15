use riscv_instruction_parser::{
    instruction_fixer::fix_instructions, parser::parse_insts_from_riscv_unified_db,
    report_generator::generate_detailed_extension_report, types::save_instructions_to_json,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut instructions = parse_insts_from_riscv_unified_db("assets/riscv-unified-db")?;
    println!("Total instructions: {}", instructions.len());

    fix_instructions(&mut instructions);
    println!("Fix completed!");

    save_instructions_to_json(&instructions, "assets/riscv_instructions.json")?;
    println!("Saved as riscv_instructions.json");

    generate_detailed_extension_report(
        &instructions,
        "assets/riscv_detailed_extension_report.md",
    )?;

    println!(
        "Generated detailed extension report riscv_detailed_extension_report.md",
    );

    Ok(())
}
