use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs::{self, File},
    path::Path,
};

use crate::{
    execution_output::{ExceptionInfo, ExecutionContextOutput, InstructionContext},
    isa_base::ISABase,
    sd_instruction_cluster::{
        InstructionCluster, classify_mnemonic, is_div_rem_mnemonic, is_signed_div_rem_mnemonic,
        memory_access_width,
    },
    utils::{MemoryValueWidth, extract_registers_from_instruction},
};

static MEMORY_OPERAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?P<imm>-?(?:0[xX][0-9a-fA-F]+|\d+))?\s*\(\s*(?P<base>x\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp)\s*\)",
    )
    .expect("memory operand regex compiles")
});

/// SDModel state for one retired instruction: the opcode plus the
/// data-sensitive predicates satisfied by the instruction context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ExecutionState {
    pub opcode: String,
    pub predicates: BTreeSet<String>,
}

/// Transition-guided fuzzing key: consecutive SDModel execution states.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: ExecutionState,
    pub to: ExecutionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionSummary {
    pub implementation: String,
    pub total_states: usize,
    pub total_transitions: usize,
    pub unique_transitions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionAnalysis {
    pub implementation: String,
    pub states: Vec<ExecutionState>,
    pub transitions: Vec<StateTransition>,
    pub summary: TransitionSummary,
}

pub fn analyze_transitions(
    output: &ExecutionContextOutput,
    instructions: &[String],
) -> TransitionAnalysis {
    let implementation = output.riscv_impl.to_string();
    let states = label_execution_states(output, instructions);
    let transitions = extract_transitions(&states);
    let unique_transitions = unique_counted_transitions(&transitions).len();
    let summary = TransitionSummary {
        implementation: implementation.clone(),
        total_states: states.len(),
        total_transitions: transitions.len(),
        unique_transitions,
    };

    TransitionAnalysis {
        implementation,
        states,
        transitions,
        summary,
    }
}

pub fn label_execution_states(
    output: &ExecutionContextOutput,
    instructions: &[String],
) -> Vec<ExecutionState> {
    let mut exceptions_by_index: BTreeMap<usize, Vec<&ExceptionInfo>> = BTreeMap::new();
    for exception in &output.exceptions {
        exceptions_by_index
            .entry(exception.user_instruction_index)
            .or_default()
            .push(exception);
    }

    let limit = output.contexts.len().min(instructions.len());
    let mut states = Vec::with_capacity(limit);
    for idx in 0..limit {
        let instruction = &instructions[idx];
        let opcode = instruction_opcode(instruction);
        let cluster = classify_mnemonic(&opcode);
        let context = &output.contexts[idx];
        let mut predicates = BTreeSet::new();

        label_register_boundaries(&mut predicates, cluster, &opcode, context, output.isa_base);
        if labels_immediate_boundaries(cluster) {
            label_immediate_boundaries(&mut predicates, instruction, output.isa_base);
        }
        if cluster.is_memory() {
            label_memory_boundaries(&mut predicates, context, output.mem_range);
        }
        label_instruction_triggers(
            &mut predicates,
            cluster,
            &opcode,
            instruction,
            context,
            output,
        );
        if let Some(exceptions) = exceptions_by_index.get(&idx) {
            label_exceptions(&mut predicates, &opcode, exceptions);
        }

        states.push(ExecutionState { opcode, predicates });
    }

    states
}

pub fn extract_transitions(states: &[ExecutionState]) -> Vec<StateTransition> {
    states
        .windows(2)
        .map(|window| StateTransition {
            from: window[0].clone(),
            to: window[1].clone(),
        })
        .collect()
}

pub fn unique_counted_transitions(transitions: &[StateTransition]) -> BTreeSet<StateTransition> {
    transitions
        .iter()
        .filter(|transition| counted_transition(transition))
        .cloned()
        .collect()
}

pub fn unique_counted_modes(states: &[ExecutionState]) -> BTreeSet<ExecutionState> {
    states
        .iter()
        .filter(|state| counted_mode(state))
        .cloned()
        .collect()
}

pub fn counted_transition(transition: &StateTransition) -> bool {
    if transition.from.opcode == "nop" || transition.to.opcode == "nop" {
        return false;
    }
    !transition.from.predicates.is_empty() || !transition.to.predicates.is_empty()
}

pub fn counted_mode(state: &ExecutionState) -> bool {
    state.opcode != "nop" && !state.predicates.is_empty()
}

pub fn write_transition_report_json(
    path: &Path,
    analysis: &TransitionAnalysis,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, analysis)?;
    Ok(())
}

