use crate::RoundingMode;
use once_cell::sync::Lazy;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Canonical representation for floating-point payload bits across formats.
pub type FloatBits = u128;

/// Floating-point encoding formats supported by data-sensitive generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FloatFormat {
    Half,
    BFloat16,
    Single,
    Double,
    Quad,
}

impl FloatFormat {
    fn bit_width(&self) -> u32 {
        match self {
            FloatFormat::Half | FloatFormat::BFloat16 => 16,
            FloatFormat::Single => 32,
            FloatFormat::Double => 64,
            FloatFormat::Quad => 128,
        }
    }

    fn mask(&self) -> FloatBits {
        match self.bit_width() {
            16 => 0xFFFF,
            32 => 0xFFFF_FFFF,
            64 => 0xFFFF_FFFF_FFFF_FFFF,
            _ => FloatBits::MAX,
        }
    }

    pub fn supports_payload_injection(&self) -> bool {
        !matches!(self, FloatFormat::Quad)
    }

    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "F" => Some(FloatFormat::Single),
            "D" => Some(FloatFormat::Double),
            "Q" => Some(FloatFormat::Quad),
            "Zfh" | "Zfhmin" => Some(FloatFormat::Half),
            "Zfbfmin" | "Zfbfminnh" => Some(FloatFormat::BFloat16),
            _ => None,
        }
    }

    fn from_format_token(token: &str) -> Option<Self> {
        match token {
            "h" => Some(FloatFormat::Half),
            "s" => Some(FloatFormat::Single),
            "d" => Some(FloatFormat::Double),
            "q" => Some(FloatFormat::Quad),
            "bf16" => Some(FloatFormat::BFloat16),
            _ => None,
        }
    }
}

/// Represents a bucket of interesting floating-point bit patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperandClass {
    Zero,
    NegativeZero,
    PositiveInfinity,
    NegativeInfinity,
    QuietNaN,
    SignalingNaN,
    InvalidOperation,
    SubnormalMin,
    SubnormalMax,
    FlushToZero,
    MaxNormal,
    MinNormal,
    One,
    Random,
    Preserve,
}

impl OperandClass {
    /// Sample a bit pattern for a specific floating-point format.
    pub fn choose_sample_bits_for_format<R: Rng>(
        &self,
        rng: &mut R,
        format: FloatFormat,
    ) -> Option<FloatBits> {
        match self {
            OperandClass::Random => Some(self.generate_random_bits(rng, format)),
            OperandClass::Preserve => None,
            _ => {
                let canonical_bits = self.lookup_canonical_bits(format);
                if canonical_bits.len() == 1 {
                    Some(canonical_bits[0])
                } else {
                    // If multiple candidates exist (e.g., several NaNs), pick one at random
                    Some(canonical_bits[rng.random_range(0..canonical_bits.len())])
                }
            }
        }
    }

    // Generate a random bit pattern
    fn generate_random_bits<R: Rng>(&self, rng: &mut R, format: FloatFormat) -> FloatBits {
        let raw = match format {
            FloatFormat::Half | FloatFormat::BFloat16 => rng.random::<u16>() as FloatBits,
            FloatFormat::Single => rng.random::<u32>() as FloatBits,
            FloatFormat::Double => rng.random::<u64>() as FloatBits,
            FloatFormat::Quad => rng.random::<u128>(),
        };
        raw & format.mask()
    }

