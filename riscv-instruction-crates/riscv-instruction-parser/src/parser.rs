use crate::types::*;
use std::{
    collections::{HashMap, HashSet}, // Ensure HashSet is imported
    fs,
    path::Path,
};

pub fn parse_insts_from_riscv_unified_db<P: AsRef<Path>>(
    riscv_unified_db_repo_path: P,
) -> Result<Vec<Instruction>, Box<dyn std::error::Error>> {
    let inst_path = riscv_unified_db_repo_path.as_ref().join("arch/inst");

    let mut all_instructions = Vec::new();

    // Read all extension folders
    for entry in fs::read_dir(&inst_path)? {
        let entry = entry?;
        let path = entry.path();

        // Skip non-directory entries
        if !path.is_dir() {
            continue;
        }

        let extension_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();

        // Parse the extension name
        let extension = if let Some(ext) = ISAExtension::from_str(extension_name) {
            ext
        } else {
            panic!("Unknown extension: {}", extension_name);
        };

        // Read all YAML files in the extension folder
        for inst_entry in fs::read_dir(&path)? {
            let inst_entry = inst_entry?;
            let inst_path = inst_entry.path();

            if inst_path.extension().and_then(|s| s.to_str()) != Some("yaml") {
                continue;
            }

            match parse_instruction_file(&inst_path, extension) {
                Ok(instruction) => {
                    all_instructions.push(instruction);
                }
                Err(e) => {
                    panic!("Failed to parse instruction file {:?}: {}", inst_path, e);
                }
            }
        }
    }

    Ok(all_instructions)
}

fn parse_instruction_file(
    file_path: &Path,
    extension: ISAExtension,
) -> Result<Instruction, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    let yaml_inst: YamlInstruction = serde_yaml::from_str(&content)?;

    // Determine which ISA bases are supported
    let isa_bases = determine_isa_bases(&yaml_inst);

    // Parse operands
    let operands = parse_operands(&yaml_inst, &isa_bases)?;

    // If the assembly syntax matches the instruction name (or with dots replaced by underscores), leave it empty
    let name_with_underscores = yaml_inst.name.replace('.', "_");
    let assembly_syntax =
        if yaml_inst.assembly == yaml_inst.name || yaml_inst.assembly == name_with_underscores {
            AssemblySyntax::Format(String::new())
        } else {
            AssemblySyntax::Format(yaml_inst.assembly.clone())
        };

    let instruction = Instruction {
        name: yaml_inst.name,
        extension,
        isa_bases,
        operands,
        assembly_syntax,
        memory_access: None,
    };

    Ok(instruction)
}

