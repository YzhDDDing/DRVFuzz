use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;
use riscv_instruction::separated_instructions::RiscvInstruction;
use riscv_instruction_types::{
    FloatingPointRegister, InstructionSequence, IntegerRegister, LoadImmediatePurpose,
    PseudoInstruction, ValidatedValue,
    data_sensitive::{
        FloatBits, FloatFormat, OperandClass, RuleSelectionKind, instruction_rule_for,
    },
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

use crate::{
    isa_base::ISABase,
    sd_instruction_cluster::{
        InstructionCluster, classify_mnemonic, is_div_rem_mnemonic, memory_access_width,
    },
};

static REGISTER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([xf])(\d{1,2})\b").expect("register regex compiles"));
static INTEGER_REGISTER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(x\d{1,2}|zero|ra|sp|gp|tp|fp|t[0-6]|s(?:[0-9]|1[0-1])|a[0-7])\b")
        .expect("integer register regex compiles")
});
static MEMORY_OPERAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?P<imm>-?(?:0[xX][0-9a-fA-F]+|\d+))?\s*\(\s*(?P<base>x\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp)\s*\)",
    )
    .expect("memory operand regex compiles")
});
static ALIAS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(zero|ra|sp|gp|tp|fp|t[0-6]|s(?:[0-9]|1[0-1])|a[0-7])\b")
        .expect("alias regex compiles")
});
static ALIAS_MAP: Lazy<HashMap<&'static str, u8>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert("zero", 0);
    map.insert("ra", 1);
    map.insert("sp", 2);
    map.insert("gp", 3);
    map.insert("tp", 4);
    map.insert("t0", 5);
    map.insert("t1", 6);
    map.insert("t2", 7);
    map.insert("fp", 8);
    map.insert("s0", 8);
    map.insert("s1", 9);
    map.insert("a0", 10);
    map.insert("a1", 11);
    map.insert("a2", 12);
    map.insert("a3", 13);
    map.insert("a4", 14);
    map.insert("a5", 15);
    map.insert("a6", 16);
    map.insert("a7", 17);
    map.insert("s2", 18);
    map.insert("s3", 19);
    map.insert("s4", 20);
    map.insert("s5", 21);
    map.insert("s6", 22);
    map.insert("s7", 23);
    map.insert("s8", 24);
    map.insert("s9", 25);
    map.insert("s10", 26);
    map.insert("s11", 27);
    map.insert("t3", 28);
    map.insert("t4", 29);
    map.insert("t5", 30);
    map.insert("t6", 31);
    map
});

/// Errors that can occur while injecting data-sensitive setup instructions.
#[derive(Debug, Error)]
pub enum DataSensitiveInjectionError {
    #[error("no temporary register was available within range {0:?}")]
    NoAvailableTempRegisters((u8, u8)),
    #[error("invalid register number x{0}")]
    InvalidRegister(u8),
    #[error("float format {0:?} is not supported for payload injection")]
    UnsupportedFloatFormat(FloatFormat),
}

/// Injects data-sensitive rounding modes and operand payloads into sequences.
pub struct DataSensitiveInjector {
    temp_registers: Vec<u8>,
    temp_register_range: (u8, u8),
    probability: f64,
    rule_completion: HashMap<String, HashSet<usize>>,
    isa_base: ISABase,
}

// const DATA_SENSITIVE_COMMENT: &str = "data sensitive";

impl DataSensitiveInjector {
    /// Build an injector that rotates through the default plan.
    pub fn with_default_plan(
        temp_register_range: (u8, u8),
        probability: f64,
    ) -> Result<Self, DataSensitiveInjectionError> {
        Self::new(temp_register_range, probability, ISABase::Rv64)
    }

    /// Build an injector for a concrete ISA base so XLEN-sensitive
    /// boundary values match RV32/RV64 generation.
    pub fn with_isa_plan(
        temp_register_range: (u8, u8),
        probability: f64,
        isa_base: ISABase,
    ) -> Result<Self, DataSensitiveInjectionError> {
        Self::new(temp_register_range, probability, isa_base)
    }