    // Renamed from canonical_bits_for_format and removed the Random-handling logic
    fn lookup_canonical_bits(&self, format: FloatFormat) -> &'static [FloatBits] {
        match format {
            FloatFormat::Half => match self {
                OperandClass::Zero => &ZERO_BITS_16,
                OperandClass::NegativeZero => &NEG_ZERO_BITS_16,
                OperandClass::PositiveInfinity => &POS_INF_BITS_16,
                OperandClass::NegativeInfinity => &NEG_INF_BITS_16,
                OperandClass::QuietNaN => &QUIET_NAN_BITS_16,
                OperandClass::SignalingNaN => &SIGNALING_NAN_BITS_16,
                OperandClass::InvalidOperation => &INVALID_OP_BITS_16,
                OperandClass::SubnormalMin => &SUBNORMAL_MIN_BITS_16,
                OperandClass::SubnormalMax => &SUBNORMAL_MAX_BITS_16,
                OperandClass::FlushToZero => &ZERO_BITS_16,
                OperandClass::MaxNormal => &MAX_NORMAL_BITS_16,
                OperandClass::MinNormal => &MIN_NORMAL_BITS_16,
                OperandClass::One => &ONE_BITS_16,
                _ => &[],
            },
            FloatFormat::BFloat16 => match self {
                OperandClass::Zero => &ZERO_BITS_BF16,
                OperandClass::NegativeZero => &NEG_ZERO_BITS_BF16,
                OperandClass::PositiveInfinity => &POS_INF_BITS_BF16,
                OperandClass::NegativeInfinity => &NEG_INF_BITS_BF16,
                OperandClass::QuietNaN => &QUIET_NAN_BITS_BF16,
                OperandClass::SignalingNaN => &SIGNALING_NAN_BITS_BF16,
                OperandClass::InvalidOperation => &INVALID_OP_BITS_BF16,
                OperandClass::SubnormalMin => &SUBNORMAL_MIN_BITS_BF16,
                OperandClass::SubnormalMax => &SUBNORMAL_MAX_BITS_BF16,
                OperandClass::FlushToZero => &ZERO_BITS_BF16,
                OperandClass::MaxNormal => &MAX_NORMAL_BITS_BF16,
                OperandClass::MinNormal => &MIN_NORMAL_BITS_BF16,
                OperandClass::One => &ONE_BITS_BF16,
                _ => &[],
            },
            FloatFormat::Single => match self {
                OperandClass::Zero => &ZERO_BITS_32,
                OperandClass::NegativeZero => &NEG_ZERO_BITS_32,
                OperandClass::PositiveInfinity => &POS_INF_BITS_32,
                OperandClass::NegativeInfinity => &NEG_INF_BITS_32,
                OperandClass::QuietNaN => &QUIET_NAN_BITS_32,
                OperandClass::SignalingNaN => &SIGNALING_NAN_BITS_32,
                OperandClass::InvalidOperation => &INVALID_OP_BITS_32,
                OperandClass::SubnormalMin => &SUBNORMAL_MIN_BITS_32,
                OperandClass::SubnormalMax => &SUBNORMAL_MAX_BITS_32,
                OperandClass::FlushToZero => &ZERO_BITS_32,
                OperandClass::MaxNormal => &MAX_NORMAL_BITS_32,
                OperandClass::MinNormal => &MIN_NORMAL_BITS_32,
                OperandClass::One => &ONE_BITS_32,
                _ => &[],
            },
            FloatFormat::Double => match self {
                OperandClass::Zero => &ZERO_BITS_64,
                OperandClass::NegativeZero => &NEG_ZERO_BITS_64,
                OperandClass::PositiveInfinity => &POS_INF_BITS_64,
                OperandClass::NegativeInfinity => &NEG_INF_BITS_64,
                OperandClass::QuietNaN => &QUIET_NAN_BITS_64,
                OperandClass::SignalingNaN => &SIGNALING_NAN_BITS_64,
                OperandClass::InvalidOperation => &INVALID_OP_BITS_64,
                OperandClass::SubnormalMin => &SUBNORMAL_MIN_BITS_64,
                OperandClass::SubnormalMax => &SUBNORMAL_MAX_BITS_64,
                OperandClass::FlushToZero => &ZERO_BITS_64,
                OperandClass::MaxNormal => &MAX_NORMAL_BITS_64,
                OperandClass::MinNormal => &MIN_NORMAL_BITS_64,
                OperandClass::One => &ONE_BITS_64,
                _ => &[],
            },
            FloatFormat::Quad => match self {
                OperandClass::Zero => &ZERO_BITS_128,
                OperandClass::NegativeZero => &NEG_ZERO_BITS_128,
                OperandClass::PositiveInfinity => &POS_INF_BITS_128,
                OperandClass::NegativeInfinity => &NEG_INF_BITS_128,
                OperandClass::QuietNaN => &QUIET_NAN_BITS_128,
                OperandClass::SignalingNaN => &SIGNALING_NAN_BITS_128,
                OperandClass::InvalidOperation => &INVALID_OP_BITS_128,
                OperandClass::SubnormalMin => &SUBNORMAL_MIN_BITS_128,
                OperandClass::SubnormalMax => &SUBNORMAL_MAX_BITS_128,
                OperandClass::FlushToZero => &ZERO_BITS_128,
                OperandClass::MaxNormal => &MAX_NORMAL_BITS_128,
                OperandClass::MinNormal => &MIN_NORMAL_BITS_128,
                OperandClass::One => &ONE_BITS_128,
                _ => &[],
            },
        }
    }
}

static ZERO_BITS_16: [FloatBits; 1] = [0x0000];
static NEG_ZERO_BITS_16: [FloatBits; 1] = [0x8000];
static POS_INF_BITS_16: [FloatBits; 1] = [0x7C00];
static NEG_INF_BITS_16: [FloatBits; 1] = [0xFC00];
static QUIET_NAN_BITS_16: [FloatBits; 1] = [0x7E00];
static SIGNALING_NAN_BITS_16: [FloatBits; 1] = [0x7C01];
static INVALID_OP_BITS_16: [FloatBits; 1] = [0x7C02];
static SUBNORMAL_MIN_BITS_16: [FloatBits; 1] = [0x0001];
static SUBNORMAL_MAX_BITS_16: [FloatBits; 1] = [0x03FF];
static MAX_NORMAL_BITS_16: [FloatBits; 1] = [0x7BFF];
static MIN_NORMAL_BITS_16: [FloatBits; 1] = [0x0400];
static ONE_BITS_16: [FloatBits; 1] = [0x3C00];

