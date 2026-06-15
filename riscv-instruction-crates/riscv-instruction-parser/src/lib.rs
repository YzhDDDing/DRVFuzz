pub mod instruction_fixer;
pub mod operand_extractor;
pub mod parser;
pub mod report_generator;
pub mod types;

#[cfg(test)]
mod tests {
    use super::types::{AssemblySyntax, Instruction, MemoryAddressInfo}; // Ensure new type is imported
    use regex::Regex;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn generate_new_json_with_memory_access_info() -> Result<(), Box<dyn std::error::Error>> {
        // --- File loading section (same as before) ---
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let candidates = [
            "assets/riscv_instructions.json",
            "../assets/riscv_instructions.json",
            "../../assets/riscv_instructions.json",
        ];
        let mut found_path = None;
        for rel in candidates.iter() {
            let p = Path::new(manifest_dir).join(rel);
            if p.exists() {
                found_path = Some(p);
                break;
            }
        }
        let path = found_path.ok_or_else(|| {
            let tried = candidates
                .iter()
                .map(|r| format!("{}", Path::new(manifest_dir).join(r).display()))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("Failed to find riscv_instructions.json; tried: {}", tried)
        })?;
        let data = std::fs::read_to_string(&path)?;
        let instructions: Vec<Instruction> = serde_json::from_str(&data)?;
        let total_instructions = instructions.len();

        // --- Filtering and extraction logic ---

        // More robust regex with named capture groups
        // (?P<name>...) defines a capture group called 'name'
        let addr_re = Regex::new(
            r"(?x) # Enable free-spacing mode for comments and spaces
            # Optional offset part
            (?P<offset>\{[^}]+\})?
            \s*
            # Parentheses and base register part
            \(
            \s*
            (?P<base>sp|\{[^}]+\})
            \s*
            \)
            ",
        )?;

        println!("\nMemory-access instructions and their address components (new logic):");
        println!(
            "{:<22} | {:<15} | {:<15}",
            "Instruction", "Base register", "Offset operand"
        );
        println!("{:-<22}-+-{:-<15}-+-{:-<15}", "", "", "");

        let mut mem_instruction_count = 0;
        let mut new_instructions = Vec::with_capacity(instructions.len());

        for mut inst in instructions {
            let syntax_str = match &inst.assembly_syntax {
                AssemblySyntax::Format(s) => s.as_str(),
                AssemblySyntax::RustCode(s) => s.as_str(),
            };

            let mut mem_info: Option<MemoryAddressInfo> = None;

            // 1. Special cases
            if inst.name == "lpad" {
                mem_info = Some(MemoryAddressInfo {
                    base_register_fixed: Some("PC".to_string()),
                    base_register_operand: None,
                    offset_operand: Some("uimm".to_string()),
                });
            } else if inst.name == "c.addi4spn" || inst.name == "c.addi16sp" {
                // These compute addresses even if not pure memory accesses; handle them too
                mem_info = Some(MemoryAddressInfo {
                    base_register_fixed: Some("sp".to_string()),
                    base_register_operand: None,
                    offset_operand: inst
                        .operands
                        .iter()
                        .find(|o| o.name == "uimm" || o.name == "imm")
                        .map(|o| o.name.clone()),
                });
            } else if let Some(caps) = addr_re.captures(syntax_str) {
                // 2. General pattern match
                let mut base_op = None;
                let mut base_fix = None;

                if let Some(base_match) = caps.name("base") {
                    let base_str = base_match.as_str();
                    if base_str == "sp" {
                        base_fix = Some("sp".to_string());
                    } else if base_str.starts_with('{') && base_str.ends_with('}') {
                        base_op = Some(base_str[1..base_str.len() - 1].to_string());
                    }
                }

                // Only treat as memory access if a base is successfully parsed
                if base_op.is_some() || base_fix.is_some() {
                    let offset_op = caps
                        .name("offset")
                        .map(|m| m.as_str())
                        .map(|s| s[1..s.len() - 1].to_string()); // Remove braces

                    mem_info = Some(MemoryAddressInfo {
                        base_register_operand: base_op,
                        base_register_fixed: base_fix,
                        offset_operand: offset_op,
                    });
                }
            }

            if let Some(info) = mem_info {
                mem_instruction_count += 1;

                let base_disp = info
                    .base_register_operand
                    .as_deref()
                    .or(info.base_register_fixed.as_deref())
                    .unwrap_or("?");

                let off_disp = info.offset_operand.as_deref().unwrap_or("N/A (zero)");

                println!("{:<22} | {:<15} | {:<15}", inst.name, base_disp, off_disp);

                inst.memory_access = Some(info);
            }

            new_instructions.push(inst);
        }

        println!("\n--- Summary ---");
        println!(
            "Read {} instructions from {}; found {} memory-access instructions.",
            path.display(),
            total_instructions,
            mem_instruction_count
        );

        // Write updated instruction set with memory info back to a new file in the same dir
        let out_path = path.with_file_name("riscv_instructions_new.json");
        let json = serde_json::to_string_pretty(&new_instructions)?;
        std::fs::write(&out_path, json)?;
        println!("Generated new file: {}", out_path.display());

        // Assertions
        assert_eq!(mem_instruction_count, 428, "Filtered instruction count differs from expected");

        let lw_inst = new_instructions.iter().find(|i| i.name == "lw").unwrap();
        assert_eq!(
            lw_inst
                .memory_access
                .as_ref()
                .unwrap()
                .base_register_operand,
            Some("xs1".to_string())
        );
        assert_eq!(
            lw_inst.memory_access.as_ref().unwrap().offset_operand,
            Some("imm".to_string())
        );

        let c_lwsp_inst = new_instructions
            .iter()
            .find(|i| i.name == "c.lwsp")
            .unwrap();
        assert_eq!(
            c_lwsp_inst
                .memory_access
                .as_ref()
                .unwrap()
                .base_register_fixed,
            Some("sp".to_string())
        );
        assert_eq!(
            c_lwsp_inst.memory_access.as_ref().unwrap().offset_operand,
            Some("uimm".to_string())
        );

        let amoswap_w_inst = new_instructions
            .iter()
            .find(|i| i.name == "amoswap.w")
            .unwrap();
        assert_eq!(
            amoswap_w_inst
                .memory_access
                .as_ref()
                .unwrap()
                .base_register_operand,
            Some("xs1".to_string())
        );
        assert_eq!(
            amoswap_w_inst
                .memory_access
                .as_ref()
                .unwrap()
                .offset_operand,
            None
        );

        Ok(())
    }
}
