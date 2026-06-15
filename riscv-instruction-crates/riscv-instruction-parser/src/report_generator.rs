use crate::types::{
    AssemblySyntax, ISABase, ISAExtension, Instruction, Operand, OperandRestriction, OperandType,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

pub fn generate_detailed_extension_report(
    instructions: &[Instruction],
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut report = String::new();
    report.push_str("# RISC-V Instruction Report by Extension\n\n");

    // Current time (simple version)
    report.push_str(&format!(
        "**Report generated at**: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));

    // Collect instructions and group by extension
    let mut extension_groups: BTreeMap<ISAExtension, Vec<&Instruction>> = BTreeMap::new();

    for inst in instructions.iter() {
        extension_groups
            .entry(inst.extension)
            .or_insert_with(Vec::new)
            .push(inst);
    }

    // Overview statistics table
    report.push_str("## 📊 Overview Statistics\n\n");
    report.push_str("| Extension | Standard Instrs | Compressed Instrs | Total | Description |\n");
    report.push_str("|------|-------------|-------------|------|------|\n");

    for (extension, instructions_in_ext) in &extension_groups {
        let standard_count = instructions_in_ext
            .iter()
            .filter(|inst| !inst.name.starts_with("c."))
            .count();
        let compressed_count = instructions_in_ext
            .iter()
            .filter(|inst| inst.name.starts_with("c."))
            .count();
        let total_count = instructions_in_ext.len();
        let description = get_extension_description(*extension);

        report.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            extension, standard_count, compressed_count, total_count, description
        ));
    }

    report.push_str("\n");

    // Generate a detailed table for each extension
    for (extension, instructions_in_ext) in &extension_groups {
        report.push_str(&format!("## 🔧 {} Extension Instructions\n\n", extension));
        report.push_str(&format!(
            "**Extension description**: {}\n\n",
            get_extension_description(*extension)
        ));
        report.push_str(&format!(
            "**Total instructions**: {}\n\n",
            instructions_in_ext.len()
        ));

        // Group by instruction type
        let mut standard_instructions: Vec<&Instruction> = Vec::new();
        let mut compressed_instructions: Vec<&Instruction> = Vec::new();

        for inst in instructions_in_ext {
            if inst.name.starts_with("c.") {
                compressed_instructions.push(inst);
            } else {
                standard_instructions.push(inst);
            }
        }

        // Standard instruction table
        if !standard_instructions.is_empty() {
            report.push_str("### 📝 Standard Instructions\n\n");
            generate_instruction_table(&mut report, &standard_instructions);
            report.push_str("\n");
        }

        // Compressed instruction table
        if !compressed_instructions.is_empty() {
            report.push_str("### 📦 Compressed Instructions\n\n");
            generate_instruction_table(&mut report, &compressed_instructions);
            report.push_str("\n");
        }

        report.push_str("---\n\n");
    }

    // ISA compatibility report
    report.push_str("## 🏗️ ISA Compatibility\n\n");
    generate_isa_compatibility_report(&mut report, instructions);

    // Operand analysis report
    report.push_str("## 📋 Operand Usage Statistics\n\n");
    generate_operand_usage_report(&mut report, instructions);

    // Instruction counts grouped by operand count
    report.push_str("## 📏 Instruction Counts by Operand Number\n\n");
    generate_operand_count_statistics(&mut report, instructions);

    fs::write(output_path, report)?;

    Ok(())
}

fn generate_instruction_table(report: &mut String, instructions: &[&Instruction]) {
    report.push_str("| Instruction | ISA Support | Operand Count | Assembly | Operands | Operand Lengths (RV32/RV64) | Operand Constraints |\n");
    report.push_str("|-------------|-------------|---------------|----------|----------|----------------------------|---------------------|\n");

    let mut sorted_instructions = instructions.to_vec();
    sorted_instructions.sort_by(|a, b| a.name.cmp(&b.name));

    for inst in sorted_instructions {
        let isa_support = format_isa_bases(&inst.isa_bases);
        let operand_count = inst.operands.len();
        let assembly_syntax = format_assembly_syntax(&inst.assembly_syntax);
        let operand_names = format_operand_names(&inst.operands);
        let operand_lengths = format_operand_lengths(&inst.operands);
        let operand_restrictions = format_operand_restrictions(&inst.operands);

        report.push_str(&format!(
            "| `{}` | {} | {} | `{}` | {} | {} | {} |\n",
            inst.name,
            isa_support,
            operand_count,
            escape_markdown(&assembly_syntax),
            operand_names,
            operand_lengths,
            operand_restrictions
        ));
    }
}

fn generate_isa_compatibility_report(report: &mut String, instructions: &[Instruction]) {
    let mut rv32_only = 0;
    let mut rv64_only = 0;
    let mut both = 0;
    let mut rv32_extensions: BTreeSet<ISAExtension> = BTreeSet::new();
    let mut rv64_extensions: BTreeSet<ISAExtension> = BTreeSet::new();
    let mut both_extensions: BTreeSet<ISAExtension> = BTreeSet::new();

    for inst in instructions.iter() {
        let has_rv32 = inst.isa_bases.contains(&ISABase::RV32);
        let has_rv64 = inst.isa_bases.contains(&ISABase::RV64);

        match (has_rv32, has_rv64) {
            (true, true) => {
                both += 1;
                both_extensions.insert(inst.extension);
            }
            (true, false) => {
                rv32_only += 1;
                rv32_extensions.insert(inst.extension);
            }
            (false, true) => {
                rv64_only += 1;
                rv64_extensions.insert(inst.extension);
            }
            (false, false) => {} // Should not happen
        }
    }

    report.push_str("| ISA Base | Instruction Count | Extensions |\n");
    report.push_str("|----------|-------------------|------------|\n");
    report.push_str(&format!(
        "| RV32 only | {} | {} |\n",
        rv32_only,
        format_extension_list(&rv32_extensions)
    ));
    report.push_str(&format!(
        "| RV64 only | {} | {} |\n",
        rv64_only,
        format_extension_list(&rv64_extensions)
    ));
    report.push_str(&format!(
        "| RV32 and RV64 | {} | {} |\n",
        both,
        format_extension_list(&both_extensions)
    ));

    report.push_str("\n");
}

fn generate_operand_usage_report(report: &mut String, instructions: &[Instruction]) {
    let mut operand_counts: HashMap<String, usize> = HashMap::new();
    let mut operand_in_extensions: HashMap<String, BTreeSet<ISAExtension>> = HashMap::new();
    let mut operand_lengths: HashMap<String, HashMap<ISABase, BTreeSet<u8>>> = HashMap::new();
    let mut operand_restrictions_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut operand_types: HashMap<String, BTreeSet<OperandType>> = HashMap::new();

    for inst in instructions.iter() {
        for operand in &inst.operands {
            *operand_counts.entry(operand.name.clone()).or_insert(0) += 1;
            operand_in_extensions
                .entry(operand.name.clone())
                .or_insert_with(BTreeSet::new)
                .insert(inst.extension);

            // Collect operand type info
            if let Some(operand_type) = &operand.operand_type {
                operand_types
                    .entry(operand.name.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(operand_type.clone());
            }

            // Collect operand length info
            let lengths = operand_lengths
                .entry(operand.name.clone())
                .or_insert_with(HashMap::new);
            for (base, length) in &operand.bit_lengths {
                lengths
                    .entry(*base)
                    .or_insert_with(BTreeSet::new)
                    .insert(*length);
            }

            // Collect operand constraint info
            if let Some(restrictions) = &operand.restrictions {
                let restriction_desc = format_single_operand_restrictions(restrictions);
                if !restriction_desc.is_empty() {
                    operand_restrictions_map
                        .entry(operand.name.clone())
                        .or_insert_with(Vec::new)
                        .push(restriction_desc);
                }
            }
        }
    }

    let mut operand_vec: Vec<_> = operand_counts.iter().collect();
    operand_vec.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

    report.push_str("### 🏷️ Operand Details\n\n");
    report.push_str(
        "| Operand | Usage Count | Operand Type | Appears In Extensions | Lengths (RV32/RV64) | Constraints |\n",
    );
    report.push_str(
        "|---------|-------------|--------------|-----------------------|---------------------|-------------|\n",
    );

    for (operand_name, count) in operand_vec.iter() {
        let operand_type_str = operand_types
            .get(*operand_name)
            .map(|types| {
                if types.len() == 1 {
                    format!("{}", types.iter().next().unwrap())
                } else {
                    types
                        .iter()
                        .map(|t| format!("{}", t))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            })
            .unwrap_or_else(|| "Unknown".to_string());

        let extensions = operand_in_extensions
            .get(*operand_name)
            .map(|exts| format_extension_list(exts))
            .unwrap_or_default();

        let lengths = operand_lengths
            .get(*operand_name)
            .map(|length_map| format_operand_length_distribution(length_map))
            .unwrap_or_else(|| "Unknown".to_string());

        let restrictions = operand_restrictions_map
            .get(*operand_name)
            .map(|restr_vec| {
                let unique_restrictions: BTreeSet<_> = restr_vec.iter().collect();
                if unique_restrictions.is_empty() {
                    "None".to_string()
                } else {
                    unique_restrictions
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                }
            })
            .unwrap_or_else(|| "None".to_string());

        report.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            operand_name, count, operand_type_str, extensions, lengths, restrictions
        ));
    }

    report.push_str("\n");

    // Add operand length statistics
    generate_operand_length_statistics(report, instructions);

    // Add operand constraint statistics
    generate_operand_restriction_statistics(report, instructions);
}

fn generate_operand_count_statistics(report: &mut String, instructions: &[Instruction]) {
    let mut operand_count_stats: HashMap<usize, Vec<String>> = HashMap::new();

    for inst in instructions.iter() {
        let count = inst.operands.len();
        operand_count_stats
            .entry(count)
            .or_insert_with(Vec::new)
            .push(inst.name.clone());
    }

    let mut count_vec: Vec<_> = operand_count_stats.iter().collect();
    count_vec.sort_by_key(|(count, _)| **count);

    report.push_str("| Operand Count | Instruction Count | Example Instructions |\n");
    report.push_str("|---------------|------------------|----------------------|\n");

    for (count, inst_names) in count_vec {
        let examples = if inst_names.len() <= 5 {
            inst_names
                .iter()
                .map(|s| format!("`{}`", s))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            let first_five: Vec<String> = inst_names
                .iter()
                .take(5)
                .map(|s| format!("`{}`", s))
                .collect();
            format!("{}, ... ({} total)", first_five.join(", "), inst_names.len())
        };

        report.push_str(&format!(
            "| {} | {} | {} |\n",
            count,
            inst_names.len(),
            examples
        ));
    }

    report.push_str("\n");
}

fn get_extension_description(extension: ISAExtension) -> &'static str {
    match extension {
        ISAExtension::I => "Base integer ISA",
        ISAExtension::M => "Multiply/Divide extension",
        ISAExtension::F => "Single-precision floating extension",
        ISAExtension::D => "Double-precision floating extension",
        ISAExtension::Q => "Quad-precision floating extension",
        ISAExtension::C => "Compressed instruction extension",
        ISAExtension::V => "Vector extension",
        ISAExtension::B => "Bit-manipulation extension",
        ISAExtension::H => "Virtualization extension",
        ISAExtension::S => "Privileged architecture extension",
        ISAExtension::Zifencei => "Instruction fence extension",
        ISAExtension::Zicsr => "Control/status register extension",
        ISAExtension::Zaamo => "Atomic memory operations",
        ISAExtension::Zabha => "Byte and halfword atomics",
        ISAExtension::Zacas => "Compare-and-swap atomics",
        ISAExtension::Zalasr => "Load-reserved/store-conditional extension",
        ISAExtension::Zalrsc => "LR/SC atomic extension",
        ISAExtension::Zawrs => "Wait-on-reservation-set extension",
        ISAExtension::Zba => "Bitmanip address generation",
        ISAExtension::Zbb => "Basic bitmanip",
        ISAExtension::Zbc => "Carry-less bitmanip",
        ISAExtension::Zbkb => "Bitmanip crypto (basic)",
        ISAExtension::Zbkx => "Bitmanip crypto (crossbar)",
        ISAExtension::Zbs => "Single-bit operations",
        ISAExtension::Zcb => "Compressed base extension",
        ISAExtension::Zcd => "Compressed double-precision floating",
        ISAExtension::Zcf => "Compressed single-precision floating",
        ISAExtension::Zcmop => "Compressed mop extension",
        ISAExtension::Zcmp => "Compressed pointer extension",
        ISAExtension::Zfbfmin => "Scalar BF16 conversion",
        ISAExtension::Zfh => "Half-precision floating extension",
        ISAExtension::Zicbom => "Cache block management",
        ISAExtension::Zicboz => "Cache block zeroing",
        ISAExtension::Zicfilp => "Control-flow integrity",
        ISAExtension::Zicfiss => "Shadow stack extension",
        ISAExtension::Zicond => "Conditional operation extension",
        ISAExtension::Zilsd => "Load/store pair extension",
        ISAExtension::Zimop => "Maybe-operation extension",
        ISAExtension::Zkn => "NIST crypto base extension",
        ISAExtension::Zknd => "NIST AES decryption extension",
        ISAExtension::Zkne => "NIST AES encryption extension",
        ISAExtension::Zknh => "NIST SHA hashing extension",
        ISAExtension::Zks => "ShangMi crypto extension",
        ISAExtension::Zvbb => "Vector basic bitmanip",
        ISAExtension::Zvbc => "Vector carry-less bitmanip",
        ISAExtension::Zvfbfmin => "Vector BF16 conversion",
        ISAExtension::Zvfbfwma => "Vector BF16 multiply-accumulate",
        ISAExtension::Zvkg => "Vector GCM/GMAC",
        ISAExtension::Zvkned => "Vector NIST AES",
        ISAExtension::Zvknha => "Vector NIST SHA-2",
        ISAExtension::Zvks => "Vector ShangMi",
        ISAExtension::Sdext => "Debug extension",
        ISAExtension::Smdbltrp => "M-mode double-trap extension",
        ISAExtension::Smrnmi => "M-mode recoverable NMI extension",
        ISAExtension::Svinval => "Fine-grained address-translation cache invalidation",
    }
}