static ZERO_BITS_BF16: [FloatBits; 1] = [0x0000];
static NEG_ZERO_BITS_BF16: [FloatBits; 1] = [0x8000];
static POS_INF_BITS_BF16: [FloatBits; 1] = [0x7F80];
static NEG_INF_BITS_BF16: [FloatBits; 1] = [0xFF80];
static QUIET_NAN_BITS_BF16: [FloatBits; 1] = [0x7FC0];
static SIGNALING_NAN_BITS_BF16: [FloatBits; 1] = [0x7F81];
static INVALID_OP_BITS_BF16: [FloatBits; 1] = [0x7F82];
static SUBNORMAL_MIN_BITS_BF16: [FloatBits; 1] = [0x0001];
static SUBNORMAL_MAX_BITS_BF16: [FloatBits; 1] = [0x007F];
static MAX_NORMAL_BITS_BF16: [FloatBits; 1] = [0x7F7F];
static MIN_NORMAL_BITS_BF16: [FloatBits; 1] = [0x0080];
static ONE_BITS_BF16: [FloatBits; 1] = [0x3F80];

static ZERO_BITS_32: [FloatBits; 1] = [0x0000_0000];
static NEG_ZERO_BITS_32: [FloatBits; 1] = [0x8000_0000];
static POS_INF_BITS_32: [FloatBits; 1] = [0x7F80_0000];
static NEG_INF_BITS_32: [FloatBits; 1] = [0xFF80_0000];
static QUIET_NAN_BITS_32: [FloatBits; 1] = [0x7FC0_0000];
static SIGNALING_NAN_BITS_32: [FloatBits; 1] = [0x7F80_0001];
static INVALID_OP_BITS_32: [FloatBits; 1] = [0x7F80_0002];
static SUBNORMAL_MIN_BITS_32: [FloatBits; 1] = [0x0000_0001];
static SUBNORMAL_MAX_BITS_32: [FloatBits; 1] = [0x007F_FFFF];
static MAX_NORMAL_BITS_32: [FloatBits; 1] = [0x7F7F_FFFF];
static MIN_NORMAL_BITS_32: [FloatBits; 1] = [0x0080_0000];
static ONE_BITS_32: [FloatBits; 1] = [0x3F80_0000];

static ZERO_BITS_64: [FloatBits; 1] = [0x0000_0000_0000_0000];
static NEG_ZERO_BITS_64: [FloatBits; 1] = [0x8000_0000_0000_0000];
static POS_INF_BITS_64: [FloatBits; 1] = [0x7FF0_0000_0000_0000];
static NEG_INF_BITS_64: [FloatBits; 1] = [0xFFF0_0000_0000_0000];
static QUIET_NAN_BITS_64: [FloatBits; 1] = [0x7FF8_0000_0000_0000];
static SIGNALING_NAN_BITS_64: [FloatBits; 1] = [0x7FF0_0000_0000_0001];
static INVALID_OP_BITS_64: [FloatBits; 1] = [0x7FF0_0000_0000_0002];
static SUBNORMAL_MIN_BITS_64: [FloatBits; 1] = [0x0000_0000_0000_0001];
static SUBNORMAL_MAX_BITS_64: [FloatBits; 1] = [0x000F_FFFF_FFFF_FFFF];
static MAX_NORMAL_BITS_64: [FloatBits; 1] = [0x7FEF_FFFF_FFFF_FFFF];
static MIN_NORMAL_BITS_64: [FloatBits; 1] = [0x0010_0000_0000_0000];
static ONE_BITS_64: [FloatBits; 1] = [0x3FF0_0000_0000_0000];

static ZERO_BITS_128: [FloatBits; 1] = [0x0000_0000_0000_0000_0000_0000_0000_0000];
static NEG_ZERO_BITS_128: [FloatBits; 1] = [0x8000_0000_0000_0000_0000_0000_0000_0000];
static POS_INF_BITS_128: [FloatBits; 1] = [0x7FFF_0000_0000_0000_0000_0000_0000_0000];
static NEG_INF_BITS_128: [FloatBits; 1] = [0xFFFF_0000_0000_0000_0000_0000_0000_0000];
static QUIET_NAN_BITS_128: [FloatBits; 1] = [0x7FFF_8000_0000_0000_0000_0000_0000_0000];
static SIGNALING_NAN_BITS_128: [FloatBits; 1] = [0x7FFF_0000_0000_0000_0000_0000_0000_0001];
static INVALID_OP_BITS_128: [FloatBits; 1] = [0x7FFF_0000_0000_0000_0000_0000_0000_0002];
static SUBNORMAL_MIN_BITS_128: [FloatBits; 1] = [0x0000_0000_0000_0000_0000_0000_0000_0001];
static SUBNORMAL_MAX_BITS_128: [FloatBits; 1] = [0x0000_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF];
static MAX_NORMAL_BITS_128: [FloatBits; 1] = [0x7FFE_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF];
static MIN_NORMAL_BITS_128: [FloatBits; 1] = [0x0001_0000_0000_0000_0000_0000_0000_0000];
static ONE_BITS_128: [FloatBits; 1] = [0x3FFF_0000_0000_0000_0000_0000_0000_0000];

/// Standard RISC-V rounding modes for data-sensitive generation.
pub const STANDARD_ROUNDING_MODES: [RoundingMode; 5] = [
    RoundingMode::RNE,
    RoundingMode::RTZ,
    RoundingMode::RDN,
    RoundingMode::RUP,
    RoundingMode::RMM,
];

const INSTRUCTION_SPEC_JSON: &str = include_str!("../../assets/riscv_instructions_new.json");

