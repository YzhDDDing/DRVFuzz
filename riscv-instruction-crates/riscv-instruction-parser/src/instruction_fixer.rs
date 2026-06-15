use crate::{operand_extractor::extract_operands_from_asm_without_name, types::*};
use std::collections::{HashMap, HashSet};

/// Fix all issues in instruction definitions
pub fn fix_instructions(instructions: &mut [Instruction]) {
    for instruction in instructions.iter_mut() {
        fix_instruction(instruction);
    }
}

/// Fix a single instruction
fn fix_instruction(instruction: &mut Instruction) {
    // Instructions should not use RustCode syntax; before fixing they should be YAML-derived Format
    if matches!(instruction.assembly_syntax, AssemblySyntax::RustCode(_)) {
        panic!(
            "Unexpected RustCode syntax for instruction: {}",
            instruction.name
        );
    }

    let mut assembly_syntax = match &instruction.assembly_syntax {
        AssemblySyntax::Format(s) => s.clone(),
        AssemblySyntax::RustCode(_) => unreachable!(),
    };

    // Extract operands from the original assembly syntax
    let mut syntax_operands = extract_operands_from_asm_without_name(&assembly_syntax);

    // Reconcile operand names so instruction.operands match the assembly syntax
    reconcile_operand_names(instruction, &syntax_operands);

    // Handle special-case instructions
    handle_special_instruction(&mut assembly_syntax, instruction, &mut syntax_operands);

    // Generate final assembly syntax based on instruction type
    instruction.assembly_syntax = generate_assembly_syntax(
        &instruction.name,
        &assembly_syntax,
        &syntax_operands,
        instruction,
    );
    // Assign operand_type for every operand
    assign_operand_types(instruction);
}

/// Generate the final assembly syntax according to the instruction type
fn generate_assembly_syntax(
    name: &str,
    assembly_syntax: &str,
    syntax_operands: &HashSet<String>,
    instruction: &Instruction,
) -> AssemblySyntax {
    let operands_segment = wrap_operands_with_braces(&assembly_syntax, &syntax_operands);

    if name == "c.lui" {
        // c.lui uses custom formatting code
        AssemblySyntax::RustCode(
            "format!(\"c.lui {}, 0x{:x}\", xd, imm.get() as u32 & 0xfffff)".to_string(),
        )
    } else if name == "fcvtmod.w.d" {
        // fcvtmod.w.d uses the hard-coded rtz rounding mode
        AssemblySyntax::Format(format!("fcvtmod.w.d {}, rtz", operands_segment))
    } else if matches!(name, "cm.pop" | "cm.push" | "cm.popretz" | "cm.popret") {
        // cm.* instructions use custom formatting for SavedRegListWithStackAdj
        AssemblySyntax::RustCode(generate_cm_instruction_assembly_code(name))
    } else if matches!(name, "c.sh" | "c.lh" | "c.lhu") {
        // These compressed instructions use a 1-bit boolean uimm that must be turned into an offset
        AssemblySyntax::RustCode(generate_compressed_offset_assembly_code(
            name,
            &operands_segment,
            instruction,
        ))
    } else if matches!(name, "vs1r.v" | "vs2r.v" | "vs4r.v" | "vs8r.v") {
        // vs*r.v instructions use an address-with-offset format
        AssemblySyntax::Format(format!("{} {{vs3}}, 0({{xs1}})", name))
    } else if is_aqrl_instruction(instruction) {
        // For aqrl-bearing instructions, generate dedicated Rust formatting code
        AssemblySyntax::RustCode(generate_aqrl_assembly_code(
            name,
            &operands_segment,
            instruction,
        ))
    } else if is_mop_instruction(instruction) {
        // For MOP instructions, generate dedicated Rust formatting code
        AssemblySyntax::RustCode(generate_mop_assembly_code(
            name,
            &operands_segment,
            instruction,
        ))
    } else if is_vector_instruction_with_vm(instruction) {
        // For vector instructions with a vm operand, generate dedicated Rust formatting code
        AssemblySyntax::RustCode(generate_vector_vm_assembly_code(
            name,
            &operands_segment,
            instruction,
        ))
    } else if matches!(
        name,
        "sspush.x1" | "sspush.x5" | "sspopchk.x1" | "sspopchk.x5"
    ) {
        // Shadow-stack instructions use hard-coded formatting
        AssemblySyntax::Format(name.replace(".", " "))
    } else {
        // For ordinary instructions, prepend the name and wrap operands
        let final_assembly = if operands_segment.trim().is_empty() {
            name.to_string()
        } else {
            format!("{} {}", name, operands_segment)
        };
        AssemblySyntax::Format(final_assembly)
    }
}

/// Reconcile operand name mappings
fn reconcile_operand_names(
    instruction: &mut Instruction,
    syntax_operands: &std::collections::HashSet<String>,
) {
    if syntax_operands.is_empty() && instruction.operands.is_empty() {
        return;
    }

    let name_mapping = create_operand_name_mapping();
    let mut available_syntax_ops: Vec<String> = syntax_operands.iter().cloned().collect();
    let mut updated_operands = Vec::with_capacity(instruction.operands.len());

    for operand in &instruction.operands {
        let new_name =
            find_matching_operand_name(&operand.name, &mut available_syntax_ops, &name_mapping);

        updated_operands.push(Operand {
            name: new_name,
            operand_type: operand.operand_type.clone(),
            bit_lengths: operand.bit_lengths.clone(),
            restrictions: operand.restrictions.clone(),
        });
    }

    instruction.operands = updated_operands;
}