pub fn write_transition_report_md(
    path: &Path,
    analysis: &TransitionAnalysis,
    instructions: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = String::new();
    writeln!(&mut content, "# SDModel Transition Report\n")?;
    writeln!(
        &mut content,
        "| Item | Value |\n| --- | --- |\n| Reference implementation | {} |\n| States | {} |\n| Transitions | {} |\n| Unique transitions | {} |\n",
        analysis.summary.implementation,
        analysis.summary.total_states,
        analysis.summary.total_transitions,
        analysis.summary.unique_transitions
    )?;

    writeln!(&mut content, "## Instruction States\n")?;
    writeln!(
        &mut content,
        "| Index | Opcode | Predicates | Instruction |\n| --- | --- | --- | --- |"
    )?;
    for (idx, state) in analysis.states.iter().enumerate() {
        let predicates = join_predicates(&state.predicates);
        let instruction = instructions
            .get(idx)
            .map(|line| escape_markdown_cell(line))
            .unwrap_or_else(|| "-".to_string());
        writeln!(
            &mut content,
            "| {} | `{}` | {} | `{}` |",
            idx,
            escape_markdown_cell(&state.opcode),
            predicates,
            instruction
        )?;
    }

    writeln!(&mut content, "\n## Transitions\n")?;
    writeln!(
        &mut content,
        "| Index | From | To | From Predicates | To Predicates |\n| --- | --- | --- | --- | --- |"
    )?;
    for (idx, transition) in analysis.transitions.iter().enumerate() {
        writeln!(
            &mut content,
            "| {} | `{}` | `{}` | {} | {} |",
            idx,
            escape_markdown_cell(&transition.from.opcode),
            escape_markdown_cell(&transition.to.opcode),
            join_predicates(&transition.from.predicates),
            join_predicates(&transition.to.predicates)
        )?;
    }

    fs::write(path, content)?;
    Ok(())
}

fn label_register_boundaries(
    predicates: &mut BTreeSet<String>,
    cluster: InstructionCluster,
    opcode: &str,
    context: &InstructionContext,
    isa_base: ISABase,
) {
    let label_integer = labels_integer_register_boundaries(cluster);
    let label_float = labels_float_register_boundaries(cluster);
    for entry in &context.register_context.entries {
        if label_integer && entry.name.starts_with('x') {
            label_integer_boundary(predicates, entry.value, isa_base);
        } else if label_float && entry.name.starts_with('f') {
            label_float_boundary(predicates, opcode, entry.value);
        }
    }
}

fn labels_integer_register_boundaries(cluster: InstructionCluster) -> bool {
    matches!(
        cluster,
        InstructionCluster::Load
            | InstructionCluster::Store
            | InstructionCluster::Atomic
            | InstructionCluster::UpperImmediate
            | InstructionCluster::IntegerArithmetic
            | InstructionCluster::IntegerCompare
            | InstructionCluster::IntegerShift
            | InstructionCluster::IntegerLogic
            | InstructionCluster::IntegerBitmanip
            | InstructionCluster::IntegerMultiply
            | InstructionCluster::IntegerDivideRemainder
            | InstructionCluster::Crypto
            | InstructionCluster::FloatingLoad
            | InstructionCluster::FloatingStore
            | InstructionCluster::FloatingConvert
            | InstructionCluster::FloatingMove
            | InstructionCluster::CompressedStack
    )
}