/// Flexible operand description that can cover arbitrary operand counts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperandSet {
    pub classes: Vec<OperandClass>,
}

impl OperandSet {
    pub fn new(classes: Vec<OperandClass>) -> Self {
        Self { classes }
    }

    pub fn preserve(count: usize) -> Self {
        Self {
            classes: vec![OperandClass::Preserve; count],
        }
    }

    pub fn len(&self) -> usize {
        self.classes.len()
    }
}

/// Rule definition describing how a family of instructions should be biased.
#[derive(Debug)]
pub struct InstructionRule {
    name: String,
    candidates: Vec<RuleCandidate>,
}

impl InstructionRule {
    fn new(name: String, candidates: Vec<RuleCandidate>) -> Self {
        Self { name, candidates }
    }

    pub fn choose_candidate<R: Rng>(&self, rng: &mut R) -> Option<&RuleCandidate> {
        if self.candidates.is_empty() {
            return None;
        }
        let idx = rng.random_range(0..self.candidates.len());
        self.candidates.get(idx)
    }

    pub fn choose_candidate_with_coverage<R: Rng>(
        &self,
        rng: &mut R,
        covered_once: &HashSet<usize>,
    ) -> Option<(usize, &RuleCandidate)> {
        let one_shots: Vec<(usize, &RuleCandidate)> = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(idx, cand)| {
                cand.selection_kind == RuleSelectionKind::CoverOnce && !covered_once.contains(idx)
            })
            .collect();

        let pool: Vec<(usize, &RuleCandidate)> = if !one_shots.is_empty() {
            one_shots
        } else {
            self.candidates
                .iter()
                .enumerate()
                .filter(|(_, cand)| cand.selection_kind == RuleSelectionKind::Repeatable)
                .collect()
        };

        if pool.is_empty() {
            return None;
        }

        let idx = rng.random_range(0..pool.len());
        let (cand_idx, cand) = pool[idx];
        Some((cand_idx, cand))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn candidates(&self) -> &[RuleCandidate] {
        &self.candidates
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleSelectionKind {
    CoverOnce,
    Repeatable,
}

#[derive(Debug)]
pub struct RuleCandidate {
    operand_set: OperandSet,
    rounding_modes: Vec<RoundingMode>,
    description: Option<&'static str>,
    float_format: Option<FloatFormat>,
    selection_kind: RuleSelectionKind,
}

impl RuleCandidate {
    fn new(
        operand_set: OperandSet,
        rounding_modes: Vec<RoundingMode>,
        description: Option<&'static str>,
        float_format: Option<FloatFormat>,
        selection_kind: RuleSelectionKind,
    ) -> Self {
        Self {
            operand_set,
            rounding_modes,
            description,
            float_format,
            selection_kind,
        }
    }

    pub fn operand_set(&self) -> &OperandSet {
        &self.operand_set
    }

    pub fn description(&self) -> Option<&'static str> {
        self.description
    }

    pub fn float_format(&self) -> Option<FloatFormat> {
        self.float_format
    }

    pub fn selection_kind(&self) -> RuleSelectionKind {
        self.selection_kind
    }

    pub fn choose_rounding_mode<R: Rng>(&self, rng: &mut R) -> Option<RoundingMode> {
        if self.rounding_modes.is_empty() {
            return None;
        }
        let idx = rng.random_range(0..self.rounding_modes.len());
        self.rounding_modes.get(idx).copied()
    }

    fn preserve(descriptor: &RuleDescriptor, float_format: Option<FloatFormat>) -> Self {
        Self::new(
            OperandSet::preserve(descriptor.expected_sources),
            descriptor.rounding_modes(),
            Some(descriptor.description),
            float_format,
            RuleSelectionKind::CoverOnce,
        )
    }
}

#[derive(Deserialize)]
struct JsonInstructionSpec {
    name: String,
    extension: String,
    operands: Vec<JsonOperandSpec>,
}

#[derive(Deserialize)]
struct JsonOperandSpec {
    name: String,
    #[serde(rename = "operand_type")]
    operand_type: String,
}

impl JsonInstructionSpec {
    fn float_source_count(&self) -> usize {
        self.operands
            .iter()
            .filter(|operand| {
                operand.operand_type == "FloatingPointRegister" && operand.name.starts_with("fs")
            })
            .count()
    }

    fn floating_source_roles(&self) -> Vec<String> {
        let mut roles: Vec<(usize, String)> = self
            .operands
            .iter()
            .filter_map(|operand| {
                if operand.operand_type != "FloatingPointRegister" {
                    return None;
                }
                operand
                    .name
                    .strip_prefix("fs")
                    .and_then(|suffix| suffix.parse::<usize>().ok())
                    .map(|order| (order, operand.name.clone()))
            })
            .collect();

        roles.sort_by_key(|(order, _)| *order);
        roles.into_iter().map(|(_, name)| name).collect()
    }