/// Build the operand name mapping table
fn create_operand_name_mapping() -> HashMap<&'static str, Vec<&'static str>> {
    [
        ("rd", vec!["xd", "fd", "qd"]),
        ("rs1", vec!["xs1", "fs1", "qs1"]),
        ("rs2", vec!["xs2", "fs2", "qs2"]),
        ("rs3", vec!["xs3", "fs3"]),
        ("fd", vec!["xd"]),
        ("fs1", vec!["xs1"]),
        ("fs2", vec!["xs2"]),
        ("fs3", vec!["xs3"]),
        ("rm", vec!["frm"]),
        ("rlist", vec!["reg_list"]),
        ("spimm", vec!["stack_adj"]),
        ("imm", vec!["offset", "imm12", "uimm"]),
        ("uimm", vec!["imm", "nzuimm"]),
        ("nzuimm", vec!["uimm", "imm"]),
        ("zimm5", vec!["imm"]),
    ]
    .iter()
    .map(|(k, v)| (*k, v.to_vec()))
    .collect()
}

/// Find a matching operand name
fn find_matching_operand_name(
    current_name: &str,
    available_syntax_ops: &mut Vec<String>,
    name_mapping: &HashMap<&str, Vec<&str>>,
) -> String {
    // Prefer direct matches
    if let Some(pos) = available_syntax_ops.iter().position(|x| x == current_name) {
        return available_syntax_ops.remove(pos);
    }

    // Otherwise try mapped equivalents
    if let Some(possible_names) = name_mapping.get(current_name) {
        for &mapped_name in possible_names {
            if let Some(pos) = available_syntax_ops.iter().position(|x| x == mapped_name) {
                return available_syntax_ops.remove(pos);
            }
        }
    }

    current_name.to_string()
}

/// Handle all special instructions' assembly syntax and operands uniformly
fn handle_special_instruction(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    // Fix vtypei being misnamed as xs1 in vsetvli
    fix_vsetvli_operand_names(instruction);
    // Fix fence instructions whose assembly syntax was marked "TODO"
    fix_fence_instruction(operands_part, instruction, final_syntax_operands);
    // Fix the special immediate range for c.lui
    fix_clui_instruction(operands_part, instruction, final_syntax_operands);
    // Fix instructions that need their round mode removed
    fix_round_mode_removal(operands_part, instruction, final_syntax_operands);
    // Fix the incorrect second operand type for fli instructions
    fix_fli_instruction_operands(operands_part, instruction, final_syntax_operands);
    // Fix instructions where uimm was mistakenly written as imm
    fix_uimm_instead_of_imm(operands_part, instruction, final_syntax_operands);
    // Fix base+offset addressing syntax for certain floating-point load/store instructions
    fix_floating_point_load_store_addressing(operands_part, instruction, final_syntax_operands);
    // Fix instructions that misuse floating-point vs integer registers
    fix_floating_point_register_errors(instruction, operands_part, final_syntax_operands);
    // Fix instructions that misuse vector vs integer registers
    fix_vector_register_errors_wrapper(instruction, operands_part, final_syntax_operands);
    // Fix any missing round mode
    fix_missing_round_mode(operands_part, instruction, final_syntax_operands);
    fix_compressed_stack_pointer_instructions(operands_part, instruction, final_syntax_operands);
    // Fix compressed stack-pointer instructions missing the (sp) suffix
    fix_compressed_stack_adjustment_instructions(operands_part, instruction);
    // Fix the missing operand on add.uw
    fix_add_uw_missing_operands(instruction);
    // Fix address syntax for hypervisor load/store instructions
    fix_hypervisor_load_store_instructions(operands_part, instruction);
    // Fix the rnum operand range for aes64ks1i
    fix_aes64ks1i_rnum_range(instruction);
    // Fix ISABase mistakes (e.g., RV32-only encoded as RV32+RV64)
    fix_isabase_error_instructions(instruction);
    // Fix c.mop.n where n was not constrained to odd values
    fix_c_mop_n_instruction(instruction);
    // Fix cm.* merging of reg_list and stack_adj
    fix_cm_instructions_reg_list_stack_adj(instruction, operands_part, final_syntax_operands);
    // Fix operand merge for cm.mvsa01
    fix_cm_mvsa01_instruction(instruction, operands_part, final_syntax_operands);

    // Should run last: ensure v0 and sp are absent from final operands
    fix_vector_v0_hardcoded_operands(final_syntax_operands);
    fix_sp_should_not_in_final_syntax_operands(final_syntax_operands);
}

fn fix_c_mop_n_instruction(instruction: &mut Instruction) {
    // Ensure c.mop.n constrains n to odd values
    if instruction.name == "c.mop.n" {
        instruction
            .operands
            .iter_mut()
            .find(|op| op.name == "n")
            .map(|op| {
                op.restrictions = Some(OperandRestriction {
                    multiple_of: None,
                    min_max: None,
                    odd_only: Some(true),
                    forbidden_values: Vec::with_capacity(0),
                });
            });
    }
}

fn fix_add_uw_missing_operands(instruction: &mut Instruction) {
    // Add the missing operands for add.uw
    if instruction.name == "add.uw" {
        instruction.operands.push(Operand {
            name: "xd".to_string(),
            operand_type: Some(OperandType::IntegerRegister),
            bit_lengths: HashMap::from([(ISABase::RV32, 5), (ISABase::RV64, 5)]),
            restrictions: None,
        });

        instruction.operands.push(Operand {
            name: "xs1".to_string(),
            operand_type: Some(OperandType::IntegerRegister),
            bit_lengths: HashMap::from([(ISABase::RV32, 5), (ISABase::RV64, 5)]),
            restrictions: None,
        });

        instruction.operands.push(Operand {
            name: "xs2".to_string(),
            operand_type: Some(OperandType::IntegerRegister),
            bit_lengths: HashMap::from([(ISABase::RV32, 5), (ISABase::RV64, 5)]),
            restrictions: None,
        });
    }
}

fn fix_sp_should_not_in_final_syntax_operands(
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    // Ensure sp is not present in the final syntax operands
    if final_syntax_operands.contains("sp") {
        final_syntax_operands.remove("sp");
    }
}