    /// Build an injector from an explicit sweep definition.
    pub fn new(
        temp_register_range: (u8, u8),
        probability: f64,
        isa_base: ISABase,
    ) -> Result<Self, DataSensitiveInjectionError> {
        if temp_register_range.0 > temp_register_range.1 {
            return Err(DataSensitiveInjectionError::NoAvailableTempRegisters(
                temp_register_range,
            ));
        }

        let temp_registers: Vec<u8> = (temp_register_range.0..=temp_register_range.1).collect();
        if temp_registers.is_empty() {
            return Err(DataSensitiveInjectionError::NoAvailableTempRegisters(
                temp_register_range,
            ));
        }

        Ok(Self {
            temp_registers,
            temp_register_range,
            probability: probability.clamp(0.0, 1.0),
            rule_completion: HashMap::new(),
            isa_base,
        })
    }

    /// Apply the next plan entry to the provided instruction sequence when the probability check succeeds.
    pub fn try_apply<R: Rng>(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        rng: &mut R,
    ) -> Result<bool, DataSensitiveInjectionError> {
        if !rng.random_bool(self.probability) {
            return Ok(false);
        }

        if self.try_apply_float_rule(sequence, rng)? {
            return Ok(true);
        }

        Ok(self.try_apply_integer_or_exception_rule(sequence, rng)?)
    }

    fn try_apply_float_rule<R: Rng>(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        rng: &mut R,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let mnemonic = instruction_mnemonic(&sequence.instruction);
        let Some(rule) = instruction_rule_for(&mnemonic) else {
            return Ok(false);
        };

        let completion = self
            .rule_completion
            .entry(rule.name().to_string())
            .or_default();

        let (candidate_idx, candidate) = match rule.choose_candidate_with_coverage(rng, completion)
        {
            Some(candidate) => candidate,
            None => return Ok(false),
        };

        let float_format = match candidate.float_format() {
            Some(format) => format,
            None => return Ok(false),
        };

        let used_gp = collect_gp_register_usage(sequence);
        let mut fp_sources = floating_source_registers(&sequence.instruction);
        fp_sources.dedup();

        if fp_sources.is_empty() {
            return Ok(false);
        }

        if candidate.operand_set().classes.len() != fp_sources.len() {
            return Ok(false);
        }

        let temp_reg = self
            .temp_registers
            .iter()
            .copied()
            .find(|reg| *reg != 0 && !used_gp.contains(reg))
            .ok_or(DataSensitiveInjectionError::NoAvailableTempRegisters(
                self.temp_register_range,
            ))?;

        let tmp = IntegerRegister::new(temp_reg)
            .map_err(|_| DataSensitiveInjectionError::InvalidRegister(temp_reg))?;

        let mut prefix = Vec::new();
        if let Some(rounding) = candidate.choose_rounding_mode(rng) {
            push_data_sensitive_instruction(
                &mut prefix,
                PseudoInstruction::SetRoundingMode { mode: rounding },
            );
        }

        for (&dest, class) in fp_sources.iter().zip(&candidate.operand_set().classes) {
            if should_inject_operand(*class) {
                // If choose_sample_bits_for_format returns Some(bits_value), enter this block.
                // If it returns None (Operand::Preserve), skip the entire block.
                if let Some(bits_value) = class.choose_sample_bits_for_format(rng, float_format) {
                    // Note: we pass the unpacked bits_value (the concrete value) to generate_float_pseudo_instructions
                    for inst in
                        generate_float_pseudo_instructions(tmp, dest, bits_value, float_format)?
                    {
                        push_data_sensitive_instruction(&mut prefix, inst);
                    }
                }
            }
        }
        let existing_pre = std::mem::take(&mut sequence.pre_instructions);
        sequence.pre_instructions = prefix;
        sequence.pre_instructions.extend(existing_pre);
        sequence.post_instructions.insert(
            0,
            PseudoInstruction::Comment("Post data-sensitive cleanup".to_string()),
        );
        if candidate.selection_kind() == RuleSelectionKind::CoverOnce {
            completion.insert(candidate_idx);
        }

        Ok(true)
    }

