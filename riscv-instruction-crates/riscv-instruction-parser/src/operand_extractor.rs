use std::collections::HashSet;

/// Extract operand names from assembly syntax
pub fn extract_operands_from_asm_without_name(format: &str) -> HashSet<String> {
    if format.is_empty() {
        return HashSet::new(); // If the format or instruction name is empty, return an empty set
    }

    let mut operands = HashSet::new();

    // Improved logic: remove the instruction-name prefix
    let operands_part_str = format;

    // If nothing remains after removing the instruction name, it's an operand-less instruction
    if operands_part_str.is_empty() {
        return operands;
    }

    // Method 1: match operands inside braces, e.g., {rd}, {rs1}, {imm}
    // Works well for operand parts like "{xd}, {xs1}, {xs2}"
    operands.extend(extract_braced_operands(&operands_part_str));

    // Method 2: use regex patterns to match operands when method 1 is insufficient or formats differ
    // Only likely to add operands if brace extraction was insufficient or operands aren't braced
    if operands.is_empty() || !operands_part_str.contains('{') {
        // Optimization: if brace operands already exist, regex may be unnecessary
        operands.extend(extract_operands_regex(&operands_part_str));
    }

    // Method 3: if nothing found yet, try simple word matching
    if operands.is_empty() {
        operands.extend(extract_operands_simple(&operands_part_str));
    }

    operands
}

fn extract_braced_operands(format_part: &str) -> HashSet<String> {
    // Renamed parameter for clarity
    let mut operands = HashSet::new();
    let mut chars = format_part.chars().peekable(); // Use format_part

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut operand = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    chars.next(); // Consume '}'
                    break;
                } else {
                    operand.push(chars.next().unwrap());
                }
            }
            if !operand.is_empty() {
                operands.insert(operand);
            }
        }
    }

    operands
}

fn extract_operands_regex(format_part: &str) -> HashSet<String> {
    // Renamed parameter
    let mut operands = HashSet::new();

    // Analyze the assembly syntax string and extract possible operands.
    // Handle multiple formats:
    // 1. Space-separated operands: "rd, rs1, imm"
    // 2. Braced format: "{rd}, {rs1}, {imm}" (mostly handled by extract_braced_operands)
    // 3. Mixed format: "rd, {rs1}, imm"
    // 4. Memory-access formats: "imm(rs1)", "offset(base)"

    // Special handling: memory-access form imm(rs1)
    extract_memory_access_operands(format_part, &mut operands); // Use format_part

    // First handle comma-separated operands
    for part in format_part.split(',') {
        // Use format_part
        let cleaned = part.trim();

        // Skip segments containing parentheses; they were handled in memory-access parsing
        if cleaned.contains('(') && cleaned.contains(')') {
            continue;
        }

        let final_cleaned = cleaned
            .trim_matches(|c: char| "{}[]<>".contains(c)) // Remove braces/brackets, keep parentheses for memory detection
            .trim()
            .trim_matches('-'); // Remove leading minus (e.g., -stack_adj)

        if is_likely_operand(final_cleaned) {
            operands.insert(final_cleaned.to_string());
        }

        // Special-case: also capture operands that start with '-' such as -stack_adj or -spimm
        if cleaned.starts_with('-') {
            let without_minus = cleaned.trim_start_matches('-');
            if is_likely_operand(without_minus) {
                operands.insert(without_minus.to_string());
            }
        }
    }

    // Then handle space-separated operands
    for word in format_part.split_whitespace() {
        // Use format_part
        // Skip parts that contain parentheses
        if word.contains('(') && word.contains(')') {
            continue;
        }

        let cleaned = word
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '+')
            .trim_matches('-'); // Remove leading minus

        if is_likely_operand(cleaned) {
            operands.insert(cleaned.to_string());
        }

        // Special-case: operands with a leading minus such as -spimm or -stack_adj
        if word.starts_with('-') {
            let without_minus = word
                .trim_start_matches('-')
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '+');
            if is_likely_operand(without_minus) {
                operands.insert(without_minus.to_string());
            }
        }
    }

    // Special-case: for cm.push and cm.pop instructions, force looking for stack_adj
    if (format_part.contains("cm.push") || format_part.contains("cm.pop"))
        && format_part.contains("stack_adj")
    {
        // Use format_part
        operands.insert("stack_adj".to_string());
    }

    // Special-case: find indexed operands (e.g., rs1+1, vs2+1)
    extract_indexed_operands_from_format(format_part, &mut operands); // Use format_part

    operands
}

