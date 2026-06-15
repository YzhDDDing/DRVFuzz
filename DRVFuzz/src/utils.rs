use std::collections::{BTreeMap, HashMap};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{ContextExtractionError, RegexError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterContextEntry {
    pub name: String,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterContext {
    pub entries: Vec<RegisterContextEntry>,
}

impl Default for RegisterContext {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryValueWidth {
    Byte,
    Half,
    Word,
    Dword,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryContextEntry {
    pub width: MemoryValueWidth,
    pub addresses: Vec<u64>,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryContext {
    pub base_register: String,
    pub base_offset: u64,
    pub entries: Vec<MemoryContextEntry>,
}

impl Default for MemoryContext {
    fn default() -> Self {
        Self {
            base_register: String::new(),
            base_offset: 0,
            entries: Vec::new(),
        }
    }
}

pub fn extract_register_context(
    instruction: &str,
    registers: &HashMap<String, u64>,
) -> Result<Option<RegisterContext>, ContextExtractionError> {
    let reg_names = extract_registers_from_instruction(instruction)?;

    if reg_names.is_empty() {
        return Ok(None);
    }

    let mut entries = Vec::new();
    for reg in reg_names {
        // Only display registers that exist
        if let Some(&value) = registers.get(&reg) {
            entries.push(RegisterContextEntry { name: reg, value });
        }
    }

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(RegisterContext { entries }))
    }
}

pub fn extract_memory_context(
    instruction: &str,
    offset_hint: Option<i64>,
    registers_before: &HashMap<String, u64>,
    memory_before: &BTreeMap<u64, u8>,
    isa_base: crate::isa_base::ISABase,
    user_mem_range: (u64, u64),
) -> Result<Option<MemoryContext>, ContextExtractionError> {
    let addr_pattern = Regex::new(
        r"(?P<imm>-?(?:0[xX][0-9a-fA-F]+|\d+))?\s*\(\s*(?P<base>(?:[xf]\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp))\s*\)",
    )
    .map_err(|source| RegexError::CompilationFailed {
        pattern: r"(?P<imm>-?(?:0[xX][0-9a-fA-F]+|\d+))?\s*\(\s*(?P<base>(?:[xf]\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp))\s*\)"
            .to_string(),
        source,
    })?;

    let captures = if let Some(cap) = addr_pattern.captures(instruction) {
        cap
    } else {
        return Ok(None);
    };

    let base_name_raw = match captures.name("base").map(|m| m.as_str()) {
        Some(name) => name,
        None => return Ok(None),
    };

    let base_name = normalize_register_name(base_name_raw);

    let base_val = if let Some(val) = registers_before.get(&base_name) {
        *val
    } else {
        return Ok(None);
    };

    let imm_str = captures.name("imm").map(|m| m.as_str()).unwrap_or("0");
    let offset = if let Some(value) = offset_hint {
        value
    } else {
        parse_offset(imm_str)?
    };

    let effective_addr = if offset >= 0 {
        base_val.wrapping_add(offset as u64)
    } else {
        base_val.wrapping_sub((-offset) as u64)
    };

    let (mem_start, mem_end) = user_mem_range;
    if mem_end < mem_start {
        return Ok(None);
    }

    if effective_addr < mem_start || effective_addr > mem_end {
        return Ok(None);
    }

    if base_val < mem_start || base_val > mem_end {
        return Ok(None);
    }

    let normalized_base = effective_addr - mem_start;
    let base_offset = base_val - mem_start;
    let max_offset = mem_end - mem_start;

    let mut entries = Vec::new();

    fn gather_width(
        width: usize,
        normalized_base: u64,
        max_offset: u64,
        memory_before: &BTreeMap<u64, u8>,
        kind: MemoryValueWidth,
    ) -> Option<MemoryContextEntry> {
        if width == 0 {
            return None;
        }

        let mut collected = Vec::with_capacity(width);
        for i in 0..width {
            let addr = normalized_base + i as u64;
            if addr > max_offset {
                return None;
            }
            let byte = *memory_before.get(&addr)?;
            collected.push((byte, addr));
        }

        let mut value = 0u64;
        for (idx, (byte, _)) in collected.iter().enumerate() {
            value |= (*byte as u64) << (idx * 8);
        }

        let addresses = collected.into_iter().map(|(_, addr)| addr).collect();

        Some(MemoryContextEntry {
            width: kind,
            addresses,
            value,
        })
    }

    if let Some(entry) = gather_width(
        1,
        normalized_base,
        max_offset,
        memory_before,
        MemoryValueWidth::Byte,
    ) {
        entries.push(entry);
    }

    if let Some(entry) = gather_width(
        2,
        normalized_base,
        max_offset,
        memory_before,
        MemoryValueWidth::Half,
    ) {
        entries.push(entry);
    }

    if let Some(entry) = gather_width(
        4,
        normalized_base,
        max_offset,
        memory_before,
        MemoryValueWidth::Word,
    ) {
        entries.push(entry);
    }

    if isa_base == crate::isa_base::ISABase::Rv64 {
        if let Some(entry) = gather_width(
            8,
            normalized_base,
            max_offset,
            memory_before,
            MemoryValueWidth::Dword,
        ) {
            entries.push(entry);
        }
    }

    if entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(MemoryContext {
            base_register: base_name,
            base_offset,
            entries,
        }))
    }
}
fn parse_offset(value: &str) -> Result<i64, ContextExtractionError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }

    if let Some(hex) = trimmed
        .strip_prefix("-0x")
        .or_else(|| trimmed.strip_prefix("-0X"))
    {
        let magnitude = i64::from_str_radix(hex, 16).map_err(|source| {
            ContextExtractionError::OffsetParseError {
                text: trimmed.to_string(),
                source,
            }
        })?;
        return Ok(-magnitude);
    }

    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16).map_err(|source| {
            ContextExtractionError::OffsetParseError {
                text: trimmed.to_string(),
                source,
            }
        });
    }

    trimmed
        .parse::<i64>()
        .map_err(|source| ContextExtractionError::OffsetParseError {
            text: trimmed.to_string(),
            source,
        })
}