    fn try_apply_integer_or_exception_rule<R: Rng>(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        rng: &mut R,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let line = sequence.instruction.to_string();
        let mnemonic = instruction_mnemonic(&sequence.instruction);
        let cluster = classify_mnemonic(&mnemonic);
        if !cluster.sdmodel_supported() {
            return Ok(false);
        }

        if let Some(width) = memory_access_width(&mnemonic) {
            if width > 1 && rng.random_bool(0.6) {
                if self.inject_misaligned_memory_operand(sequence, &line, width)? {
                    return Ok(true);
                }
            }
            if cluster.is_memory() && self.inject_memory_boundary_operand(sequence, &line, width)? {
                return Ok(true);
            }
        }

        if is_div_rem_mnemonic(&mnemonic) {
            if self.inject_div_rem_trigger(sequence, &line, rng)? {
                return Ok(true);
            }
        }

        if cluster.uses_integer_boundary_operands() {
            if self.inject_integer_boundary(sequence, &line, cluster, rng)? {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn inject_misaligned_memory_operand(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        line: &str,
        width: u64,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let Some(caps) = MEMORY_OPERAND_RE.captures(line) else {
            return Ok(false);
        };
        let Some(base) = caps.name("base").and_then(|m| register_number(m.as_str())) else {
            return Ok(false);
        };
        if base == 0 {
            return Ok(false);
        }

        let offset = caps
            .name("imm")
            .and_then(|m| parse_integer_literal(m.as_str()))
            .unwrap_or(0);
        let desired_effective = 1i64;
        let immediate = desired_effective - offset;

        self.update_or_push_base_seed(sequence, base, immediate)?;
        sequence
            .pre_instructions
            .push(PseudoInstruction::Comment(format!(
                "SDModel exception_trigger: misaligned effective address for {}-byte access",
                width
            )));
        Ok(true)
    }

    fn inject_memory_boundary_operand(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        line: &str,
        width: u64,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let Some(caps) = MEMORY_OPERAND_RE.captures(line) else {
            return Ok(false);
        };
        let Some(base) = caps.name("base").and_then(|m| register_number(m.as_str())) else {
            return Ok(false);
        };
        if base == 0 {
            return Ok(false);
        }

        let offset = caps
            .name("imm")
            .and_then(|m| parse_integer_literal(m.as_str()))
            .unwrap_or(0);
        let desired_effective = 0i64;
        let immediate = desired_effective - offset;
        self.update_or_push_base_seed(sequence, base, immediate)?;
        sequence
            .pre_instructions
            .push(PseudoInstruction::Comment(format!(
                "SDModel boundary_value: memory lower-edge effective address for {}-byte access",
                width
            )));
        Ok(true)
    }

    fn update_or_push_base_seed(
        &self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        base: u8,
        immediate: i64,
    ) -> Result<(), DataSensitiveInjectionError> {
        let mut replaced = false;
        let target_name = format!("x{}", base);
        for pseudo in &mut sequence.pre_instructions {
            if let PseudoInstruction::LoadImmediate {
                rd,
                immediate: existing,
                purpose,
            } = pseudo
            {
                if *purpose == LoadImmediatePurpose::BaseAddress && rd.to_string() == target_name {
                    *existing = immediate;
                    replaced = true;
                }
            }
        }

        if !replaced {
            let rd = IntegerRegister::new(base)
                .map_err(|_| DataSensitiveInjectionError::InvalidRegister(base))?;
            sequence
                .pre_instructions
                .push(PseudoInstruction::LoadImmediate {
                    rd,
                    immediate,
                    purpose: LoadImmediatePurpose::BaseAddress,
                });
        }
        Ok(())
    }

    fn inject_div_rem_trigger<R: Rng>(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        line: &str,
        rng: &mut R,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let regs = integer_registers_in_order(line);
        if regs.len() < 3 {
            return Ok(false);
        }
        let rs1 = regs[1];
        let rs2 = regs[2];
        if rs2 == 0 {
            return Ok(false);
        }

        if rng.random_bool(0.5) || rs1 == 0 {
            self.push_register_seed(sequence, rs2, 0, LoadImmediatePurpose::Generic)?;
            sequence.pre_instructions.push(PseudoInstruction::Comment(
                "SDModel exception_trigger: divide-by-zero operand".to_string(),
            ));
        } else {
            let min_signed = signed_min_value(self.isa_base);
            self.push_register_seed(sequence, rs1, min_signed, LoadImmediatePurpose::Generic)?;
            self.push_register_seed(sequence, rs2, -1, LoadImmediatePurpose::Generic)?;
            sequence.pre_instructions.push(PseudoInstruction::Comment(
                "SDModel boundary_value: signed division overflow corner".to_string(),
            ));
        }
        Ok(true)
    }

    fn inject_integer_boundary<R: Rng>(
        &mut self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        line: &str,
        cluster: InstructionCluster,
        rng: &mut R,
    ) -> Result<bool, DataSensitiveInjectionError> {
        let regs = integer_sensitive_registers(line, cluster);
        let Some(target) = regs.into_iter().find(|reg| *reg != 0) else {
            return Ok(false);
        };
        let value = choose_integer_boundary_value(self.isa_base, rng);
        self.push_register_seed(sequence, target, value, LoadImmediatePurpose::Generic)?;
        sequence.pre_instructions.push(PseudoInstruction::Comment(
            "SDModel boundary_value: integer boundary operand".to_string(),
        ));
        Ok(true)
    }

    fn push_register_seed(
        &self,
        sequence: &mut InstructionSequence<RiscvInstruction>,
        reg: u8,
        value: i64,
        purpose: LoadImmediatePurpose,
    ) -> Result<(), DataSensitiveInjectionError> {
        let rd = IntegerRegister::new(reg)
            .map_err(|_| DataSensitiveInjectionError::InvalidRegister(reg))?;
        sequence
            .pre_instructions
            .push(PseudoInstruction::LoadImmediate {
                rd,
                immediate: value,
                purpose,
            });
        Ok(())
    }
}

fn push_data_sensitive_instruction(vec: &mut Vec<PseudoInstruction>, instr: PseudoInstruction) {
    vec.push(instr);
    vec.push(PseudoInstruction::Comment(
        "Pseudo instruction for data-sensitive setup".to_string(),
    ));
}

fn should_inject_operand(class: OperandClass) -> bool {
    !matches!(class, OperandClass::Preserve)
}

fn generate_float_pseudo_instructions(
    temp: IntegerRegister,
    dest: u8,
    bits: FloatBits,
    format: FloatFormat,
) -> Result<Vec<PseudoInstruction>, DataSensitiveInjectionError> {
    if !format.supports_payload_injection() {
        return Err(DataSensitiveInjectionError::UnsupportedFloatFormat(format));
    }

    let target = FloatingPointRegister::new(dest)
        .map_err(|_| DataSensitiveInjectionError::InvalidRegister(dest))?;

    let masked_bits = match format {
        FloatFormat::Half | FloatFormat::BFloat16 => bits & 0xFFFF,
        FloatFormat::Single => bits & 0xFFFF_FFFF,
        FloatFormat::Double => bits & 0xFFFF_FFFF_FFFF_FFFF,
        FloatFormat::Quad => bits,
    };

    Ok(vec![
        PseudoInstruction::LoadImmediate {
            rd: temp,
            immediate: masked_bits as i64,
            purpose: LoadImmediatePurpose::Generic,
        },
        PseudoInstruction::MoveIntegerToFloat {
            target,
            source: temp,
            format,
        },
    ])
}

fn collect_gp_register_usage(sequence: &InstructionSequence<RiscvInstruction>) -> HashSet<u8> {
    let mut gp_used = HashSet::new();

    let mut lines = sequence
        .pre_instructions
        .iter()
        .map(|pseudo| pseudo.to_string())
        .collect::<Vec<_>>();
    lines.push(sequence.instruction.to_string());
    lines.extend(
        sequence
            .post_instructions
            .iter()
            .map(|pseudo| pseudo.to_string()),
    );

    for line in lines {
        collect_from_line(&line.to_lowercase(), &mut gp_used);
    }

    gp_used
}

fn collect_from_line(line: &str, gp_used: &mut HashSet<u8>) {
    for caps in REGISTER_RE.captures_iter(line) {
        if let Ok(idx) = caps[2].parse::<u8>() {
            match &caps[1] {
                "x" => {
                    if idx <= 31 {
                        gp_used.insert(idx);
                    }
                }
                _ => {}
            }
        }
    }

    for caps in ALIAS_RE.captures_iter(line) {
        if let Some(&idx) = ALIAS_MAP.get(caps.get(0).unwrap().as_str()) {
            gp_used.insert(idx);
        }
    }
}

fn floating_source_registers(inst: &RiscvInstruction) -> Vec<u8> {
    let value = match serde_json::to_value(inst) {
        Ok(val) => val,
        Err(_) => return Vec::new(),
    };

    let mut regs = Vec::new();
    collect_fp_sources(&value, &mut regs);
    regs.sort_by_key(|&(order, _)| order);
    regs.into_iter().map(|(_, reg)| reg).collect()
}

fn collect_fp_sources(value: &Value, regs: &mut Vec<(usize, u8)>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                if let Some(idx_str) = key.strip_prefix("fs") {
                    if let Ok(order) = idx_str.parse::<usize>() {
                        if let Some(num) = val.as_u64() {
                            regs.push((order, num as u8));
                            continue;
                        }
                    }
                }
                collect_fp_sources(val, regs);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_fp_sources(val, regs);
            }
        }
        _ => {}
    }
}

