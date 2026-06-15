use log::{debug, error};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::user_instruction::UserInstructionInfo;

pub struct Tracer {
    pc_to_user_inst_idx: HashMap<u64, usize>,
    user_inst_idx_to_pcs: HashMap<usize, Vec<u64>>,
}

impl Tracer {
    pub fn new<P: AsRef<Path>>(user_insts: &[String], dump_file_path: P) -> std::io::Result<Self> {
        let path = dump_file_path.as_ref();
        debug!("Loading dump file: {}", path.display());
        let dump_content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => {
                error!("Failed to read dump file {}: {}", path.display(), e);
                return Err(e);
            }
        };

        Self::from_dump_str(&dump_content, user_insts)
    }

    fn from_dump_str(dump: &str, user_insts: &[String]) -> std::io::Result<Self> {
        // Build UserInstructionInfo from the user instructions
        let user_instruction_info = UserInstructionInfo::build(user_insts);

        let mut pc_to_user_inst_idx: HashMap<u64, usize> = HashMap::new();
        let mut active_user_inst_idx: Option<usize> = None;
        let mut found_indices: HashSet<usize> = HashSet::new();

        for line in dump.lines() {
            if let Some(pc) = parse_pc_line(line) {
                if let Some(idx) = active_user_inst_idx {
                    pc_to_user_inst_idx.insert(pc, idx);
                }
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with('.') || trimmed.ends_with(':') {
                active_user_inst_idx = None;
                continue;
            }

            let normalized = normalize_whitespace(trimmed);
            if normalized.is_empty() {
                active_user_inst_idx = None;
                continue;
            }

            for (idx, user_inst) in user_instruction_info.iter().enumerate() {
                if user_inst.instruction == normalized && !found_indices.contains(&idx) {
                    active_user_inst_idx = Some(idx);
                    found_indices.insert(idx);
                    break;
                }
            }
        }

        let missing_indices: Vec<usize> = (0..user_instruction_info.len())
            .filter(|idx| !found_indices.contains(idx))
            .collect();
        if !missing_indices.is_empty() {
            let missing_insts: Vec<_> = missing_indices
                .iter()
                .map(|&idx| &user_instruction_info[idx])
                .collect();
            error!("Missing user instructions in dump: {:?}", missing_insts);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Dump file is missing {} user instructions",
                    missing_indices.len()
                ),
            ));
        }

        let mut user_inst_idx_to_pcs: HashMap<usize, Vec<u64>> = HashMap::new();
        for (&pc, &idx) in &pc_to_user_inst_idx {
            user_inst_idx_to_pcs
                .entry(idx)
                .or_insert_with(Vec::new)
                .push(pc);
        }

        Ok(Self {
            pc_to_user_inst_idx,
            user_inst_idx_to_pcs,
        })
    }

    pub fn get_user_inst_idx(&self, pc: u64) -> Option<usize> {
        self.pc_to_user_inst_idx.get(&pc).copied()
    }

    pub fn get_all_pcs_for_user_inst(&self, idx: usize) -> Option<&[u64]> {
        self.user_inst_idx_to_pcs.get(&idx).map(|v| v.as_slice())
    }
}

fn parse_pc_line(line: &str) -> Option<u64> {
    let trimmed = line.trim();
    let colon_pos = trimmed.find(':')?;
    let addr_str = trimmed[..colon_pos].trim();
    u64::from_str_radix(addr_str, 16).ok()
}

fn normalize_whitespace(input: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_space = false;

    for ch in input.trim().chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
        } else {
            normalized.push(ch);
            last_was_space = false;
        }
    }

    normalized
}
