/// Format all mcause values in a consistent hexadecimal representation.
pub fn format_cause_code(code: u64) -> String {
    format!("{code:#x}")
}

/// Parse hexadecimal mcause strings such as `0x5` or `5`.
pub fn parse_cause_hex(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u64::from_str_radix(digits, 16).ok()
}

/// Try to map textual exception names into standard mcause codes.
///
/// The input may contain lowercase, hyphens or spaces (e.g. "ld access fault", "ILLEGAL-INSTR").
pub fn canonical_cause_code_from_name(name: &str) -> Option<u64> {
    if let Some(hex) = parse_cause_hex(name) {
        return Some(hex);
    }
    let mut normalized = normalize_alias(name);
    if let Some(stripped) = normalized.strip_prefix("TRAP_") {
        normalized = stripped.to_string();
    }
    if normalized.is_empty() {
        return None;
    }
    match normalized.as_str() {
        "INSTR_ADDR_MISALIGNED" | "INSTRUCTION_ADDR_MISALIGNED" | "MISALIGNED_FETCH" => Some(0),
        "INSTR_ACCESS_FAULT"
        | "INSTRUCTION_ACCESS_FAULT"
        | "INSTR_BUS_FAULT"
        | "FETCH_ACCESS"
        | "INSTR_ACCESS" => Some(1),
        "ILLEGAL_INSTR" | "ILLEGAL_INSTRUCTION" | "UNSUPPORTED_INSTR" => Some(2),
        "BREAKPOINT" | "EBREAK" => Some(3),
        "LD_ADDR_MISALIGNED"
        | "LOAD_ADDR_MISALIGNED"
        | "LOAD_ADDRESS_MISALIGNED"
        | "MISALIGNED_LOAD" => Some(4),
        "LD_ACCESS_FAULT" | "LOAD_ACCESS_FAULT" | "LOAD_ACCESS" => Some(5),
        "ST_ADDR_MISALIGNED"
        | "STORE_ADDR_MISALIGNED"
        | "STORE_ADDRESS_MISALIGNED"
        | "MISALIGNED_STORE" => Some(6),
        "ST_ACCESS_FAULT" | "STORE_ACCESS_FAULT" | "STORE_ACCESS" => Some(7),
        "ENV_CALL_U" | "ENV_CALL_UMODE" | "ECALL_U" | "ECALL_FROM_U" | "USER_ECALL" => Some(8),
        "ENV_CALL_S" | "ECALL_S" | "ECALL_FROM_S" | "SUPERVISOR_ECALL" => Some(9),
        "ENV_CALL_VS" | "ECALL_VS" | "ECALL_FROM_VS" | "VIRTUAL_SUPERVISOR_ECALL" => Some(10),
        "ENV_CALL_M" | "ECALL_M" | "ECALL_FROM_M" | "MACHINE_ECALL" => Some(11),
        "INSTR_PAGE_FAULT" | "INSTRUCTION_PAGE_FAULT" | "FETCH_PAGE_FAULT" => Some(12),
        "LD_PAGE_FAULT" | "LOAD_PAGE_FAULT" => Some(13),
        "ST_PAGE_FAULT" | "STORE_PAGE_FAULT" => Some(15),
        "DOUBLE_TRAP" => Some(16),
        "SOFTWARE_CHECK" => Some(18),
        "HARDWARE_ERROR" => Some(19),
        "INSTR_GUEST_PAGE_FAULT" | "INSTRUCTION_GUEST_PAGE_FAULT" | "FETCH_GUEST_PAGE_FAULT" => {
            Some(20)
        }
        "LD_GUEST_PAGE_FAULT" | "LOAD_GUEST_PAGE_FAULT" => Some(21),
        "VIRTUAL_INSTRUCTION" | "VIRTUAL_INSTR" => Some(22),
        "ST_GUEST_PAGE_FAULT" | "STORE_GUEST_PAGE_FAULT" => Some(23),
        "DEBUG_REQUEST" | "DEBUG_REQ" => Some(24),
        // Additional CVA6/BSP specific aliases.
        "INSTR_INTEGRITY_FAULT" => Some(25),
        _ => None,
    }
}

fn normalize_alias(input: &str) -> String {
    let mut normalized = String::with_capacity(input.len());
    let mut last_was_underscore = false;
    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_uppercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            normalized.push('_');
            last_was_underscore = true;
        }
    }
    if normalized.ends_with('_') {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::canonical_cause_code_from_name;

    #[test]
    fn maps_basic_aliases() {
        assert_eq!(canonical_cause_code_from_name("illegal_instr"), Some(2));
        assert_eq!(canonical_cause_code_from_name("LOAD access fault"), Some(5));
        assert_eq!(
            canonical_cause_code_from_name("st_guest_page_fault"),
            Some(23)
        );
    }
}