fn instruction_mnemonic(inst: &RiscvInstruction) -> String {
    inst.to_string()
        .split_whitespace()
        .next()
        .map(|token| token.to_ascii_lowercase())
        .unwrap_or_default()
}

fn choose_integer_boundary_value<R: Rng>(isa_base: ISABase, rng: &mut R) -> i64 {
    let values: &[i64] = match isa_base {
        ISABase::Rv32 => &[
            0,
            1,
            -1,
            i32::MIN as i64,
            i32::MAX as i64,
            0x7fff_ffff,
            -0x8000_0000,
            0x5555_5555,
            -0x5555_5556,
        ],
        ISABase::Rv64 => &[
            0,
            1,
            -1,
            i64::MIN,
            i64::MAX,
            0x7fff_ffff,
            -0x8000_0000,
            0x5555_5555_5555_5555,
            -0x5555_5555_5555_5556,
        ],
    };
    values[rng.random_range(0..values.len())]
}

fn signed_min_value(isa_base: ISABase) -> i64 {
    match isa_base {
        ISABase::Rv32 => i32::MIN as i64,
        ISABase::Rv64 => i64::MIN,
    }
}

fn integer_registers_in_order(line: &str) -> Vec<u8> {
    INTEGER_REGISTER_RE
        .captures_iter(line)
        .filter_map(|caps| register_number(caps.get(1)?.as_str()))
        .collect()
}