fn parse_operands(
    yaml_inst: &YamlInstruction,
    isa_bases: &[ISABase], // Newly added parameter
) -> Result<Vec<Operand>, Box<dyn std::error::Error>> {
    let mut operands = Vec::new();

    if let Some(encoding) = &yaml_inst.encoding {
        match encoding {
            Encoding::Simple { variables, .. } => {
                if let Some(variables) = variables {
                    for var in variables {
                        // For simple encoding, all ISAs use the same bit length,
                        // but we fill it according to the ISAs this instruction supports
                        let bit_lengths_map = parse_bit_range_simple(&var.location, isa_bases)?;
                        let operand_bit_length =
                            bit_lengths_map.values().next().copied().unwrap_or(0);
                        let max_value_for_operand =
                            if operand_bit_length > 0 && operand_bit_length < 8 {
                                ((1u16 << operand_bit_length) - 1) as u64
                            } else if operand_bit_length == 8 {
                                u64::MAX
                            } else if operand_bit_length > 8 {
                                u64::MAX
                            } else {
                                0
                            };

                        let restrictions = parse_restrictions(var, max_value_for_operand);

                        let operand = Operand {
                            name: var.name.clone(),
                            operand_type: None,
                            bit_lengths: bit_lengths_map,
                            restrictions,
                        };

                        operands.push(operand);
                    }
                }
            }
            Encoding::PerISA { rv32, rv64 } => {
                // Collect all variable names
                let mut all_variables: HashMap<String, HashMap<ISABase, String>> = HashMap::new();

                if let Some(rv32_enc) = rv32 {
                    if let Some(vars) = &rv32_enc.variables {
                        for var in vars {
                            all_variables
                                .entry(var.name.clone())
                                .or_insert_with(HashMap::new)
                                .insert(ISABase::RV32, var.location.clone());
                        }
                    }
                }

                if let Some(rv64_enc) = rv64 {
                    if let Some(vars) = &rv64_enc.variables {
                        for var in vars {
                            all_variables
                                .entry(var.name.clone())
                                .or_insert_with(HashMap::new)
                                .insert(ISABase::RV64, var.location.clone());
                        }
                    }
                }

                // Create an operand for each variable
                for (var_name, locations) in all_variables {
                    let mut bit_lengths_map = HashMap::new();
                    let mut max_bit_length = 0u8;

                    // Calculate bit length for each ISA
                    for (isa_base, location) in locations {
                        let bit_length = calculate_bit_length(&location)?;
                        bit_lengths_map.insert(isa_base, bit_length);
                        max_bit_length = max_bit_length.max(bit_length);
                    }

                    let max_value_for_operand = if max_bit_length > 0 && max_bit_length < 8 {
                        ((1u16 << max_bit_length) - 1) as u64
                    } else if max_bit_length == 8 {
                        u64::MAX
                    } else if max_bit_length > 8 {
                        u64::MAX
                    } else {
                        0
                    };

                    // Obtain constraint information from whichever ISA defines the variable
                    let restrictions = if let Some(rv32_enc) = rv32 {
                        if let Some(vars) = &rv32_enc.variables {
                            if let Some(var) = vars.iter().find(|v| v.name == var_name) {
                                parse_restrictions(var, max_value_for_operand)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else if let Some(rv64_enc) = rv64 {
                        if let Some(vars) = &rv64_enc.variables {
                            if let Some(var) = vars.iter().find(|v| v.name == var_name) {
                                parse_restrictions(var, max_value_for_operand)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let operand = Operand {
                        name: var_name,
                        operand_type: None,
                        bit_lengths: bit_lengths_map,
                        restrictions,
                    };

                    operands.push(operand);
                }
            }
        }
    }

    Ok(operands)
}

fn parse_bit_range_simple(
    location: &str,
    isa_bases: &[ISABase], // Newly added parameter
) -> Result<HashMap<ISABase, u8>, Box<dyn std::error::Error>> {
    let bit_length = calculate_bit_length(location)?;

    let mut bit_lengths = HashMap::new();
    for base in isa_bases {
        bit_lengths.insert(*base, bit_length);
    }

    Ok(bit_lengths)
}

fn calculate_bit_length(location: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let mut total_length = 0u8;

    // Handle multiple bit ranges separated by "|", e.g., "31-25|11-7" or "12|6-2"
    for part in location.split('|') {
        let part = part.trim();

        if let Some((high_str, low_str)) = part.split_once('-') {
            // Bit range format: high-low
            let high: u8 = high_str
                .trim()
                .parse()
                .map_err(|e| format!("Failed to parse high bit '{}': {}", high_str.trim(), e))?;
            let low: u8 = low_str
                .trim()
                .parse()
                .map_err(|e| format!("Failed to parse low bit '{}': {}", low_str.trim(), e))?;

            if high < low {
                return Err(format!("Invalid bit range: {} < {}", high, low).into());
            }

            let range_length = high - low + 1;
            total_length += range_length;
        } else {
            total_length += 1;
        }
    }

    Ok(total_length)
}

fn parse_restrictions(var: &Variable, max_value_for_operand: u64) -> Option<OperandRestriction> {
    let mut restriction = OperandRestriction::default();
    let mut has_any_restriction_field_set = false;

    // Parse the "left_shift" constraint
    if let Some(left_shift_yaml) = &var.left_shift {
        has_any_restriction_field_set = true;
        let multiple_of_value = 1u16 << left_shift_yaml; // 2^shift_amount
        if multiple_of_value <= u16::MAX {
            restriction.multiple_of = Some(multiple_of_value.try_into().unwrap());
        }
    }

    // Parse the "not" constraint
    if let Some(not_values_yaml) = &var.not_values {
        has_any_restriction_field_set = true;
        let mut temp_forbidden_values = HashSet::new();

        match not_values_yaml {
            serde_yaml::Value::Number(n) => {
                if let Some(val_u64) = n.as_u64() {
                    if val_u64 <= i64::MAX as u64 {
                        let val_i64 = val_u64 as i64;
                        temp_forbidden_values.insert(val_i64);
                    }
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for item in seq {
                    if let Some(n_u64) = item.as_u64() {
                        if n_u64 <= i64::MAX as u64 {
                            let val_i64 = n_u64 as i64;
                            temp_forbidden_values.insert(val_i64);
                        }
                    }
                }
            }
            _ => {} // Ignore other value types
        }

        // If multiple_of is already set, check for conflicts with forbidden_values
        if let Some(multiple_of) = restriction.multiple_of {
            // Filter out forbidden values that are not multiples of the constraint
            temp_forbidden_values.retain(|&val| val % (multiple_of as i64) == 0);

            // Directly set the filtered forbidden_values
            if !temp_forbidden_values.is_empty() {
                let mut forbidden_values: Vec<i64> = temp_forbidden_values.into_iter().collect();
                forbidden_values.sort_unstable();
                restriction.forbidden_values = forbidden_values;
            }
        } else {
            // Check whether all odd numbers are forbidden (implying multiple_of: 2)
            let all_odd_numbers_in_range: HashSet<i64> =
                (1..=max_value_for_operand as i64).step_by(2).collect();

            if !all_odd_numbers_in_range.is_empty()
                && all_odd_numbers_in_range.is_subset(&temp_forbidden_values)
            {
                restriction.multiple_of = Some(2);

                // Values explained by this pattern
                let mut explained_by_pattern = all_odd_numbers_in_range;
                // If 0 itself is forbidden
                if temp_forbidden_values.contains(&0) {
                    explained_by_pattern.insert(0);
                }

                // Remaining forbidden values not covered by multiple_of:2 (and possibly forbidden 0)
                let remaining_forbidden: Vec<i64> = temp_forbidden_values
                    .difference(&explained_by_pattern)
                    .cloned()
                    .collect();

                let mut sorted_remaining: Vec<i64> = remaining_forbidden;
                sorted_remaining.sort_unstable(); // keep ordering consistent
                restriction.forbidden_values = sorted_remaining;
            } else {
                // If it doesn't fit the multiple_of:2 pattern, use collected forbidden values
                let mut all_forbidden_from_yaml: Vec<_> =
                    temp_forbidden_values.into_iter().collect();
                all_forbidden_from_yaml.sort_unstable(); // keep ordering consistent
                restriction.forbidden_values = all_forbidden_from_yaml;
            }
        }
    }

    // Return Some if any restriction field is set (non-default)
    if restriction != OperandRestriction::default() || has_any_restriction_field_set {
        if restriction != OperandRestriction::default() {
            Some(restriction)
        } else if has_any_restriction_field_set {
            // e.g., a case like not: []
            Some(OperandRestriction::default()) // Return an empty (default) restriction
        } else {
            None
        }
    } else {
        None
    }
}

fn determine_isa_bases(yaml_inst: &YamlInstruction) -> Vec<ISABase> {
    // Check for an explicit base field first
    if let Some(base_value) = &yaml_inst.base {
        match base_value {
            32 => return vec![ISABase::RV32],
            64 => return vec![ISABase::RV64],
            _ => {}
        }
    }

    // Otherwise support both by default
    vec![ISABase::RV32, ISABase::RV64]
}