fn labels_float_register_boundaries(cluster: InstructionCluster) -> bool {
    matches!(
        cluster,
        InstructionCluster::FloatingLoad
            | InstructionCluster::FloatingStore
            | InstructionCluster::FloatingArithmetic
            | InstructionCluster::FloatingCompare
            | InstructionCluster::FloatingConvert
            | InstructionCluster::FloatingMove
            | InstructionCluster::FloatingClass
    )
}

fn labels_immediate_boundaries(cluster: InstructionCluster) -> bool {
    matches!(
        cluster,
        InstructionCluster::Load
            | InstructionCluster::Store
            | InstructionCluster::Atomic
            | InstructionCluster::UpperImmediate
            | InstructionCluster::IntegerArithmetic
            | InstructionCluster::IntegerCompare
            | InstructionCluster::IntegerShift
            | InstructionCluster::IntegerLogic
            | InstructionCluster::IntegerBitmanip
            | InstructionCluster::IntegerMultiply
            | InstructionCluster::IntegerDivideRemainder
            | InstructionCluster::Crypto
            | InstructionCluster::FloatingLoad
            | InstructionCluster::FloatingStore
            | InstructionCluster::FloatingConvert
            | InstructionCluster::FloatingMove
            | InstructionCluster::CompressedStack
    )
}

fn label_memory_boundaries(
    predicates: &mut BTreeSet<String>,
    context: &InstructionContext,
    mem_range: (u64, u64),
) {
    if mem_range.1 < mem_range.0 {
        return;
    }
    let max_offset = mem_range.1 - mem_range.0;
    for entry in &context.memory_context.entries {
        if entry
            .addresses
            .iter()
            .any(|addr| *addr == 0 || *addr == max_offset)
        {
            predicates.insert("boundary_value:memory-edge".to_string());
        }
        label_integer_boundary(predicates, entry.value, ISABase::Rv64);
    }
}

fn label_immediate_boundaries(
    predicates: &mut BTreeSet<String>,
    instruction: &str,
    isa_base: ISABase,
) {
    for immediate in integer_immediates(instruction) {
        label_integer_boundary(predicates, immediate as u64, isa_base);
    }
}

fn label_instruction_triggers(
    predicates: &mut BTreeSet<String>,
    cluster: InstructionCluster,
    opcode: &str,
    instruction: &str,
    context: &InstructionContext,
    output: &ExecutionContextOutput,
) {
    if let Some(width) = memory_access_width(opcode) {
        if is_misaligned_memory_access(instruction, context, output.mem_range, width) {
            predicates.insert("exception_trigger:misaligned-memory".to_string());
        }
    }

    if is_div_rem_mnemonic(opcode) {
        label_division_triggers(predicates, opcode, instruction, context, output.isa_base);
    }

    if matches!(
        cluster,
        InstructionCluster::FloatingArithmetic | InstructionCluster::FloatingConvert
    ) {
        label_float_operation_triggers(predicates, opcode, instruction, context);
    }

    match cluster {
        InstructionCluster::Csr => {
            predicates.insert("exception_trigger:csr-access".to_string());
        }
        InstructionCluster::System => {
            predicates.insert("exception_trigger:system-or-privileged".to_string());
        }
        InstructionCluster::Fence => {
            predicates.insert("exception_trigger:ordering-cache-or-privileged".to_string());
        }
        InstructionCluster::FloatingConstant => {
            predicates.insert("boundary_value:fp-literal-constant".to_string());
        }
        _ => {}
    }
}

fn label_exceptions(
    predicates: &mut BTreeSet<String>,
    opcode: &str,
    exceptions: &[&ExceptionInfo],
) {
    for exception in exceptions {
        predicates.insert("exception_trigger:trap".to_string());
        let cause = normalize_predicate_token(&exception.cause);
        if !cause.is_empty() {
            predicates.insert(format!("exception_trigger:cause:{cause}"));
        }

        let lower = exception.cause.to_ascii_lowercase();
        if lower.contains("misalign") {
            predicates.insert("exception_trigger:misaligned-memory".to_string());
        }
        if lower.contains("illegal") || lower.contains("invalid") || opcode.starts_with("csr") {
            predicates.insert("exception_trigger:illegal-or-csr".to_string());
        }
    }
}