    fn float_format(&self) -> Option<FloatFormat> {
        let (_, format_token) = parse_name_parts(&self.name);
        FloatFormat::from_extension(self.extension.as_str())
            .or_else(|| FloatFormat::from_format_token(format_token))
    }
}

static DATA_SENSITIVE_RULES: Lazy<Vec<InstructionRule>> =
    Lazy::new(build_instruction_rules);

static INSTRUCTION_RULE_INDEX: Lazy<HashMap<String, usize>> = Lazy::new(|| {
    let mut index = HashMap::new();
    for (idx, rule) in DATA_SENSITIVE_RULES.iter().enumerate() {
        index.insert(rule.name().to_string(), idx);
    }
    index
});

#[derive(Clone)]
struct CandidateDescriptor {
    overrides: Vec<(String, OperandClass)>,
    rounding: RoundingStyle,
    description: Option<&'static str>,
    selection_kind: RuleSelectionKind,
}

impl CandidateDescriptor {
    fn new(
        overrides: Vec<(String, OperandClass)>,
        rounding: RoundingStyle,
        description: Option<&'static str>,
    ) -> Self {
        Self {
            overrides,
            rounding,
            description,
            selection_kind: RuleSelectionKind::Repeatable,
        }
    }

    fn from_static(
        overrides: &'static [(&'static str, OperandClass)],
        rounding: RoundingStyle,
        description: Option<&'static str>,
    ) -> Self {
        let converted = overrides
            .iter()
            .map(|(role, class)| ((*role).to_string(), *class))
            .collect();
        Self::new(converted, rounding, description)
    }

    fn with_selection_kind(mut self, selection_kind: RuleSelectionKind) -> Self {
        self.selection_kind = selection_kind;
        self
    }
}

struct StaticCandidateDescriptor {
    overrides: &'static [(&'static str, OperandClass)],
    rounding: RoundingStyle,
    description: Option<&'static str>,
    selection_kind: RuleSelectionKind,
}

impl StaticCandidateDescriptor {
    const fn new(
        overrides: &'static [(&'static str, OperandClass)],
        rounding: RoundingStyle,
        description: Option<&'static str>,
    ) -> Self {
        Self {
            overrides,
            rounding,
            description,
            selection_kind: RuleSelectionKind::Repeatable,
        }
    }

    fn to_descriptor(&self) -> CandidateDescriptor {
        CandidateDescriptor::from_static(self.overrides, self.rounding, self.description)
            .with_selection_kind(self.selection_kind)
    }
}

static CUSTOM_FLOAT_RULES: Lazy<HashMap<&'static str, Vec<StaticCandidateDescriptor>>> =
    Lazy::new(|| {
        HashMap::from([(
            "fadd.s",
            vec![StaticCandidateDescriptor::new(
                &[
                    ("fs1", OperandClass::MaxNormal),
                    ("fs2", OperandClass::SubnormalMin),
                ],
                RoundingStyle::Standard,
                Some("fadd.s extreme operand mix"),
            )],
        )])
    });

static SUPPORTED_INSTRUCTIONS: Lazy<Vec<String>> = Lazy::new(|| {
    let mut names: Vec<String> = DATA_SENSITIVE_RULES
        .iter()
        .map(|rule| rule.name().to_string())
        .collect();
    names.sort();
    names
});

fn build_instruction_rules() -> Vec<InstructionRule> {
    let specs: Vec<JsonInstructionSpec> =
        serde_json::from_str(INSTRUCTION_SPEC_JSON).expect("invalid instruction JSON");
    specs
        .into_iter()
        .filter_map(build_rule_from_spec)
        .collect()
}

fn build_rule_from_spec(spec: JsonInstructionSpec) -> Option<InstructionRule> {
    let float_sources = spec.float_source_count();
    if float_sources == 0 {
        return None;
    }

    let (family, _) = parse_name_parts(&spec.name);
    let float_format = spec.float_format();
    let descriptor = descriptor_for_family(family, float_sources)?;
    let candidates = build_rule_candidates(&spec, family, float_format, descriptor);

    Some(InstructionRule::new(spec.name, candidates))
}

fn parse_name_parts(name: &str) -> (&str, &str) {
    let mut iter = name.split('.');
    let family = iter.next().unwrap_or("");
    let format = iter.next().unwrap_or("");
    (family, format)
}

fn build_rule_candidates(
    spec: &JsonInstructionSpec,
    family: &str,
    float_format: Option<FloatFormat>,
    descriptor: RuleDescriptor,
) -> Vec<RuleCandidate> {
    let mut descriptors = Vec::new();
    let usable_format = float_format.filter(FloatFormat::supports_payload_injection);
    if let Some(format) = usable_format {
        descriptors.extend(general_float_candidate_descriptors(spec, format));
        descriptors.extend(family_specific_candidate_descriptors(spec, family, format));
    }
    if let Some(custom) = CUSTOM_FLOAT_RULES.get(spec.name.as_str()) {
        descriptors.extend(custom.iter().map(|cfg| cfg.to_descriptor()));
    }

    let mut candidates: Vec<RuleCandidate> = descriptors
        .into_iter()
        .filter_map(|desc| build_candidate_from_descriptor(spec, desc, usable_format))
        .collect();

    if candidates.is_empty() {
        candidates.push(RuleCandidate::preserve(&descriptor, usable_format));
    }

    candidates
}