fn format_isa_bases(bases: &[ISABase]) -> String {
    if bases.len() == 2 {
        "RV32/64".to_string()
    } else if bases.contains(&ISABase::RV32) {
        "RV32".to_string()
    } else if bases.contains(&ISABase::RV64) {
        "RV64".to_string()
    } else {
        "Unknown".to_string()
    }
}

fn format_assembly_syntax(syntax: &AssemblySyntax) -> String {
    match syntax {
        AssemblySyntax::Format(format) => format.clone(),
        AssemblySyntax::RustCode(_) => {
            "Rust code: omitted".to_string()
        }
    }
}

fn format_extension_list(extensions: &BTreeSet<ISAExtension>) -> String {
    if extensions.len() <= 3 {
        extensions
            .iter()
            .map(|e| format!("{}", e))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let first_three: Vec<String> = extensions
            .iter()
            .take(3)
            .map(|e| format!("{}", e))
            .collect();
        format!("{}, ... ({} total)", first_three.join(", "), extensions.len())
    }
}

fn escape_markdown(text: &str) -> String {
    text.replace("|", "|").replace("{", "{").replace("}", "}")
}

fn format_operand_names(operands: &[Operand]) -> String {
    if operands.is_empty() {
        "None".to_string()
    } else {
        operands
            .iter()
            .map(|op| format!("`{}`", op.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_operand_lengths(operands: &[Operand]) -> String {
    // Print each instruction's bit lengths under RV32 and RV64
    if operands.is_empty() {
        "None".to_string()
    } else {
        let mut lengths = Vec::new();
        for op in operands {
            let mut length_str = String::new();
            for (base, length) in &op.bit_lengths {
                if !length_str.is_empty() {
                    length_str.push_str(", ");
                }
                length_str.push_str(&format!("{}:{}", base, length));
            }
            lengths.push(format!("`{}`: {}", op.name, length_str));
        }
        lengths.join("; ")
    }
}

fn format_operand_restrictions(operands: &[Operand]) -> String {
    if operands.is_empty() {
        "None".to_string()
    } else {
        let restrictions: Vec<String> = operands
            .iter()
            .filter_map(|op| {
                op.restrictions
                    .as_ref()
                    .map(|r| {
                        let desc = format_single_operand_restrictions(r);
                        if desc.is_empty() {
                            None
                        } else {
                            Some(format!("`{}`: {}", op.name, desc))
                        }
                    })
                    .flatten()
            })
            .collect();

        if restrictions.is_empty() {
            "None".to_string()
        } else {
            restrictions.join("; ")
        }
    }
}

fn format_single_operand_restrictions(restrictions: &OperandRestriction) -> String {
    let mut parts = Vec::new();

    if let Some(multiple) = restrictions.multiple_of {
        parts.push(format!("multiple of {}", multiple));
    }

    if let Some((min, max)) = restrictions.min_max {
        parts.push(format!("range [{},{}]", min, max));
    }

    if !restrictions.forbidden_values.is_empty() {
        let forbidden_str = restrictions
            .forbidden_values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("forbidden:{}", forbidden_str));
    }

    parts.join(", ")
}
fn format_operand_length_distribution(length_map: &HashMap<ISABase, BTreeSet<u8>>) -> String {
    let format_lengths = |base: &ISABase| {
        length_map.get(base).map(|set| {
            set.iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
    };

    let rv32_lengths = format_lengths(&ISABase::RV32);
    let rv64_lengths = format_lengths(&ISABase::RV64);

    match (rv32_lengths, rv64_lengths) {
        (Some(rv32), Some(rv64)) => format!("RV32:{}, RV64:{}", rv32, rv64),
        (Some(rv32), None) => format!("RV32:{}, RV64:none", rv32),
        (None, Some(rv64)) => format!("RV32:none, RV64:{}", rv64),
        (None, None) => "RV32:none, RV64:none".to_string(),
    }
}

fn generate_operand_length_statistics(report: &mut String, instructions: &[Instruction]) {
    let mut length_stats: HashMap<(ISABase, u8), usize> = HashMap::new();

    for inst in instructions.iter() {
        for operand in &inst.operands {
            for (base, length) in &operand.bit_lengths {
                *length_stats.entry((*base, *length)).or_insert(0) += 1;
            }
        }
    }

    report.push_str("### 📐 Operand Length Distribution\n\n");
    report.push_str("| ISA Base | Bit Length | Usage Count | Share |\n");
    report.push_str("|----------|------------|-------------|-------|\n");

    let total_operands: usize = length_stats.values().sum();
    let mut stats_vec: Vec<_> = length_stats.iter().collect();
    stats_vec.sort_by_key(|((base, length), _)| (*base, *length));

    for ((base, length), count) in stats_vec {
        let percentage = if total_operands > 0 {
            *count as f64 / total_operands as f64 * 100.0
        } else {
            0.0
        };

        report.push_str(&format!(
            "| {} | {} | {} | {:.1}% |\n",
            base, length, count, percentage
        ));
    }

    report.push_str("\n");
}

fn generate_operand_restriction_statistics(report: &mut String, instructions: &[Instruction]) {
    let mut restriction_stats: HashMap<String, usize> = HashMap::new();
    let mut total_operands = 0;
    let mut restricted_operands = 0;

    for inst in instructions.iter() {
        for operand in &inst.operands {
            total_operands += 1;

            if let Some(restrictions) = &operand.restrictions {
                restricted_operands += 1;

                if restrictions.multiple_of.is_some() {
                    *restriction_stats
                        .entry("Multiple constraint".to_string())
                        .or_insert(0) += 1;
                }

                if restrictions.min_max.is_some() {
                    *restriction_stats.entry("Range constraint".to_string()).or_insert(0) += 1;
                }

                if !restrictions.forbidden_values.is_empty() {
                    *restriction_stats
                        .entry("Forbidden-value constraint".to_string())
                        .or_insert(0) += 1;
                }
            }
        }
    }

    report.push_str("### 🚫 Operand Constraint Statistics\n\n");
    report.push_str("| Constraint Type | Usage Count | Share of Constrained Operands |\n");
    report.push_str("|-----------------|-------------|------------------------------|\n");

    for (restriction_type, count) in restriction_stats.iter() {
        let percentage = if restricted_operands > 0 {
            *count as f64 / restricted_operands as f64 * 100.0
        } else {
            0.0
        };

        report.push_str(&format!(
            "| {} | {} | {:.1}% |\n",
            restriction_type, count, percentage
        ));
    }

    report.push_str(&format!(
        "\n**Total operands**: {}; **Constrained operands**: {} ({:.1}%)\n\n",
        total_operands,
        restricted_operands,
        if total_operands > 0 {
            restricted_operands as f64 / total_operands as f64 * 100.0
        } else {
            0.0
        }
    ));
}