fn label_integer_boundary(predicates: &mut BTreeSet<String>, value: u64, isa_base: ISABase) {
    let mask = match isa_base {
        ISABase::Rv32 => u32::MAX as u64,
        ISABase::Rv64 => u64::MAX,
    };
    let masked = value & mask;
    let (signed_min, signed_max, minus_one) = match isa_base {
        ISABase::Rv32 => (i32::MIN as u64 & mask, i32::MAX as u64, u32::MAX as u64),
        ISABase::Rv64 => (i64::MIN as u64, i64::MAX as u64, u64::MAX),
    };

    if masked == 0 {
        predicates.insert("boundary_value:int-zero".to_string());
    }
    if masked == 1 {
        predicates.insert("boundary_value:int-one".to_string());
    }
    if masked == minus_one {
        predicates.insert("boundary_value:int-minus-one".to_string());
    }
    if masked == signed_min {
        predicates.insert("boundary_value:int-signed-min".to_string());
    }
    if masked == signed_max {
        predicates.insert("boundary_value:int-signed-max".to_string());
    }

    let bit_width = match isa_base {
        ISABase::Rv32 => 32,
        ISABase::Rv64 => 64,
    };
    let alternating_a = alternating_mask(bit_width, true);
    let alternating_b = alternating_mask(bit_width, false);
    if masked == alternating_a
        || masked == alternating_b
        || masked.count_ones() == 1
        || ((!masked) & mask).count_ones() == 1
    {
        predicates.insert("boundary_value:int-bitmask".to_string());
    }
}

fn label_float_boundary(predicates: &mut BTreeSet<String>, opcode: &str, value: u64) {
    if opcode.ends_with(".s") || opcode.contains(".s.") {
        label_float32_boundary(predicates, value as u32);
    } else if opcode.ends_with(".d") || opcode.contains(".d.") {
        label_float64_boundary(predicates, value);
    } else {
        label_float32_boundary(predicates, value as u32);
        label_float64_boundary(predicates, value);
    }
}

fn label_float32_boundary(predicates: &mut BTreeSet<String>, bits: u32) {
    let exp = (bits >> 23) & 0xff;
    let frac = bits & 0x7f_ffff;
    if bits & 0x7fff_ffff == 0 {
        predicates.insert("boundary_value:fp-zero".to_string());
    } else if exp == 0xff && frac == 0 {
        predicates.insert("boundary_value:fp-infinity".to_string());
    } else if exp == 0xff {
        if frac & (1 << 22) != 0 {
            predicates.insert("boundary_value:fp-qnan".to_string());
        } else {
            predicates.insert("boundary_value:fp-snan".to_string());
        }
    } else if exp == 0 && frac != 0 {
        predicates.insert("boundary_value:fp-subnormal".to_string());
    }
}

fn label_float64_boundary(predicates: &mut BTreeSet<String>, bits: u64) {
    let exp = (bits >> 52) & 0x7ff;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if bits & 0x7fff_ffff_ffff_ffff == 0 {
        predicates.insert("boundary_value:fp-zero".to_string());
    } else if exp == 0x7ff && frac == 0 {
        predicates.insert("boundary_value:fp-infinity".to_string());
    } else if exp == 0x7ff {
        if frac & (1 << 51) != 0 {
            predicates.insert("boundary_value:fp-qnan".to_string());
        } else {
            predicates.insert("boundary_value:fp-snan".to_string());
        }
    } else if exp == 0 && frac != 0 {
        predicates.insert("boundary_value:fp-subnormal".to_string());
    }
}

fn label_division_triggers(
    predicates: &mut BTreeSet<String>,
    opcode: &str,
    instruction: &str,
    context: &InstructionContext,
    isa_base: ISABase,
) {
    let Ok(registers) = extract_registers_from_instruction(instruction) else {
        return;
    };
    if registers.len() < 3 {
        return;
    }

    let Some(rs1) = register_value(context, &registers[1]) else {
        return;
    };
    let Some(rs2) = register_value(context, &registers[2]) else {
        return;
    };

    if rs2 == 0 {
        predicates.insert("exception_trigger:divide-by-zero".to_string());
    }

    if is_signed_div_rem_mnemonic(opcode)
        && signed_value_is_min(rs1, isa_base)
        && is_minus_one(rs2, isa_base)
    {
        predicates.insert("exception_trigger:signed-div-overflow".to_string());
        predicates.insert("boundary_value:int-signed-min".to_string());
        predicates.insert("boundary_value:int-minus-one".to_string());
    }
}