fn build_candidate_from_descriptor(
    spec: &JsonInstructionSpec,
    descriptor: CandidateDescriptor,
    float_format: Option<FloatFormat>,
) -> Option<RuleCandidate> {
    let roles = spec.floating_source_roles();
    if roles.is_empty() {
        return None;
    }

    let mut override_map = HashMap::new();
    for (role, class) in descriptor.overrides {
        override_map.insert(role, class);
    }

    if override_map
        .keys()
        .any(|role| !roles.iter().any(|existing| existing == role))
    {
        return None;
    }

    let mut classes = Vec::with_capacity(roles.len());
    for role in &roles {
        if let Some(class) = override_map.get(role) {
            classes.push(*class);
        } else {
            classes.push(OperandClass::Preserve);
        }
    }

    Some(RuleCandidate::new(
        OperandSet::new(classes),
        descriptor.rounding.modes(),
        descriptor.description,
        float_format,
        descriptor.selection_kind,
    ))
}

fn general_float_candidate_descriptors(
    spec: &JsonInstructionSpec,
    _format: FloatFormat,
) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.is_empty() {
        return Vec::new();
    }

    let mut descriptors = Vec::new();
    let first = roles[0].clone();
    descriptors.push(CandidateDescriptor::new(
        vec![(first.clone(), OperandClass::QuietNaN)],
        RoundingStyle::None,
        Some("Quiet NaN propagation"),
    ));
    descriptors.push(CandidateDescriptor::new(
        vec![(first.clone(), OperandClass::SignalingNaN)],
        RoundingStyle::None,
        Some("Signaling NaN propagation"),
    ));

    if roles.len() >= 2 {
        descriptors.push(CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::NegativeZero),
                (roles[1].clone(), OperandClass::Zero),
            ],
            RoundingStyle::Standard,
            Some("Signed zero interaction"),
        ));
        descriptors.push(CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::SubnormalMax),
                (roles[1].clone(), OperandClass::SubnormalMin),
            ],
            RoundingStyle::Standard,
            Some("Subnormal operand interaction"),
        ));
    }

    descriptors
}

fn family_specific_candidate_descriptors(
    spec: &JsonInstructionSpec,
    family: &str,
    format: FloatFormat,
) -> Vec<CandidateDescriptor> {
    match family {
        "fadd" | "fsub" => addition_subtraction_descriptors(spec, format, family == "fsub"),
        "fmul" | "fdiv" => multiplication_like_descriptors(spec, format),
        "fmadd" | "fmsub" | "fnmadd" | "fnmsub" => {
            fused_multiply_add_descriptors(spec, format)
        }
        "fsqrt" => sqrt_descriptors(spec, format),
        "fmin" | "fmax" => minmax_descriptors(spec, format),
        _ => Vec::new(),
    }
}

fn addition_subtraction_descriptors(
    spec: &JsonInstructionSpec,
    format: FloatFormat,
    is_subtraction: bool,
) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.len() < 2 {
        return Vec::new();
    }

    let mut descriptors = addition_operand_classes(&roles, format);

    if is_subtraction {
        descriptors.push(CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::MinNormal),
                (roles[1].clone(), OperandClass::SubnormalMax),
            ],
            RoundingStyle::Standard,
            Some("Catastrophic cancellation (min - subnormal max)"),
        ));
    }

    descriptors
}

fn addition_operand_classes(
    roles: &[String],
    format: FloatFormat,
) -> Vec<CandidateDescriptor> {
    if roles.len() < 2 {
        return Vec::new();
    }

    let lhs = roles[0].clone();
    let rhs = roles[1].clone();

    let scenarios = match format {
        FloatFormat::Half => vec![
            (
                (OperandClass::MaxNormal, OperandClass::MaxNormal),
                RoundingStyle::Standard,
                Some("Add/sub overflow stress"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::PositiveInfinity),
                RoundingStyle::None,
                Some("Infinity accumulation"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Infinity cancellation"),
            ),
            (
                (OperandClass::NegativeInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Negative infinity accumulation"),
            ),
            (
                (OperandClass::SubnormalMax, OperandClass::SubnormalMin),
                RoundingStyle::Standard,
                Some("Denormal add/sub"),
            ),
        ],
        FloatFormat::BFloat16 => vec![
            (
                (OperandClass::MaxNormal, OperandClass::MaxNormal),
                RoundingStyle::Standard,
                Some("Add/sub overflow stress"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::PositiveInfinity),
                RoundingStyle::None,
                Some("Infinity accumulation"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Infinity cancellation"),
            ),
            (
                (OperandClass::NegativeInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Negative infinity accumulation"),
            ),
            (
                // bf16 exponent matches single, so mix inf with max normal to stress accumulation
                (OperandClass::PositiveInfinity, OperandClass::MaxNormal),
                RoundingStyle::None,
                Some("Infinity with finite accumulation"),
            ),
            (
                (OperandClass::SubnormalMax, OperandClass::SubnormalMax),
                RoundingStyle::Standard,
                Some("Denormal add/sub"),
            ),
        ],
        FloatFormat::Single | FloatFormat::Double | FloatFormat::Quad => vec![
            (
                (OperandClass::MaxNormal, OperandClass::MaxNormal),
                RoundingStyle::Standard,
                Some("Add/sub overflow stress"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::PositiveInfinity),
                RoundingStyle::None,
                Some("Infinity accumulation"),
            ),
            (
                (OperandClass::PositiveInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Infinity cancellation"),
            ),
            (
                (OperandClass::NegativeInfinity, OperandClass::NegativeInfinity),
                RoundingStyle::None,
                Some("Negative infinity accumulation"),
            ),
            (
                (OperandClass::SubnormalMax, OperandClass::SubnormalMax),
                RoundingStyle::Standard,
                Some("Denormal add/sub"),
            ),
        ],
    };

    scenarios
        .into_iter()
        .map(|((lhs_class, rhs_class), rounding, description)| {
            CandidateDescriptor::new(
                vec![(lhs.clone(), lhs_class), (rhs.clone(), rhs_class)],
                rounding,
                description,
            )
        })
        .collect()
}

fn multiplication_like_descriptors(
    spec: &JsonInstructionSpec,
    _format: FloatFormat,
) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.len() < 2 {
        return Vec::new();
    }
    vec![
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::MaxNormal),
                (roles[1].clone(), OperandClass::MinNormal),
            ],
            RoundingStyle::Standard,
            Some("Multiplication overflow stress"),
        ),
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::Random),
                (roles[1].clone(), OperandClass::Random),
            ],
            RoundingStyle::Standard,
            Some("Randomized multiplication operands"),
        )
        .with_selection_kind(RuleSelectionKind::CoverOnce),
    ]
}

