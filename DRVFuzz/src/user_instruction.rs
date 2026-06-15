use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize, Hash)]
pub struct UserInstructionInfo {
    // Original instruction text before assembly
    pub instruction: String,
    // Disambiguator when multiple identical instructions exist pre-assembly
    pub same_index: usize,
}

impl UserInstructionInfo {
    pub fn build(user_insts: &[String]) -> Vec<Self> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut result = Vec::with_capacity(user_insts.len());
        for inst in user_insts {
            let instruction = inst.to_string();
            let counter = counts.entry(instruction.clone()).or_insert(0);
            let info = UserInstructionInfo {
                instruction,
                same_index: *counter,
            };
            *counter += 1;
            result.push(info);
        }
        result
    }
}