fn label_float_operation_triggers(
    predicates: &mut BTreeSet<String>,
    opcode: &str,
    instruction: &str,
    context: &InstructionContext,
) {
    let Ok(registers) = extract_registers_from_instruction(instruction) else {
        return;
    };
    let fp_regs = registers
        .into_iter()
        .filter(|name| name.starts_with('f'))
        .collect::<Vec<_>>();

    if opcode.starts_with("fdiv.") && fp_regs.len() >= 3 {
        if let Some(bits) = register_value(context, &fp_regs[2]) {
            if float_bits_are_zero(opcode, bits) {
                predicates.insert("exception_trigger:fp-divide-by-zero".to_string());
            }
        }
    }

    if opcode.starts_with("fsqrt.") && fp_regs.len() >= 2 {
        if let Some(bits) = register_value(context, &fp_regs[1]) {
            if float_bits_are_negative_nonzero_finite(opcode, bits) {
                predicates.insert("exception_trigger:fp-invalid-sqrt-negative".to_string());
            }
        }
    }
}

fn register_value(context: &InstructionContext, name: &str) -> Option<u64> {
    context
        .register_context
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.value)
        .or_else(|| context.registers_before.get(name).copied())
}

fn is_misaligned_memory_access(
    instruction: &str,
    context: &InstructionContext,
    mem_range: (u64, u64),
    width: u64,
) -> bool {
    if width <= 1 {
        return false;
    }

    for entry in &context.memory_context.entries {
        if memory_width_bytes(&entry.width) == width {
            if let Some(addr) = entry.addresses.first() {
                return mem_range.0.wrapping_add(*addr) % width != 0;
            }
        }
    }

    let Some((base, offset)) = parse_memory_operand(instruction) else {
        return false;
    };
    let Some(base_value) = register_value(context, &base) else {
        return false;
    };
    let effective = if offset >= 0 {
        base_value.wrapping_add(offset as u64)
    } else {
        base_value.wrapping_sub((-offset) as u64)
    };
    effective % width != 0
}

fn parse_memory_operand(instruction: &str) -> Option<(String, i64)> {
    let caps = MEMORY_OPERAND_RE.captures(instruction)?;
    let base = normalize_register_name(caps.name("base")?.as_str())?;
    let offset = caps
        .name("imm")
        .and_then(|m| parse_integer_literal(m.as_str()))
        .unwrap_or(0);
    Some((base, offset))
}

fn instruction_opcode(instruction: &str) -> String {
    let stripped = strip_inline_comment(instruction);
    stripped
        .split_whitespace()
        .next()
        .map(|token| token.trim_end_matches(',').to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

fn strip_inline_comment(line: &str) -> &str {
    let mut end = line.len();
    for marker in ["#", ";", "//"] {
        if let Some(pos) = line.find(marker) {
            end = end.min(pos);
        }
    }
    &line[..end]
}

fn memory_width_bytes(width: &MemoryValueWidth) -> u64 {
    match width {
        MemoryValueWidth::Byte => 1,
        MemoryValueWidth::Half => 2,
        MemoryValueWidth::Word => 4,
        MemoryValueWidth::Dword => 8,
    }
}

fn integer_immediates(instruction: &str) -> Vec<i64> {
    let operands = strip_inline_comment(instruction)
        .split_once(char::is_whitespace)
        .map(|(_, operands)| operands)
        .unwrap_or("");

    operands
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '(' | ')'))
        .filter_map(|token| {
            let token = token.trim();
            if token.is_empty()
                || token.starts_with('x')
                || token.starts_with('f')
                || normalize_register_name(token).is_some()
            {
                return None;
            }
            parse_integer_literal(token)
        })
        .collect()
}