fn integer_sensitive_registers(line: &str, cluster: InstructionCluster) -> Vec<u8> {
    let regs = integer_registers_in_order(line);
    if regs.is_empty() {
        return regs;
    }

    match cluster {
        InstructionCluster::Store
        | InstructionCluster::FloatingStore
        | InstructionCluster::Atomic
        | InstructionCluster::CompressedStack => regs,
        InstructionCluster::Load | InstructionCluster::FloatingLoad => {
            regs.into_iter().skip(1).collect()
        }
        InstructionCluster::FloatingConvert | InstructionCluster::FloatingMove => {
            if line
                .split_once(char::is_whitespace)
                .map(|(_, operands)| operands.trim_start().starts_with('f'))
                .unwrap_or(false)
            {
                regs
            } else {
                regs.into_iter().skip(1).collect()
            }
        }
        InstructionCluster::UpperImmediate => Vec::new(),
        InstructionCluster::Csr => regs.into_iter().skip(1).collect(),
        _ => {
            if regs.len() == 1 {
                regs
            } else {
                regs.into_iter().skip(1).collect()
            }
        }
    }
}

fn register_number(name: &str) -> Option<u8> {
    if let Some(num) = name.strip_prefix('x') {
        return num.parse::<u8>().ok().filter(|value| *value <= 31);
    }
    ALIAS_MAP.get(name).copied()
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

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_instruction::separated_instructions::RV32Extensions;
    use riscv_instruction_types::RandomConfig;

    #[test]
    fn injector_sets_rounding_mode() {
        let mut rng = rand::rng();
        let config = RandomConfig::new();
        let mut seq = generate_specific_sequence(&mut rng, &config, "fadd.s");

        let mut injector =
            DataSensitiveInjector::with_default_plan((10, 15), 1.0).expect("valid temp range");
        let applied = injector
            .try_apply(&mut seq, &mut rng)
            .expect("injection should succeed");
        assert!(applied, "injection should fire when probability is 1.0");

        assert!(
            seq.pre_instructions.iter().any(|inst| {
                matches!(
                    inst,
                    PseudoInstruction::SetRoundingMode { .. }
                        | PseudoInstruction::MoveIntegerToFloat { .. }
                )
            }),
            "pre instructions should include a float-sensitive setup payload"
        );
    }

    #[test]
    fn cover_once_rules_are_tracked_per_instruction() {
        let mut rng = rand::rng();
        let config = RandomConfig::new();
        let mut injector =
            DataSensitiveInjector::with_default_plan((10, 15), 1.0).expect("valid temp range");

        let cover_once_idx = {
            let rule = instruction_rule_for("fmul.s").expect("fmul.s rule should exist");
            rule.candidates()
                .iter()
                .enumerate()
                .find(|(_, cand)| cand.selection_kind() == RuleSelectionKind::CoverOnce)
                .map(|(idx, _)| idx)
                .expect("fmul.s should have a cover-once candidate")
        };

        let mut first = generate_specific_sequence(&mut rng, &config, "fmul.s");
        let applied_first = injector
            .try_apply(&mut first, &mut rng)
            .expect("first injection should succeed");
        assert!(applied_first, "first injection should be applied");

        let completion = injector
            .rule_completion
            .get("fmul.s")
            .expect("completion entry should exist after first apply");
        assert!(
            completion.contains(&cover_once_idx),
            "first application should mark cover-once candidate as done"
        );

        let mut second = generate_specific_sequence(&mut rng, &config, "fmul.s");
        let applied_second = injector
            .try_apply(&mut second, &mut rng)
            .expect("second injection should succeed");
        assert!(
            applied_second,
            "after consuming cover-once candidate, repeatable candidate should still run"
        );

        let completion_after = injector
            .rule_completion
            .get("fmul.s")
            .expect("completion entry should remain");
        assert_eq!(
            completion_after.len(),
            1,
            "only the cover-once candidate should be tracked"
        );
        assert!(
            completion_after.contains(&cover_once_idx),
            "cover-once candidate should stay recorded"
        );
    }

    fn generate_specific_sequence<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
        mnemonic: &str,
    ) -> InstructionSequence<RiscvInstruction> {
        for _ in 0..512 {
            if let Ok(seq) = RV32Extensions::F.random_sequence_with_rng(rng, config) {
                if instruction_mnemonic(&seq.instruction) == mnemonic {
                    return seq;
                }
            }
        }
        panic!("failed to generate instruction for mnemonic {}", mnemonic);
    }
}