fn fused_multiply_add_descriptors(
    spec: &JsonInstructionSpec,
    _format: FloatFormat,
) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.len() < 3 {
        return Vec::new();
    }
    vec![
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::MaxNormal),
                (roles[1].clone(), OperandClass::MaxNormal),
                (roles[2].clone(), OperandClass::SubnormalMin),
            ],
            RoundingStyle::Standard,
            Some("Fused multiply-add overflow"),
        ),
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::NegativeZero),
                (roles[1].clone(), OperandClass::Zero),
                (roles[2].clone(), OperandClass::QuietNaN),
            ],
            RoundingStyle::None,
            Some("Fused multiply-add zero/NaN mix"),
        ),
    ]
}

fn sqrt_descriptors(spec: &JsonInstructionSpec, _format: FloatFormat) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.is_empty() {
        return Vec::new();
    }
    vec![
        CandidateDescriptor::new(
            vec![(roles[0].clone(), OperandClass::NegativeZero)],
            RoundingStyle::Standard,
            Some("Square root of negative zero"),
        ),
        CandidateDescriptor::new(
            vec![(roles[0].clone(), OperandClass::SubnormalMin)],
            RoundingStyle::Standard,
            Some("Square root subnormal input"),
        ),
    ]
}

fn minmax_descriptors(spec: &JsonInstructionSpec, _format: FloatFormat) -> Vec<CandidateDescriptor> {
    let roles = spec.floating_source_roles();
    if roles.len() < 2 {
        return Vec::new();
    }
    vec![
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::PositiveInfinity),
                (roles[1].clone(), OperandClass::NegativeInfinity),
            ],
            RoundingStyle::None,
            Some("Min/max infinity ordering"),
        ),
        CandidateDescriptor::new(
            vec![
                (roles[0].clone(), OperandClass::QuietNaN),
                (roles[1].clone(), OperandClass::Random),
            ],
            RoundingStyle::None,
            Some("Min/max NaN absorption"),
        ),
    ]
}

fn descriptor_for_family(family: &str, float_sources: usize) -> Option<RuleDescriptor> {
    use RoundingStyle::{None as NoRounding, Standard};

    let descriptor = match family {
        "fadd" | "fsub" | "fmul" | "fdiv" => {
            RuleDescriptor::new(2, Standard, "Binary floating-point arithmetic")
        }
        "fmin" | "fmax" => RuleDescriptor::new(2, NoRounding, "Floating-point min/max selection"),
        "fsqrt" => RuleDescriptor::new(1, Standard, "Square root operations"),
        "fmadd" | "fmsub" | "fnmadd" | "fnmsub" => {
            RuleDescriptor::new(3, Standard, "Fused multiply-add style operations")
        }
        "fsgnj" | "fsgnjn" | "fsgnjx" => {
            RuleDescriptor::new(2, NoRounding, "Sign injection and bit-level moves")
        }
        "feq" | "fle" | "flt" => RuleDescriptor::new(2, NoRounding, "Floating-point comparisons"),
        "fclass" => RuleDescriptor::new(1, NoRounding, "Floating-point classification"),
        "fcvt" | "fcvtmod" => RuleDescriptor::new(1, Standard, "Floating-point conversion operations"),
        "fmv" | "fmvh" => RuleDescriptor::new(1, NoRounding, "Floating-point register moves"),
        "fround" | "froundnx" => RuleDescriptor::new(1, Standard, "Floating-point rounding operations"),
        _ => return None,
    };

    if descriptor.expected_sources != float_sources {
        return None;
    }
    Some(descriptor)
}

struct RuleDescriptor {
    expected_sources: usize,
    rounding: RoundingStyle,
    description: &'static str,
}

impl RuleDescriptor {
    const fn new(
        expected_sources: usize,
        rounding: RoundingStyle,
        description: &'static str,
    ) -> Self {
        Self {
            expected_sources,
            rounding,
            description,
        }
    }