fn get_reg_pattern() -> Result<Regex, ContextExtractionError> {
    Regex::new(r"\b([xf]\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp)\b")
        .map_err(|source| RegexError::CompilationFailed {
            pattern: r"\b([xf]\d+|zero|ra|sp|gp|tp|t[0-6]|s(?:[0-9]|1[01])|a[0-7]|fp)\b"
                .to_string(),
            source,
        })
        .map_err(ContextExtractionError::from)
}

fn normalize_register_name(name: &str) -> String {
    match name {
        // Integer register ABI aliases
        "zero" => "x0".to_string(),
        "ra" => "x1".to_string(),
        "sp" => "x2".to_string(),
        "gp" => "x3".to_string(),
        "tp" => "x4".to_string(),
        "t0" => "x5".to_string(),
        "t1" => "x6".to_string(),
        "t2" => "x7".to_string(),
        "s0" | "fp" => "x8".to_string(),
        "s1" => "x9".to_string(),
        "a0" => "x10".to_string(),
        "a1" => "x11".to_string(),
        "a2" => "x12".to_string(),
        "a3" => "x13".to_string(),
        "a4" => "x14".to_string(),
        "a5" => "x15".to_string(),
        "a6" => "x16".to_string(),
        "a7" => "x17".to_string(),
        "s2" => "x18".to_string(),
        "s3" => "x19".to_string(),
        "s4" => "x20".to_string(),
        "s5" => "x21".to_string(),
        "s6" => "x22".to_string(),
        "s7" => "x23".to_string(),
        "s8" => "x24".to_string(),
        "s9" => "x25".to_string(),
        "s10" => "x26".to_string(),
        "s11" => "x27".to_string(),
        "t3" => "x28".to_string(),
        "t4" => "x29".to_string(),
        "t5" => "x30".to_string(),
        "t6" => "x31".to_string(),
        // Already canonical xN/fN names (and any other registers we don't alias)
        _ => name.to_string(),
    }
}

pub fn extract_registers_from_instruction(
    instruction: &str,
) -> Result<Vec<String>, ContextExtractionError> {
    let reg_pattern = get_reg_pattern()?;

    let mut registers = Vec::new();
    for cap in reg_pattern.captures_iter(instruction) {
        if let Some(reg) = cap.get(1) {
            let reg_name = normalize_register_name(reg.as_str());
            if !registers.contains(&reg_name) {
                registers.push(reg_name);
            }
        }
    }

    Ok(registers)
}