fn float_bits_are_zero(opcode: &str, bits: u64) -> bool {
    if opcode.ends_with(".s") {
        ((bits as u32) & 0x7fff_ffff) == 0
    } else if opcode.ends_with(".h") {
        ((bits as u16) & 0x7fff) == 0
    } else {
        bits & 0x7fff_ffff_ffff_ffff == 0
    }
}

fn float_bits_are_negative_nonzero_finite(opcode: &str, bits: u64) -> bool {
    if opcode.ends_with(".s") {
        let bits = bits as u32;
        let sign = bits >> 31 != 0;
        let exp = (bits >> 23) & 0xff;
        let frac = bits & 0x7f_ffff;
        sign && exp != 0xff && (exp != 0 || frac != 0)
    } else if opcode.ends_with(".h") {
        let bits = bits as u16;
        let sign = bits >> 15 != 0;
        let exp = (bits >> 10) & 0x1f;
        let frac = bits & 0x03ff;
        sign && exp != 0x1f && (exp != 0 || frac != 0)
    } else {
        let sign = bits >> 63 != 0;
        let exp = (bits >> 52) & 0x7ff;
        let frac = bits & 0x000f_ffff_ffff_ffff;
        sign && exp != 0x7ff && (exp != 0 || frac != 0)
    }
}

fn signed_value_is_min(value: u64, isa_base: ISABase) -> bool {
    match isa_base {
        ISABase::Rv32 => (value & u32::MAX as u64) == ((i32::MIN as u64) & u32::MAX as u64),
        ISABase::Rv64 => value == i64::MIN as u64,
    }
}

fn is_minus_one(value: u64, isa_base: ISABase) -> bool {
    match isa_base {
        ISABase::Rv32 => (value & u32::MAX as u64) == u32::MAX as u64,
        ISABase::Rv64 => value == u64::MAX,
    }
}

fn alternating_mask(bit_width: u32, starts_with_one: bool) -> u64 {
    let mut value = 0u64;
    for bit in 0..bit_width {
        let set = if starts_with_one {
            bit % 2 == 0
        } else {
            bit % 2 == 1
        };
        if set {
            value |= 1u64 << bit;
        }
    }
    value
}

fn normalize_register_name(name: &str) -> Option<String> {
    let canonical = match name {
        "zero" => "x0",
        "ra" => "x1",
        "sp" => "x2",
        "gp" => "x3",
        "tp" => "x4",
        "t0" => "x5",
        "t1" => "x6",
        "t2" => "x7",
        "fp" | "s0" => "x8",
        "s1" => "x9",
        "a0" => "x10",
        "a1" => "x11",
        "a2" => "x12",
        "a3" => "x13",
        "a4" => "x14",
        "a5" => "x15",
        "a6" => "x16",
        "a7" => "x17",
        "s2" => "x18",
        "s3" => "x19",
        "s4" => "x20",
        "s5" => "x21",
        "s6" => "x22",
        "s7" => "x23",
        "s8" => "x24",
        "s9" => "x25",
        "s10" => "x26",
        "s11" => "x27",
        "t3" => "x28",
        "t4" => "x29",
        "t5" => "x30",
        "t6" => "x31",
        value if value.starts_with('x') => value,
        _ => return None,
    };
    Some(canonical.to_string())
}

fn parse_integer_literal(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("-0x")
        .or_else(|| trimmed.strip_prefix("-0X"))
    {
        return i64::from_str_radix(hex, 16).ok().map(|v| -v);
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16).ok();
    }
    trimmed.parse::<i64>().ok()
}

fn normalize_predicate_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn join_predicates(predicates: &BTreeSet<String>) -> String {
    if predicates.is_empty() {
        "neutral".to_string()
    } else {
        predicates
            .iter()
            .map(|pred| format!("`{}`", escape_markdown_cell(pred)))
            .collect::<Vec<_>>()
            .join("<br>")
    }
}