fn extract_memory_access_operands(format_part: &str, operands: &mut HashSet<String>) {
    // Renamed parameter
    // Look for memory-access patterns: imm(rs1), offset(base), etc.
    // Pattern: operand(register)

    let mut i = 0;
    let chars: Vec<char> = format_part.chars().collect(); // Use format_part

    while i < chars.len() {
        if chars[i] == '(' {
            // When '(' is found, search backward for the immediate
            let mut start = i;
            while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                start -= 1;
            }

            // Extract the immediate operand
            if start < i {
                let imm_operand: String = chars[start..i].iter().collect();
                let imm_cleaned = imm_operand.trim();
                if is_likely_operand(imm_cleaned) {
                    operands.insert(imm_cleaned.to_string());
                }
            }

            // Extract the register inside the parentheses
            let reg_start = i + 1;
            let mut reg_end = reg_start;
            while reg_end < chars.len() && chars[reg_end] != ')' {
                reg_end += 1;
            }

            if reg_end > reg_start && reg_end < chars.len() {
                let reg_operand: String = chars[reg_start..reg_end].iter().collect();
                let reg_cleaned = reg_operand.trim();
                if is_likely_operand(reg_cleaned) {
                    operands.insert(reg_cleaned.to_string());
                }
                i = reg_end; // Skip the processed portion
            }
        }
        i += 1;
    }
}

fn extract_indexed_operands_from_format(format_part: &str, operands: &mut HashSet<String>) {
    // Renamed parameter
    // Find patterns such as "rs1+1", "vs2+1", "fs1+1"
    let indexed_patterns = [
        "rs1+1", "rs2+1", "rs3+1", "vs1+1", "vs2+1", "vs3+1", "fs1+1", "fs2+1", "fs3+1", "xs1+1",
        "xs2+1", "xs3+1",
    ];

    for pattern in &indexed_patterns {
        if format_part.contains(pattern) {
            // Use format_part
            // Extract the base register name
            if let Some(base_reg) = pattern.split('+').next() {
                operands.insert(base_reg.to_string());
            }
        }
    }

    // Look for other possible indexed patterns, such as "rd+rs1"
    for word in format_part.split_whitespace() {
        // Use format_part
        if word.contains('+') && !word.starts_with(|c: char| c.is_ascii_digit()) {
            // Split words containing '+' to extract potential register names
            for part in word.split('+') {
                let cleaned = part.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                if is_likely_operand(cleaned) {
                    operands.insert(cleaned.to_string());
                }
            }
        }
    }
}

fn extract_operands_simple(format_part: &str) -> HashSet<String> {
    // Renamed parameter
    let mut operands = HashSet::new();

    // Expanded operand patterns covering more variants
    let common_operands = [
        // General-purpose registers
        "rd", "rs1", "rs2", "rs3", // Vector registers
        "vd", "vs1", "vs2", "vs3", "vm", // Floating-point registers
        "fd", "fs1", "fs2", "fs3", // Quad-precision floating registers
        "qd", "qs1", "qs2", "qs3", // Cryptography extension operands
        "xd", "xs1", "xs2", "xs3", // Immediate variants
        "imm", "uimm", "simm", "shamt", "zimm", "zimm5", "zimm10", "zimm11",
        // Other operands
        "aq", "rl", "rm", "pred", "succ", "offset", "csr", "nf",
        // Operands specific to compressed instructions
        "c_rd", "c_rs1", "c_rs2", "c_imm", // Atomic instruction operands
        "ordering",
    ];

    for &operand in &common_operands {
        if is_word_boundary(format_part, operand) {
            // Use format_part
            operands.insert(operand.to_string());
        }
    }

    // Special handling: find patterned operands (e.g., rs1+1, vs2+1)
    extract_indexed_operands(format_part, &mut operands); // Use format_part

    operands
}