/// Fix incorrect operand naming in vsetvli
fn fix_vsetvli_operand_names(instruction: &mut Instruction) {
    if instruction.name == "vsetvli" {
        instruction
            .operands
            .iter_mut()
            .find(|op| op.name == "xs1" && op.bit_lengths.get(&ISABase::RV32) == Some(&11))
            .map(|op| op.name = "vtypei".to_string());
    }
}

/// Fix fence instructions that were described as "TODO"
fn fix_fence_instruction(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if instruction.name != "fence" {
        return;
    }

    *operands_part = "pred, succ".to_string();
    final_syntax_operands.insert("pred".to_string());
    final_syntax_operands.insert("succ".to_string());
}

/// Fix the special-case handling for c.lui
fn fix_clui_instruction(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if instruction.name != "c.lui" {
        return;
    }

    // Correct operand restriction: replace multiple_of 4096 with range limit and forbidden value
    instruction
        .operands
        .iter_mut()
        .find(|op| op.name == "imm")
        .map(|op| {
            op.restrictions = Some(OperandRestriction {
                multiple_of: None,
                min_max: Some((-32, 31)),
                forbidden_values: vec![0],
                odd_only: None,
            });
        });

    // Clear operand segment because assembly syntax will be generated by generate_assembly_syntax
    *operands_part = String::new();
    final_syntax_operands.clear();
}

/// Fix instructions that need the round mode removed
fn fix_round_mode_removal(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(
        instruction.name.as_str(),
        "fcvt.d.w"
            | "fcvt.d.wu"
            | "fcvt.d.s"
            | "fcvt.q.w"
            | "fcvt.q.wu"
            | "fcvt.q.s"
            | "fcvt.q.d"
            | "fcvt.q.h"
            | "fcvt.s.bf16"
            | "fcvt.s.h"
            | "fcvt.d.h"
            | "fcvtmod.w.d"
    ) {
        return;
    }

    // Remove rm/frm operands from the operand definitions
    instruction
        .operands
        .retain(|op| op.name != "rm" && op.name != "frm");

    // Remove round-mode references from assembly syntax
    *operands_part = operands_part.replace(", rm", "").replace(", frm", "");

    // Remove them from the syntax-operand set
    final_syntax_operands.remove("rm");
    final_syntax_operands.remove("frm");
}

/// Fix operand type errors in FLI instructions
fn fix_fli_instruction_operands(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(
        instruction.name.as_str(),
        "fli.d" | "fli.s" | "fli.h" | "fli.q"
    ) {
        return;
    }

    // The second operand of these instructions should be an unsigned immediate, not a register
    if let Some(operand) = instruction
        .operands
        .iter_mut()
        .find(|op| matches!(op.name.as_str(), "qs1" | "fs1" | "xs1" | "imm"))
    {
        let old_name = std::mem::replace(&mut operand.name, "uimm".to_string());
        operand.operand_type = Some(OperandType::FliConstant);

        if final_syntax_operands.remove(&old_name) {
            final_syntax_operands.insert("uimm".to_string());
            *operands_part = operands_part.replace(&old_name, "uimm");
        }
    }
}

/// Fix instructions where imm should be uimm
fn fix_uimm_instead_of_imm(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(
        instruction.name.as_str(),
        // Base instructions
        "lui" | "auipc" |
        // CSR instructions
        "csrrsi" | "csrrwi" | "csrrci" |
        // Immediates in compressed instructions (typically unsigned)
        "c.fswsp" | "c.sdsp" | "c.lwsp" | "c.ldsp" | "c.fsdsp" | "c.fldsp" | "c.swsp" | "c.flwsp" | "c.fld" | "c.fsd" | "c.ld" | "c.sd" | "c.sw" | "c.lw" | "c.flw" | "c.fsw"| "c.addi4spn" | "c.srai" | "c.srli" | "c.slli" |
        // Zcb extension
        "c.lbu" | "c.lh" | "c.lhu" | "c.sb" | "c.sh" |
        // V extension - vector slide instructions
        "vslidedown.vi" | "vslideup.vi" |
        // V extension - vector gather instructions
        "vrgather.vi" |
        // V extension - vector logical/arithmetic shift instructions
        "vsll.vi" | "vsrl.vi" | "vsra.vi" |
        // V extension - vector narrowing saturation instructions
        "vnclipu.wi" |
        // V extension - vector narrowing/saturation shift instructions
        "vnsra.wi" | "vnsrl.wi" | "vssra.wi" | "vssrl.wi" |
        // V extension - vector saturated arithmetic/logical right-shift immediates
        "vssra.vi" | "vssrl.vi" |
        // Zvbb extension
        "vror.vi" |
        // Zvbb extension - vector widening shift instructions
        "vwsll.vi" |
        "vnclip.wi"|
        "lpad" |
        "vaeskf1.vi" | "vaeskf2.vi" | "vsm3c.vi" | "vsm4k.vi"
    ) {
        return;
    }

    // Rename operands: change imm to uimm
    for operand in instruction.operands.iter_mut() {
        if operand.name == "imm" {
            operand.name = "uimm".to_string();
        }
    }

    // Update operand references in assembly syntax
    if final_syntax_operands.contains("imm") {
        final_syntax_operands.remove("imm");
        final_syntax_operands.insert("uimm".to_string());
        *operands_part = operands_part.replace("imm", "uimm");
    }
}