fn escape_markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        execution_output::InstructionContext,
        riscv_impls::RiscVImpl,
        utils::{RegisterContext, RegisterContextEntry},
    };
    use std::collections::HashMap;

    #[test]
    fn labels_integer_boundary_state() {
        let output = ExecutionContextOutput {
            exceptions: vec![],
            register_changes: vec![vec![]],
            memory_changes: vec![vec![]],
            contexts: vec![InstructionContext {
                registers_before: HashMap::new(),
                memory_before: BTreeMap::new(),
                register_context: RegisterContext {
                    entries: vec![RegisterContextEntry {
                        name: "x2".to_string(),
                        value: u64::MAX,
                    }],
                },
                memory_context: Default::default(),
            }],
            mem_range: (0, 63),
            riscv_impl: RiscVImpl::Spike,
            isa_base: ISABase::Rv64,
        };

        let states = label_execution_states(&output, &["add x1, x2, x3".to_string()]);
        assert!(
            states[0]
                .predicates
                .contains("boundary_value:int-minus-one")
        );
    }

    #[test]
    fn csr_pseudo_ops_do_not_get_register_boundary_labels() {
        let output = output_with_register_context(vec![RegisterContextEntry {
            name: "x2".to_string(),
            value: 0,
        }]);

        let states = label_execution_states(&output, &["fsflags x1, x2".to_string()]);

        assert!(
            states[0]
                .predicates
                .contains("exception_trigger:csr-access")
        );
        assert!(!states[0].predicates.contains("boundary_value:int-zero"));
    }

    #[test]
    fn li_keeps_immediate_boundary_label() {
        let output = output_with_register_context(vec![]);

        let states = label_execution_states(&output, &["li x1, 0".to_string()]);

        assert!(states[0].predicates.contains("boundary_value:int-zero"));
    }

    #[test]
    fn unique_counted_transitions_ignores_neutral_and_nop_edges() {
        let transitions = vec![
            transition(state("addi", &[]), state("lui", &[])),
            transition(state("nop", &[]), state("fcvt.lu.d", &[])),
            transition(
                state("addi", &["boundary_value:int-zero"]),
                state("lui", &[]),
            ),
            transition(
                state("addi", &["boundary_value:int-zero"]),
                state("lui", &[]),
            ),
        ];

        let unique = unique_counted_transitions(&transitions);
        assert_eq!(unique.len(), 1);
        assert!(unique.contains(&transition(
            state("addi", &["boundary_value:int-zero"]),
            state("lui", &[])
        )));
    }

    #[test]
    fn unique_counted_modes_keeps_only_data_sensitive_states() {
        let states = vec![
            state("addi", &[]),
            state("nop", &["boundary_value:int-zero"]),
            state("addi", &["boundary_value:int-zero"]),
            state("addi", &["boundary_value:int-zero"]),
            state("lui", &["boundary_value:int-zero"]),
        ];

        let unique = unique_counted_modes(&states);
        assert_eq!(unique.len(), 2);
        assert!(unique.contains(&state("addi", &["boundary_value:int-zero"])));
        assert!(unique.contains(&state("lui", &["boundary_value:int-zero"])));
    }

    fn transition(from: ExecutionState, to: ExecutionState) -> StateTransition {
        StateTransition { from, to }
    }

    fn output_with_register_context(entries: Vec<RegisterContextEntry>) -> ExecutionContextOutput {
        ExecutionContextOutput {
            exceptions: vec![],
            register_changes: vec![vec![]],
            memory_changes: vec![vec![]],
            contexts: vec![InstructionContext {
                registers_before: HashMap::new(),
                memory_before: BTreeMap::new(),
                register_context: RegisterContext { entries },
                memory_context: Default::default(),
            }],
            mem_range: (0, 63),
            riscv_impl: RiscVImpl::Spike,
            isa_base: ISABase::Rv64,
        }
    }

    fn state(opcode: &str, predicates: &[&str]) -> ExecutionState {
        ExecutionState {
            opcode: opcode.to_string(),
            predicates: predicates.iter().map(|item| item.to_string()).collect(),
        }
    }
}