fn extract_indexed_operands(format_part: &str, operands: &mut HashSet<String>) {
    // Renamed parameter
    // Look for patterns like "rs1+1" or "vs2+1"
    let patterns = ["rs1+1", "vs2+1", "fs1+1"];

    for pattern in &patterns {
        if format_part.contains(pattern) {
            // Use format_part
            // Extract the base register name
            let base_reg = pattern.split('+').next().unwrap();
            operands.insert(base_reg.to_string());
        }
    }
}

fn is_word_boundary(text: &str, word: &str) -> bool {
    if let Some(start) = text.find(word) {
        let end = start + word.len();

        // Check the character before
        let before_ok = start == 0 || !text.chars().nth(start - 1).unwrap_or(' ').is_alphanumeric();

        // Check the character after
        let after_ok = end >= text.len() || !text.chars().nth(end).unwrap_or(' ').is_alphanumeric();

        before_ok && after_ok
    } else {
        false
    }
}

fn is_likely_operand(word: &str) -> bool {
    if word.is_empty() || word.len() > 15 {
        // Apply a length cap because some operands can be long
        return false;
    }

    // Check whether it matches common operand patterns
    let common_patterns = [
        // Register patterns
        |s: &str| s.starts_with("rd") || s.starts_with("rs") || s.starts_with("rt"),
        |s: &str| s.starts_with("fd") || s.starts_with("fs") || s.starts_with("ft"),
        |s: &str| s.starts_with("vd") || s.starts_with("vs") || s.starts_with("vt"),
        |s: &str| s.starts_with("xd") || s.starts_with("xs"),
        |s: &str| s.starts_with("hd") || s.starts_with("hs"),
        |s: &str| s.starts_with("qd") || s.starts_with("qs"),
        |s: &str| s.starts_with("dd"),
        // Immediate patterns
        |s: &str| s.contains("imm"),
        |s: &str| s.starts_with("shamt"),
        |s: &str| s.starts_with("zimm") || s.starts_with("uimm") || s.starts_with("simm"),
        |s: &str| s == "offset" || s.starts_with("csr"),
        // Special operands
        |s: &str| s == "aq" || s == "rl" || s == "rm" || s == "vm",
        |s: &str| s == "pred" || s == "succ" || s == "nf",
        // Operands specific to compressed instructions
        |s: &str| s == "reg_list" || s == "stack_adj", // cm.push/cm.pop operands
        |s: &str| s == "rlist" || s == "spimm",        // cm.push/cm.pop internal operand names
        // Other common operands
        |s: &str| s == "ordering" || s == "fence" || s == "fm",
        |s: &str| {
            s.starts_with("c_") && (s.contains("rd") || s.contains("rs") || s.contains("imm"))
        },
        // Vector operands
        |s: &str| {
            s.starts_with("v") && (s.len() >= 2) && s[1..].chars().all(|c| c.is_alphanumeric())
        },
        // Floating-point related
        |s: &str| {
            s.starts_with("f") && s.len() >= 2 && s.chars().skip(1).all(|c| c.is_alphanumeric())
        },
        // Generic pattern: identifiers with underscores are likely operands
        |s: &str| s.contains('_') && s.chars().all(|c| c.is_alphanumeric() || c == '_'),
        // Simple alphanumeric strings (at least 2 chars to avoid single-letter noise)
        |s: &str| {
            s.len() >= 2
                && s.chars().all(|c| c.is_alphanumeric())
                && s.chars().any(|c| c.is_alphabetic())
        },
    ];

    common_patterns.iter().any(|pattern| pattern(word))
}