/// Fix base+offset addressing for specific floating load/store instructions
fn fix_floating_point_load_store_addressing(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(instruction.name.as_str(), "flw" | "fsw" | "fld" | "flq") {
        return;
    }

    let (expected_first_reg, expected_base_reg, expected_offset_imm) =
        match instruction.name.as_str() {
            "flw" => ("fd", "xs1", "imm"),
            "fsw" => ("fs2", "xs1", "imm"), // fs2 is the source register
            "fld" => ("fd", "xs1", "imm"),
            "flq" => ("fd", "xs1", "imm"),
            _ => return,
        };

    let current_operands_str = operands_part.trim();
    let parts: Vec<&str> = current_operands_str.split(',').map(|s| s.trim()).collect();

    if parts.len() == 3
        && parts[0] == expected_first_reg
        && parts[1] == expected_base_reg
        && parts[2] == expected_offset_imm
        && final_syntax_operands.contains(expected_first_reg)
        && final_syntax_operands.contains(expected_base_reg)
        && final_syntax_operands.contains(expected_offset_imm)
    {
        // Convert from "reg, base, imm" to "reg, imm(base)"
        *operands_part = format!(
            "{}, {}({})",
            expected_first_reg, expected_offset_imm, expected_base_reg
        );
    }
}

/// Wrapper to fix vector register type errors
fn fix_vector_register_errors_wrapper(
    instruction: &mut Instruction,
    operands_part: &mut String,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    match instruction.name.as_str() {
        "vmv.x.s" => {
            fix_operand_name(instruction, "vd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "vd", "xd");
        }
        "vcpop.m" => {
            fix_operand_name(instruction, "vd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "vd", "xd");
        }
        _ => {}
    }
}

/// Fix situations where the round mode operand is missing
fn fix_missing_round_mode(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !instruction.operands.iter().any(|o| o.name == "rm") || final_syntax_operands.contains("rm")
    {
        return;
    }

    if operands_part.trim().is_empty() {
        *operands_part = "rm".to_string();
    } else {
        *operands_part = format!(
            "{}, rm",
            operands_part.trim_end_matches(|c: char| c.is_whitespace() || c == ',')
        );
    }

    final_syntax_operands.insert("rm".to_string());
}

/// Fix compressed stack-pointer instructions by adding the (sp) suffix
fn fix_compressed_stack_pointer_instructions(
    operands_part: &mut String,
    instruction: &mut Instruction,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(
        instruction.name.as_str(),
        "c.flwsp" | "c.fswsp" | "c.ldsp" | "c.lwsp" | "c.sdsp" | "c.swsp" | "c.fsdsp" | "c.fldsp"
    ) {
        return;
    }

    let expected_pattern = match instruction.name.as_str() {
        "c.flwsp" => ("fd", "uimm"),
        "c.fswsp" => ("fs2", "uimm"),
        "c.ldsp" => ("xd", "uimm"),
        "c.lwsp" => ("xd", "uimm"),
        "c.sdsp" => ("xs2", "uimm"),
        "c.swsp" => ("xs2", "uimm"),
        "c.fsdsp" => ("fs2", "uimm"),
        "c.fldsp" => ("fd", "uimm"),
        _ => return,
    };

    let current_operands_str = operands_part.trim();
    let parts: Vec<&str> = current_operands_str.split(',').map(|s| s.trim()).collect();

    if parts.len() == 2
        && parts[0] == expected_pattern.0
        && parts[1] == expected_pattern.1
        && final_syntax_operands.contains(expected_pattern.0)
        && final_syntax_operands.contains(expected_pattern.1)
    {
        // Convert from "{reg}, {uimm}" to "{reg}, {uimm}(sp)"
        *operands_part = format!("{}, {}(sp)", expected_pattern.0, expected_pattern.1);
    }
}

/// Fix c.addi4spn and c.addi16sp by adding the sp register
fn fix_compressed_stack_adjustment_instructions(
    operands_part: &mut String,
    instruction: &mut Instruction,
) {
    if instruction.name == "c.addi4spn" {
        // Convert from "xd, uimm" to "xd, sp, uimm"
        *operands_part = "xd, sp, uimm".to_string();
    } else if instruction.name == "c.addi16sp" {
        *operands_part = "sp, imm".to_string();
    }
}

/// Remove hard-coded v0 operands in vector instructions
fn fix_vector_v0_hardcoded_operands(final_syntax_operands: &mut std::collections::HashSet<String>) {
    final_syntax_operands.remove("v0");
}

/// Fix incorrect floating-point register usage
fn fix_floating_point_register_errors(
    instruction: &mut Instruction,
    operands_part: &mut String,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    match instruction.name.as_str() {
        // Q extension instructions
        "fcvt.d.q" => {
            // Fix: xd -> fd (destination should be a floating register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "fcvt.h.q" => {
            // Fix: xd -> hd (destination should be a half-precision floating register)
            fix_operand_name(instruction, "xd", "hd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "hd");
        }
        "fcvt.lu.q" => {
            // Fix: hs1 -> xs1 (source should be an integer register)
            fix_operand_name(instruction, "qd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "qd", "xd");
        }
        "fcvt.wu.q" => {
            // Fix: xs1 -> qs1 (source should be a quad-precision floating register)
            fix_operand_name(instruction, "xs1", "qs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "qs1");
        }
        "fmin.q" => {
            // Fix: all integer registers -> quad-precision floating registers
            fix_operand_name(instruction, "xd", "qd");
            fix_operand_name(instruction, "xs1", "qs1");
            fix_operand_name(instruction, "xs2", "qs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "qd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "qs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "qs2");
        }
        // F extension instructions
        "fmaxm.s" | "fmin.s" => {
            // Fix: all integer registers -> single-precision floating registers
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        "fnmsub.s" => {
            // Fix: all integer registers -> single-precision floating registers
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            fix_operand_name(instruction, "xs3", "fs3");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xs3", "fs3");
        }
        "fround.s" | "froundnx.s" => {
            // Fix: xs1 -> fs1 (source should be a single-precision floating register)
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        // D extension - floating arithmetic instructions (all operands should be floating registers)
        "fadd.d" | "fsub.d" | "fmul.d" | "fdiv.d" => {
            // Fix: xd -> fd, xs1 -> fs1, xs2 -> fs2
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        "fsqrt.d" | "fround.d" | "froundnx.d" => {
            // Fix: xd -> fd, xs1 -> fs1
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        "fmadd.d" | "fmsub.d" | "fnmadd.d" | "fnmsub.d" => {
            // Fix: xd -> fd, xs1 -> fs1, xs2 -> fs2, xs3 -> fs3
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            fix_operand_name(instruction, "xs3", "fs3");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xs3", "fs3");
        }
        "fli.d" => {
            // Fix: xd -> fd
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        // D extension - int to float (source integer register, destination floating register)
        "fcvt.d.l" | "fcvt.d.lu" | "fcvt.d.w" | "fcvt.d.wu" => {
            // Fix: xd -> fd (xs1 stays an integer register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        // D extension - float to int (source floating register, destination integer register)
        "fcvt.l.d" | "fcvt.lu.d" | "fcvt.w.d" | "fcvt.wu.d" | "fcvtmod.w.d" => {
            // Fix: xs1 -> fs1 (xd remains an integer register)
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        // D extension - float-to-float conversions
        "fcvt.d.s" => {
            // Fix: xd -> fd, xs1 -> fs1
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        "fcvt.s.d" => {
            // Fix: xd -> fd, xs1 -> fs1
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        // D extension - floating comparisons (source floating, destination integer)
        "feq.d" | "fle.d" | "fleq.d" | "flt.d" | "fltq.d" => {
            // Fix: xs1 -> fs1, xs2 -> fs2 (xd remains integer)
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        "fclass.d" => {
            // Fix: xs1 -> fs1 (xd remains integer)
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        // D extension - floating max/min and sign ops (all operands floating)
        "fmax.d" | "fmaxm.d" | "fmin.d" | "fminm.d" | "fsgnj.d" | "fsgnjn.d" | "fsgnjx.d" => {
            // Fix: xd -> fd, xs1 -> fs1, xs2 -> fs2
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        // D extension - floating move instructions
        "fmv.d.x" => {
            // Fix: xd -> fd (xs1 stays integer)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "fmv.x.d" | "fmvh.x.d" => {
            // Fix: xs1 -> fs1 (xd stays integer)
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        "fmvp.d.x" => {
            // Fix: xd -> fd (destination floating, sources remain integer)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            // Note: xs1 and xs2 remain integer registers; no change needed
        }
        // Zcd extension - compressed double-precision floating instructions
        "c.fld" => {
            // Fix: xd -> fd (floating load target should be floating register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "c.fsd" => {
            // Fix: xs2 -> fs2 (floating store source should be floating register)
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        // Zcf extension - compressed single-precision floating instructions
        "c.flw" => {
            // Fix: xd -> fd (floating load target should be floating register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "c.fsw" => {
            // Fix: xs2 -> fs2 (floating store source should be floating register)
            fix_operand_name(instruction, "xs2", "fs2");
            update_syntax_operands(operands_part, final_syntax_operands, "xs2", "fs2");
        }
        // Zfbfmin extension - scalar BF16 floating instructions
        "fcvt.bf16.s" | "fcvt.s.bf16" => {
            // Fix: xd -> fd, xs1 -> fs1 (both operands should be floating registers)
            fix_operand_name(instruction, "xd", "fd");
            fix_operand_name(instruction, "xs1", "fs1");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xs1", "fs1");
        }
        // Zfh extension - half-precision floating instructions
        "fclass.h" | "fcvt.l.h" | "fcvt.lu.h" | "fcvt.w.h" | "fcvt.wu.h" | "feq.h" | "fle.h"
        | "fleq.h" | "flt.h" | "fltq.h" => {
            // Fix: fd -> xd (results of floating comparisons go to integer registers)
            fix_operand_name(instruction, "fd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "fd", "xd");
        }
        "flh" | "fcvt.h.wu" | "fcvt.h.w" | "fcvt.h.lu" | "fcvt.h.l" => {
            // Fix: xd -> fd (floating load target should be floating register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "fmv.h.x" => {
            // Fix: xd -> fd (moving from integer to floating register)
            fix_operand_name(instruction, "xd", "fd");
            update_syntax_operands(operands_part, final_syntax_operands, "xd", "fd");
        }
        "fmv.x.h" => {
            // Fix: fd -> xd (moving from floating to integer register)
            fix_operand_name(instruction, "fd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "fd", "xd");
        }
        "fltq.q" => {
            // Fix: qd -> xd (floating comparison results go to integer registers)
            fix_operand_name(instruction, "fd", "xd");
            update_syntax_operands(operands_part, final_syntax_operands, "fd", "xd");
        }

        _ => {}
    }
}

/// Fix operand names inside instructions
fn fix_operand_name(instruction: &mut Instruction, old_name: &str, new_name: &str) {
    for operand in instruction.operands.iter_mut() {
        if operand.name == old_name {
            operand.name = new_name.to_string();
        }
    }
}

/// Update operand references within assembly syntax
fn update_syntax_operands(
    operands_part: &mut String,
    final_syntax_operands: &mut std::collections::HashSet<String>,
    old_name: &str,
    new_name: &str,
) {
    if final_syntax_operands.contains(old_name) {
        final_syntax_operands.remove(old_name);
        final_syntax_operands.insert(new_name.to_string());
        *operands_part = operands_part.replace(old_name, new_name);
    }
}

/// Wrap operands with braces
fn wrap_operands_with_braces(
    asm_without_name: &str,
    syntax_operands: &std::collections::HashSet<String>,
) -> String {
    if asm_without_name.trim().is_empty() || syntax_operands.is_empty() {
        return asm_without_name.to_string();
    }

    let mut result = asm_without_name.to_string();

    for operand_name in syntax_operands {
        result = result.replace(operand_name, &format!("{{{}}}", operand_name));
    }

    result
}

/// Check whether this is an AMO instruction
fn is_aqrl_instruction(instruction: &Instruction) -> bool {
    instruction
        .operands
        .iter()
        .any(|o| o.name == "aq" || o.name == "rl")
}

/// Check whether an aqrl instruction needs address wrapping
fn needs_address_wrapping(name: &str) -> bool {
    matches!(
        name,
        "ssamoswap.d" | "ssamoswap.w" | "sc.d" | "sc.w" | "lr.d" | "lr.w"
    )
}

/// Generate assembly code for instructions containing aqrl
fn generate_aqrl_assembly_code(
    name: &str,
    operands_segment: &str,
    instruction: &Instruction,
) -> String {
    let has_aq = instruction.operands.iter().any(|o| o.name == "aq");
    let has_rl = instruction.operands.iter().any(|o| o.name == "rl");

    // Check if address wrapping is needed
    if needs_address_wrapping(name) {
        // For such instructions, wrap the last operand with 0()
        let operands_without_braces = operands_segment.replace("{", "").replace("}", "");

        let operands: Vec<&str> = operands_without_braces
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let (prefix_operands, last_operand) = if operands.len() > 0 {
            let last_idx = operands.len() - 1;
            let prefix = if last_idx > 0 {
                // Add braces to preceding operands
                let prefix_with_braces: Vec<String> = operands[..last_idx]
                    .iter()
                    .map(|op| format!("{{{}}}", op))
                    .collect();
                format!(" {}, ", prefix_with_braces.join(", "))
            } else {
                " ".to_string()
            };
            (prefix, operands[last_idx])
        } else {
            (" ".to_string(), "")
        };

        let escaped_prefix = prefix_operands.replace('\\', "\\\\").replace('"', "\\\"");

        let last_operand = last_operand.replace('(', "").replace(')', "");

        match (has_aq, has_rl) {
            (true, true) => format!(
                r#"{{
    let suffix = if *aq && *rl {{
        ".aqrl"
    }} else if *aq {{
        ".aq"
    }} else if *rl {{
        ".rl"
    }} else {{
        ""
    }};
    format!("{name}{{suffix}}{escaped_prefix}0({{{last_operand}}})")
}}"#
            ),
            (true, false) => format!(
                r#"{{
    let suffix = if *aq {{
        ".aq"
    }} else {{
        ""
    }};
    format!("{name}{{suffix}}{escaped_prefix}0({{{last_operand}}})")
}}"#
            ),
            (false, true) => format!(
                r#"{{
    let suffix = if *rl {{
        ".rl"
    }} else {{
        ""
    }};
    format!("{name}{{suffix}}{escaped_prefix}0({{{last_operand}}})")
}}"#
            ),
            (false, false) => format!(r#"format!("{name}{escaped_prefix}0({{{last_operand}}})"#),
        }
    } else {
        // For ordinary aqrl instructions, address wrapping is not required
        let operands_part = if operands_segment.trim().is_empty() {
            String::new()
        } else {
            format!(" {}", operands_segment)
        };

        let escaped_operands = operands_part.replace('\\', "\\\\").replace('"', "\\\"");

        match (has_aq, has_rl) {
            (true, true) => format!(
                r#"{{
    let suffix = if *aq && *rl {{
        ".aqrl"
    }} else if *aq {{
        ".aq"
    }} else if *rl {{
        ".rl"
    }} else {{
        ""
    }};
    format!("{}{{suffix}}{}", )
}}"#,
                name, escaped_operands
            ),
            (true, false) => format!(
                r#"{{
    let suffix = if *aq {{
        ".aq"
    }} else {{
        ""
    }};
    format!("{}{{suffix}}{}", )
}}"#,
                name, escaped_operands
            ),
            (false, true) => format!(
                r#"{{
    let suffix = if *rl {{
        ".rl"
    }} else {{
        ""
    }};
    format!("{}{{suffix}}{}", )
}}"#,
                name, escaped_operands
            ),
            (false, false) => format!(r#"format!("{}{}", )"#, name, escaped_operands),
        }
    }
}

/// Check whether this is a MOP instruction
fn is_mop_instruction(instruction: &Instruction) -> bool {
    (instruction.name.starts_with("mop.") || instruction.name.starts_with("c.mop"))
        && instruction.operands.iter().any(|o| o.name == "n")
}

/// Generate assembly for MOP instructions
fn generate_mop_assembly_code(
    name: &str,
    operands_segment: &str,
    _instruction: &Instruction,
) -> String {
    let operands_part = if operands_segment.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", operands_segment)
    };

    let escaped_operands = operands_part.replace('\\', "\\\\").replace('"', "\\\"");

    // Extract the instruction name prefix (drop trailing .n)
    let name_prefix = if name.ends_with(".n") {
        &name[..name.len() - 2]
    } else {
        name
    };

    format!(r#"format!("{}.{{}}{}", n)"#, name_prefix, escaped_operands)
}

/// Check whether the vector instruction includes a vm operand
fn is_vector_instruction_with_vm(instruction: &Instruction) -> bool {
    instruction.name.starts_with('v') && instruction.operands.iter().any(|o| o.name == "vm")
}

/// Generate assembly for vector instructions containing a vm operand
fn generate_vector_vm_assembly_code(
    name: &str,
    operands_segment: &str,
    _instruction: &Instruction,
) -> String {
    // Remove the {vm} part from the operand segment
    let operands_without_vm = operands_segment
        .replace(", {vm}", "")
        .replace("{vm}, ", "")
        .replace("{vm}", "");

    let operands_part = if operands_without_vm.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", operands_without_vm)
    };

    let escaped_operands = operands_part.replace('\\', "\\\\").replace('"', "\\\"");

    format!(
        r#"{{
    let vm_suffix = if *vm {{
        ""
    }} else {{
        ", v0.t"
    }};
    format!("{}{}{{vm_suffix}}")
}}"#,
        name, escaped_operands
    )
}

/// Assign the correct operand_type for each operand of an instruction
fn assign_operand_types(instruction: &mut Instruction) {
    for operand in instruction.operands.iter_mut() {
        if operand.operand_type.is_none() {
            operand.operand_type = Some(determine_operand_type(&operand.name));
        }
    }
}

/// Determine operand type based on its name
fn determine_operand_type(operand_name: &str) -> OperandType {
    match operand_name {
        // 1. Explicit register types
        "xd" | "xs1" | "xs2" | "rd" | "rs1" | "rs2" | "rs3" => OperandType::IntegerRegister,
        "fd" | "fs1" | "fs2" | "fs3" | "qd" | "qs1" | "qs2" | "qs3" | "hd" | "dd" => {
            OperandType::FloatingPointRegister
        }
        "vd" | "vs1" | "vs2" | "vs3" => OperandType::VectorRegister,

        // 2. Newly added special formatting types
        "csr" => OperandType::CSRAddress,
        "rm" | "frm" => OperandType::RoundMode,

        // 3. Explicit immediate types
        // Unsigned integer immediates (excluding "csr", "rm", "frm")
        "uimm" | "shamt" | "n" | "rnum" | "fm" | "vtypei" | "vm" | "aq" | "rl" | "bs" => {
            OperandType::UnsignedInteger
        }

        // Signed integer immediates
        "imm" | "imm12" | "offset" => OperandType::SignedInteger,

        // FenceMode
        "pred" | "succ" => OperandType::FenceMode,

        "r1s" | "r2s" => OperandType::SavedIntegerRegister,
        "ne_r1s_r2s" => OperandType::NotEqualCompressedSavedIntegerRegisterPair, // Newly added

        // 4. Fallback inference based on naming pattern
        _ => {
            if operand_name.starts_with('x') {
                OperandType::IntegerRegister
            } else if operand_name.starts_with('f')
                || operand_name.starts_with('q')
                || operand_name.starts_with('h')
            {
                OperandType::FloatingPointRegister
            } else if operand_name.starts_with('v') {
                OperandType::VectorRegister
            } else if operand_name.contains("imm") || operand_name.contains("offset") {
                OperandType::SignedInteger
            } else {
                // Default fallback to unsigned integer
                OperandType::UnsignedInteger
            }
        }
    }
}

/// Fix address syntax for hypervisor load/store instructions
fn fix_hypervisor_load_store_instructions(
    operands_part: &mut String,
    instruction: &mut Instruction,
) {
    match instruction.name.as_str() {
        // H-Load instructions: convert "{xd}, {xs1}" to "{xd}, 0({xs1})"
        "hlv.b" | "hlv.bu" | "hlv.d" | "hlv.h" | "hlv.hu" | "hlv.w" | "hlv.wu" | "hlvx.hu"
        | "hlvx.wu" => {
            *operands_part = "xd, 0(xs1)".to_string();
        }
        // H-Store instructions: convert "{xs1}, {xs2}" to "{xs2}, 0({xs1})"
        "hsv.b" | "hsv.d" | "hsv.h" | "hsv.w" => {
            *operands_part = "xs2, 0(xs1)".to_string();
        }
        _ => {}
    }
}

/// Fix instructions with incorrect ISABase
fn fix_isabase_error_instructions(instruction: &mut Instruction) {
    // RV64 only
    match instruction.name.as_str() {
        "ssamoswap.d" => {
            instruction.isa_bases = vec![ISABase::RV64];
            instruction.operands.iter_mut().for_each(|op| {
                op.bit_lengths.remove(&ISABase::RV32);
            });
        }
        _ => {}
    }

    // RV32 only
    match instruction.name.as_str() {
        "ssamoswap.w" => {
            instruction.isa_bases = vec![ISABase::RV32];
            instruction.operands.iter_mut().for_each(|op| {
                op.bit_lengths.remove(&ISABase::RV64);
            });
        }
        _ => {}
    }
}

/// Fix the rnum operand range for aes64ks1i
fn fix_aes64ks1i_rnum_range(instruction: &mut Instruction) {
    if instruction.name != "aes64ks1i" {
        return;
    }

    // Add range constraint [0, 10] for rnum
    instruction
        .operands
        .iter_mut()
        .find(|op| op.name == "rnum")
        .map(|op| {
            op.restrictions = Some(OperandRestriction {
                multiple_of: None,
                min_max: Some((0, 10)),
                forbidden_values: vec![],
                odd_only: None,
            });
        });
}

/// Generate assembly for compressed instruction offsets
fn generate_compressed_offset_assembly_code(
    name: &str,
    operands_segment: &str,
    _instruction: &Instruction,
) -> String {
    // Remove braces to obtain raw operands
    let operands_without_braces = operands_segment.replace("{", "").replace("}", "");

    let operands: Vec<&str> = operands_without_braces
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    match name {
        "c.sh" => {
            // c.sh {xs2}, {uimm}({xs1}) -> c.sh {xs2}, offset({xs1})
            if operands.len() >= 3 {
                let xs2 = operands[0];
                let xs1 = operands[2]; // last operand is the base register
                format!(
                    r#"{{
    let offset = if *uimm {{
        "2"
    }} else {{
        "0"
    }};
    format!("{name} {{{xs2}}}, {{offset}}({{{xs1}}})")
}}"#
                )
            } else {
                format!(r#"format!("{name} {{xs2}}, 0({{xs1}})")"#)
            }
        }
        "c.lh" | "c.lhu" => {
            // c.lh {xd}, {uimm}({xs1}) -> c.lh {xd}, offset({xs1})
            if operands.len() >= 3 {
                let xd = operands[0];
                let xs1 = operands[2]; // last operand is the base register
                format!(
                    r#"{{
    let offset = if *uimm {{
        "2"
    }} else {{
        "0"
    }};
    format!("{name} {{{xd}}}, {{offset}}({{{xs1}}})")
}}"#
                )
            } else {
                format!(r#"format!("{name} {{xd}}, 0({{xs1}})")"#)
            }
        }
        _ => unreachable!(),
    }
}

/// Fix merging of reg_list and stack_adj for cm.* instructions
fn fix_cm_instructions_reg_list_stack_adj(
    instruction: &mut Instruction,
    operands_part: &mut String,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if !matches!(
        instruction.name.as_str(),
        "cm.pop" | "cm.push" | "cm.popretz" | "cm.popret"
    ) {
        return;
    }

    // Check whether both reg_list and stack_adj exist
    let has_reg_list = instruction.operands.iter().any(|op| op.name == "reg_list");
    let has_stack_adj = instruction.operands.iter().any(|op| op.name == "stack_adj");

    if has_reg_list && has_stack_adj {
        // Retrieve original operand bit lengths and restrictions
        let reg_list_operand = instruction
            .operands
            .iter()
            .find(|op| op.name == "reg_list")
            .cloned();
        let stack_adj_operand = instruction
            .operands
            .iter()
            .find(|op| op.name == "stack_adj")
            .cloned();

        // Create the merged operand
        if let (Some(reg_list), Some(stack_adj)) = (reg_list_operand, stack_adj_operand) {
            let mut combined_bit_lengths = HashMap::new();
            // Merge bit lengths: reg_list (4 bits) + stack_adj (2 bits) = 6 bits
            for (&isa_base, &reg_bits) in &reg_list.bit_lengths {
                if let Some(&stack_bits) = stack_adj.bit_lengths.get(&isa_base) {
                    combined_bit_lengths.insert(isa_base, reg_bits + stack_bits);
                }
            }

            let combined_operand = Operand {
                name: "saved_reg_list_with_stack_adj".to_string(),
                operand_type: Some(OperandType::SavedRegListWithStackAdj),
                bit_lengths: combined_bit_lengths,
                restrictions: None, // Composite operand restrictions handled internally
            };

            // Replace operand list
            instruction.operands = vec![combined_operand];

            // Update syntax operand set
            final_syntax_operands.clear();
            final_syntax_operands.insert("saved_reg_list_with_stack_adj".to_string());

            // Clear operand segment; Rust code will render it
            *operands_part = String::new();
        }
    }
}

/// Generate assembly for cm.* instructions
fn generate_cm_instruction_assembly_code(name: &str) -> String {
    match name {
        "cm.pop" => r#"{
    let reg_list_str = saved_reg_list_with_stack_adj.get_saved_reg_list_string();
    let stack_adj = saved_reg_list_with_stack_adj.get_stack_adjustment();
    format!("cm.pop {}, {}", reg_list_str, stack_adj)
}"#
        .to_string(),
        "cm.push" => r#"{
    let reg_list_str = saved_reg_list_with_stack_adj.get_saved_reg_list_string();
    let stack_adj = saved_reg_list_with_stack_adj.get_stack_adjustment();
    format!("cm.push {}, -{}", reg_list_str, stack_adj)
}"#
        .to_string(),
        "cm.popretz" => r#"{
    let reg_list_str = saved_reg_list_with_stack_adj.get_saved_reg_list_string();
    let stack_adj = saved_reg_list_with_stack_adj.get_stack_adjustment();
    format!("cm.popretz {}, {}", reg_list_str, stack_adj)
}"#
        .to_string(),
        "cm.popret" => r#"{
    let reg_list_str = saved_reg_list_with_stack_adj.get_saved_reg_list_string();
    let stack_adj = saved_reg_list_with_stack_adj.get_stack_adjustment();
    format!("cm.popret {}, {}", reg_list_str, stack_adj)
}"#
        .to_string(),
        _ => unreachable!(),
    }
}

/// Fix the operand merge of r1s and r2s for cm.mvsa01
fn fix_cm_mvsa01_instruction(
    instruction: &mut Instruction,
    operands_part: &mut String,
    final_syntax_operands: &mut std::collections::HashSet<String>,
) {
    if instruction.name != "cm.mvsa01" {
        return;
    }

    let has_r1s = instruction.operands.iter().any(|op| op.name == "r1s");
    let has_r2s = instruction.operands.iter().any(|op| op.name == "r2s");

    if has_r1s && has_r2s {
        // Assume r1s and r2s are both 3 bits
        let mut combined_bit_lengths = HashMap::new();
        // Look up original bit-length information for r1s and r2s
        let r1s_op = instruction.operands.iter().find(|op| op.name == "r1s");
        let r2s_op = instruction.operands.iter().find(|op| op.name == "r2s");

        if let (Some(op1), Some(op2)) = (r1s_op, r2s_op) {
            for (base, len1) in &op1.bit_lengths {
                if let Some(len2) = op2.bit_lengths.get(base) {
                    combined_bit_lengths.insert(*base, len1 + len2); // merge lengths
                }
            }
        } else {
            // If originals aren't found, fall back to defaults (simplified handling)
            combined_bit_lengths.insert(ISABase::RV32, 6);
            combined_bit_lengths.insert(ISABase::RV64, 6);
        }

        let merged_operand = Operand {
            name: "ne_r1s_r2s".to_string(),
            operand_type: Some(OperandType::NotEqualCompressedSavedIntegerRegisterPair),
            bit_lengths: combined_bit_lengths,
            restrictions: None, // "Not equal" constraint handled by the type itself
        };

        // Remove old operands and add the merged one
        instruction
            .operands
            .retain(|op| op.name != "r1s" && op.name != "r2s");
        instruction.operands.push(merged_operand);

        // Update assembly syntax; assume original was "r1s, r2s" or similar
        // Simplified replacement—other formats may need more handling
        if operands_part.contains("r1s") && operands_part.contains("r2s") {
            *operands_part = operands_part
                .replace("r1s, r2s", "ne_r1s_r2s")
                .replace("r1s,r2s", "ne_r1s_r2s"); // handle no-space case
        } else {
            // If the original format is unknown, simply set the new operand name
            *operands_part = "ne_r1s_r2s".to_string();
        }

        // Update final syntax operand set
        final_syntax_operands.remove("r1s");
        final_syntax_operands.remove("r2s");
        final_syntax_operands.insert("ne_r1s_r2s".to_string());
    }
}