    fn rounding_modes(&self) -> Vec<RoundingMode> {
        self.rounding.modes()
    }
}

#[derive(Clone, Copy)]
enum RoundingStyle {
    None,
    Standard,
}

impl RoundingStyle {
    fn modes(&self) -> Vec<RoundingMode> {
        match self {
            RoundingStyle::None => Vec::new(),
            RoundingStyle::Standard => STANDARD_ROUNDING_MODES.to_vec(),
        }
    }
}

/// Return the static list of instruction rules currently registered.
pub fn instruction_rules() -> &'static [InstructionRule] {
    &DATA_SENSITIVE_RULES
}

/// Lookup the rule that should be used for a specific instruction mnemonic.
pub fn instruction_rule_for(name: &str) -> Option<&'static InstructionRule> {
    INSTRUCTION_RULE_INDEX
        .get(name)
        .and_then(|&idx| DATA_SENSITIVE_RULES.get(idx))
}

/// Enumerate the exact instruction mnemonics that can be biased today.
pub fn supported_instruction_names() -> &'static [String] {
    &SUPPORTED_INSTRUCTIONS
}

/// Configuration used when generation should favor data-sensitive operand values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSensitiveConfig {
    enabled: bool,
    probability: f64,
}

impl Default for DataSensitiveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            probability: 1.0,
        }
    }
}

impl DataSensitiveConfig {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(probability: f64) -> Self {
        Self {
            enabled: true,
            probability: probability.clamp(0.0, 1.0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn probability(&self) -> f64 {
        self.probability
    }

    pub fn should_apply<R: Rng>(&self, rng: &mut R) -> bool {
        self.enabled && rng.random_bool(self.probability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn lookup_known_instruction() {
        let rule = instruction_rule_for("fadd.s").expect("rule should exist for fadd.s");
        assert!(
            !rule.candidates().is_empty(),
            "fadd.s should have at least one rule candidate"
        );
        assert!(
            rule.candidates()
                .iter()
                .any(|cand| cand.operand_set().len() == 2),
            "fadd.s should include binary candidates"
        );
        assert!(
            rule.candidates()
                .iter()
                .all(|cand| cand.float_format() == Some(FloatFormat::Single)),
            "fadd.s candidates should be tagged with single-precision format"
        );
    }

    #[test]
    fn supported_instruction_list_not_empty() {
        let names = supported_instruction_names();
        assert!(
            names.iter().any(|name| name == "fadd.s"),
            "expected fadd.s to be part of the supported list"
        );
    }

    #[test]
    fn operand_set_preserve_builder() {
        let set = OperandSet::preserve(3);
        assert_eq!(set.classes.len(), 3);
        assert!(set.classes.iter().all(|class| *class == OperandClass::Preserve));
    }

    #[test]
    fn custom_rule_overrides_operands() {
        let rule = instruction_rule_for("fadd.s").expect("rule must exist");
        let candidate = rule
            .candidates()
            .iter()
            .find(|cand| cand.description() == Some("fadd.s extreme operand mix"))
            .expect("custom candidate should exist for fadd.s");
        let set = candidate.operand_set();
        assert_eq!(set.classes.len(), 2);
        assert_eq!(set.classes[0], OperandClass::MaxNormal);
        assert_eq!(set.classes[1], OperandClass::SubnormalMin);
    }

    #[test]
    fn coverage_prefers_uncovered_one_shots() {
        let candidates = vec![
            RuleCandidate::new(
                OperandSet::preserve(1),
                Vec::new(),
                Some("cover once"),
                None,
                RuleSelectionKind::CoverOnce,
            ),
            RuleCandidate::new(
                OperandSet::preserve(1),
                Vec::new(),
                Some("repeatable"),
                None,
                RuleSelectionKind::Repeatable,
            ),
        ];
        let rule = InstructionRule::new("test.coverage".to_string(), candidates);
        let mut rng = rand::rng();
        let mut covered = HashSet::new();

        let (first_idx, first) = rule
            .choose_candidate_with_coverage(&mut rng, &covered)
            .expect("first candidate should be available");
        assert_eq!(first_idx, 0);
        assert_eq!(first.selection_kind(), RuleSelectionKind::CoverOnce);

        covered.insert(first_idx);
        let (second_idx, second) = rule
            .choose_candidate_with_coverage(&mut rng, &covered)
            .expect("repeatable candidate should be available");
        assert_eq!(second_idx, 1);
        assert_eq!(second.selection_kind(), RuleSelectionKind::Repeatable);
    }

    #[test]
    fn coverage_exhausts_one_shot_candidates() {
        let candidates = vec![RuleCandidate::new(
            OperandSet::preserve(1),
            Vec::new(),
            None,
            None,
            RuleSelectionKind::CoverOnce,
        )];
        let rule = InstructionRule::new("test.coverage.empty".to_string(), candidates);
        let mut rng = rand::rng();
        let mut covered = HashSet::new();

        let (idx, _) = rule
            .choose_candidate_with_coverage(&mut rng, &covered)
            .expect("single candidate should be chosen");
        covered.insert(idx);

        assert!(
            rule.choose_candidate_with_coverage(&mut rng, &covered)
                .is_none(),
            "all candidates were cover-once and should be exhausted"
        );
    }
}
