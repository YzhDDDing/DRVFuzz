use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ISAExtension {
    B,
    C,
    D,
    F,
    H,
    I,
    M,
    Q,
    S,
    Sdext,
    Smdbltrp,
    Smrnmi,
    Svinval,
    V,
    Zaamo,
    Zabha,
    Zacas,
    Zalasr,
    Zalrsc,
    Zawrs,
    Zba,
    Zbb,
    Zbc,
    Zbkb,
    Zbkx,
    Zbs,
    Zcb,
    Zcd,
    Zcf,
    Zcmop,
    Zcmp,
    Zfbfmin,
    Zfh,
    Zicbom,
    Zicboz,
    Zicfilp,
    Zicfiss,
    Zicond,
    Zicsr,
    Zifencei,
    Zilsd,
    Zimop,
    Zkn,
    Zknd,
    Zkne,
    Zknh,
    Zks,
    Zvbb,
    Zvbc,
    Zvfbfmin,
    Zvfbfwma,
    Zvkg,
    Zvkned,
    Zvknha,
    Zvks,
}
impl Display for ISAExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ISAExtension::I => write!(f, "I"),
            ISAExtension::M => write!(f, "M"),
            ISAExtension::F => write!(f, "F"),
            ISAExtension::D => write!(f, "D"),
            ISAExtension::Q => write!(f, "Q"),
            ISAExtension::C => write!(f, "C"),
            ISAExtension::V => write!(f, "V"),
            ISAExtension::B => write!(f, "B"),
            ISAExtension::H => write!(f, "H"),
            ISAExtension::S => write!(f, "S"),
            ISAExtension::Zifencei => write!(f, "Zifencei"),
            ISAExtension::Zicsr => write!(f, "Zicsr"),
            ISAExtension::Zaamo => write!(f, "Zaamo"),
            ISAExtension::Zabha => write!(f, "Zabha"),
            ISAExtension::Zacas => write!(f, "Zacas"),
            ISAExtension::Zalasr => write!(f, "Zalasr"),
            ISAExtension::Zalrsc => write!(f, "Zalrsc"),
            ISAExtension::Zawrs => write!(f, "Zawrs"),
            ISAExtension::Zba => write!(f, "Zba"),
            ISAExtension::Zbb => write!(f, "Zbb"),
            ISAExtension::Zbc => write!(f, "Zbc"),
            ISAExtension::Zbkb => write!(f, "Zbkb"),
            ISAExtension::Zbkx => write!(f, "Zbkx"),
            ISAExtension::Zbs => write!(f, "Zbs"),
            ISAExtension::Zcb => write!(f, "Zcb"),
            ISAExtension::Zcd => write!(f, "Zcd"),
            ISAExtension::Zcf => write!(f, "Zcf"),
            ISAExtension::Zcmop => write!(f, "Zcmop"),
            ISAExtension::Zcmp => write!(f, "Zcmp"),
            ISAExtension::Zfbfmin => write!(f, "Zfbfmin"),
            ISAExtension::Zfh => write!(f, "Zfh"),
            ISAExtension::Zicbom => write!(f, "Zicbom"),
            ISAExtension::Zicboz => write!(f, "Zicboz"),
            ISAExtension::Zicfilp => write!(f, "Zicfilp"),
            ISAExtension::Zicfiss => write!(f, "Zicfiss"),
            ISAExtension::Zicond => write!(f, "Zicond"),
            ISAExtension::Zilsd => write!(f, "Zilsd"),
            ISAExtension::Zimop => write!(f, "Zimop"),
            ISAExtension::Zkn => write!(f, "Zkn"),
            ISAExtension::Zknd => write!(f, "Zknd"),
            ISAExtension::Zkne => write!(f, "Zkne"),
            ISAExtension::Zknh => write!(f, "Zknh"),
            ISAExtension::Zks => write!(f, "Zks"),
            ISAExtension::Zvbb => write!(f, "Zvbb"),
            ISAExtension::Zvbc => write!(f, "Zvbc"),
            ISAExtension::Zvfbfmin => write!(f, "Zvfbfmin"),
            ISAExtension::Zvfbfwma => write!(f, "Zvfbfwma"),
            ISAExtension::Zvkg => write!(f, "Zvkg"),
            ISAExtension::Zvkned => write!(f, "Zvkned"),
            ISAExtension::Zvknha => write!(f, "Zvknha"),
            ISAExtension::Zvks => write!(f, "Zvks"),
            ISAExtension::Sdext => write!(f, "Sdext"),
            ISAExtension::Smdbltrp => write!(f, "Smdbltrp"),
            ISAExtension::Smrnmi => write!(f, "Smrnmi"),
            ISAExtension::Svinval => write!(f, "Svinval"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ISABase {
    RV32,
    RV64,
}
impl Display for ISABase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ISABase::RV32 => write!(f, "RV32"),
            ISABase::RV64 => write!(f, "RV64"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ISAModule {
    pub base: ISABase,
    pub extension: ISAExtension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OperandType {
    // Register types
    IntegerRegister,
    SavedIntegerRegister,
    FloatingPointRegister,
    VectorRegister,
    // Numeric types
    SignedInteger,
    UnsignedInteger,
    // Special formatting types
    CSRAddress,
    RoundMode,
    FenceMode,
    FliConstant,
    SavedRegListWithStackAdj,
    NotEqualCompressedSavedIntegerRegisterPair,
}

impl Display for OperandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperandType::IntegerRegister => write!(f, "Integer Register"),
            OperandType::SavedIntegerRegister => write!(f, "Saved Integer Register"),
            OperandType::FloatingPointRegister => write!(f, "Floating Point Register"),
            OperandType::VectorRegister => write!(f, "Vector Register"),
            OperandType::SignedInteger => write!(f, "Signed Integer"),
            OperandType::UnsignedInteger => write!(f, "Unsigned Integer"),
            OperandType::CSRAddress => write!(f, "CSR Address"),
            OperandType::RoundMode => write!(f, "Round Mode"),
            OperandType::FenceMode => write!(f, "Fence Mode"),
            OperandType::FliConstant => write!(f, "FLI Constant"),
            OperandType::SavedRegListWithStackAdj => write!(f, "Saved Reg List With Stack Adj"),
            OperandType::NotEqualCompressedSavedIntegerRegisterPair => {
                write!(f, "Not Equal Compressed Saved Integer Register Pair")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operand_type: Option<OperandType>,
    pub bit_lengths: HashMap<ISABase, u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restrictions: Option<OperandRestriction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OperandRestriction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple_of: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_max: Option<(i64, i64)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_values: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub odd_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instruction {
    pub name: String,
    pub extension: ISAExtension,
    pub isa_bases: Vec<ISABase>,
    pub operands: Vec<Operand>,
    pub assembly_syntax: AssemblySyntax,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_access: Option<MemoryAddressInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AssemblySyntax {
    Format(String),
    RustCode(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MemoryAddressInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_register_operand: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_register_fixed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_operand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Description {
    String(String),
    Structured(Vec<DescriptionItem>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptionItem {
    pub id: Option<String>,
    pub normative: Option<bool>,
    pub text: String,
}

// YAML parsing structures for RISC-V instruction definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlInstruction {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub kind: String,
    pub name: String,
    pub long_name: String,
    pub description: Description,
    #[serde(rename = "definedBy")]
    pub defined_by: DefinedBy,
    pub base: Option<u8>,
    pub assembly: String,
    pub encoding: Option<Encoding>,
    pub access: Option<Access>,
    #[serde(rename = "data_independent_timing")]
    pub data_independent_timing: Option<bool>,
    #[serde(rename = "operation()")]
    pub operation: Option<String>,
    #[serde(rename = "sail()")]
    pub sail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefinedBy {
    Single(String),
    Multiple(Vec<String>),
    Complex(DefinedByComplex),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefinedByComplex {
    #[serde(rename = "anyOf")]
    pub any_of: Option<Vec<DefinedByItem>>,
    #[serde(rename = "allOf")]
    pub all_of: Option<Vec<DefinedByItem>>,
    #[serde(rename = "oneOf")]
    pub one_of: Option<Vec<DefinedByItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefinedByItem {
    Single(String),
    Multiple(Vec<String>),
    Complex(DefinedByComplex),
}

#[derive(Debug, Clone, Serialize)]
pub enum Encoding {
    Simple {
        #[serde(rename = "match")]
        match_pattern: String,
        #[serde(default)]
        variables: Option<Vec<Variable>>,
    },
    PerISA {
        #[serde(rename = "RV32")]
        rv32: Option<SimpleEncoding>,
        #[serde(rename = "RV64")]
        rv64: Option<SimpleEncoding>,
    },
}

impl<'de> Deserialize<'de> for Encoding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        use serde_yaml::Value;

        let value = Value::deserialize(deserializer)?;

        if let Value::Mapping(ref map) = value {
            // Check for a "match" field — this indicates Simple encoding
            if map.contains_key(&Value::String("match".to_string())) {
                // Simple encoding
                let match_pattern = map
                    .get(&Value::String("match".to_string()))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| D::Error::custom("Missing or invalid 'match' field"))?
                    .to_string();

                let variables = map
                    .get(&Value::String("variables".to_string()))
                    .map(|v| serde_yaml::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| D::Error::custom(format!("Invalid variables: {}", e)))?;

                return Ok(Encoding::Simple {
                    match_pattern,
                    variables,
                });
            }

            // Check for RV32 or RV64 fields — this indicates PerISA encoding
            if map.contains_key(&Value::String("RV32".to_string()))
                || map.contains_key(&Value::String("RV64".to_string()))
            {
                // PerISA encoding
                let rv32 = map
                    .get(&Value::String("RV32".to_string()))
                    .map(|v| serde_yaml::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| D::Error::custom(format!("Invalid RV32: {}", e)))?;

                let rv64 = map
                    .get(&Value::String("RV64".to_string()))
                    .map(|v| serde_yaml::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| D::Error::custom(format!("Invalid RV64: {}", e)))?;

                return Ok(Encoding::PerISA { rv32, rv64 });
            }
        }

        Err(D::Error::custom("Unable to determine encoding format"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEncoding {
    #[serde(rename = "match")]
    pub match_pattern: String,
    #[serde(default)]
    pub variables: Option<Vec<Variable>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(deserialize_with = "deserialize_location")]
    pub location: String,
    pub sign_extend: Option<bool>,
    pub left_shift: Option<u8>,
    pub alias: Option<String>,
    #[serde(rename = "not")]
    pub not_values: Option<serde_yaml::Value>,
}

fn deserialize_location<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value: serde_yaml::Value = serde::Deserialize::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::String(s) => Ok(s),
        serde_yaml::Value::Number(n) => Ok(n.to_string()),
        _ => Err(D::Error::custom("location must be a string or number")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Access {
    pub s: Option<String>,  // supervisor
    pub u: Option<String>,  // user
    pub vs: Option<String>, // virtual supervisor
    pub vu: Option<String>, // virtual user
}

impl ISAExtension {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "I" => Some(ISAExtension::I),
            "M" => Some(ISAExtension::M),
            "F" => Some(ISAExtension::F),
            "D" => Some(ISAExtension::D),
            "Q" => Some(ISAExtension::Q),
            "C" => Some(ISAExtension::C),
            "V" => Some(ISAExtension::V),
            "B" => Some(ISAExtension::B),
            "H" => Some(ISAExtension::H),
            "S" => Some(ISAExtension::S),
            "Zifencei" => Some(ISAExtension::Zifencei),
            "Zicsr" => Some(ISAExtension::Zicsr),
            "Zaamo" => Some(ISAExtension::Zaamo),
            "Zabha" => Some(ISAExtension::Zabha),
            "Zacas" => Some(ISAExtension::Zacas),
            "Zalasr" => Some(ISAExtension::Zalasr),
            "Zalrsc" => Some(ISAExtension::Zalrsc),
            "Zawrs" => Some(ISAExtension::Zawrs),
            "Zba" => Some(ISAExtension::Zba),
            "Zbb" => Some(ISAExtension::Zbb),
            "Zbc" => Some(ISAExtension::Zbc),
            "Zbkb" => Some(ISAExtension::Zbkb),
            "Zbkx" => Some(ISAExtension::Zbkx),
            "Zbs" => Some(ISAExtension::Zbs),
            "Zcb" => Some(ISAExtension::Zcb),
            "Zcd" => Some(ISAExtension::Zcd),
            "Zcf" => Some(ISAExtension::Zcf),
            "Zcmop" => Some(ISAExtension::Zcmop),
            "Zcmp" => Some(ISAExtension::Zcmp),
            "Zfbfmin" => Some(ISAExtension::Zfbfmin),
            "Zfh" => Some(ISAExtension::Zfh),
            "Zicbom" => Some(ISAExtension::Zicbom),
            "Zicboz" => Some(ISAExtension::Zicboz),
            "Zicfilp" => Some(ISAExtension::Zicfilp),
            "Zicfiss" => Some(ISAExtension::Zicfiss),
            "Zicond" => Some(ISAExtension::Zicond),
            "Zilsd" => Some(ISAExtension::Zilsd),
            "Zimop" => Some(ISAExtension::Zimop),
            "Zkn" => Some(ISAExtension::Zkn),
            "Zknd" => Some(ISAExtension::Zknd),
            "Zkne" => Some(ISAExtension::Zkne),
            "Zknh" => Some(ISAExtension::Zknh),
            "Zks" => Some(ISAExtension::Zks),
            "Zvbb" => Some(ISAExtension::Zvbb),
            "Zvbc" => Some(ISAExtension::Zvbc),
            "Zvfbfmin" => Some(ISAExtension::Zvfbfmin),
            "Zvfbfwma" => Some(ISAExtension::Zvfbfwma),
            "Zvkg" => Some(ISAExtension::Zvkg),
            "Zvkned" => Some(ISAExtension::Zvkned),
            "Zvknha" => Some(ISAExtension::Zvknha),
            "Zvks" => Some(ISAExtension::Zvks),
            "Sdext" => Some(ISAExtension::Sdext),
            "Smdbltrp" => Some(ISAExtension::Smdbltrp),
            "Smrnmi" => Some(ISAExtension::Smrnmi),
            "Svinval" => Some(ISAExtension::Svinval),
            _ => None,
        }
    }
}

// Standalone save helper (impl block for Vec<Instruction> was removed)
pub fn save_instructions_to_json<P: AsRef<std::path::Path>>(
    instructions: &[Instruction],
    output_path: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(instructions)?;
    std::fs::write(output_path, json)?;
    Ok(())
}

pub fn save_instructions_to_yaml<P: AsRef<std::path::Path>>(
    instructions: &[Instruction],
    output_path: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let yaml = serde_yaml::to_string(instructions)?;
    std::fs::write(output_path, yaml)?;
    Ok(())
}
