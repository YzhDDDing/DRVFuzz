use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

use crate::{
    riscv_impls_vec::{GeneratedTestCase, InstructionBlock},
    sd_model::{
        ExecutionState, StateTransition, TransitionAnalysis, unique_counted_modes,
        unique_counted_transitions,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuidanceStrategy {
    Transition,
    Mode,
}

impl GuidanceStrategy {
    pub fn label(self) -> &'static str {
        match self {
            GuidanceStrategy::Transition => "transition",
            GuidanceStrategy::Mode => "mode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GuidanceKey {
    Transition(StateTransition),
    Mode(ExecutionState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceRecord {
    pub strategy: GuidanceStrategy,
    pub total_items: usize,
    pub unique_items: usize,
    pub new_items: usize,
    pub visited_items: usize,
    pub seed_pool_size: usize,
}

#[derive(Debug, Clone)]
struct GuidedSeed {
    testcase: GeneratedTestCase,
    new_item_count: usize,
}

/// Shared SDModel guidance state. It keeps a global set of visited feedback
/// keys and a small seed pool of testcases that discovered new keys.
#[derive(Debug)]
pub struct GuidanceState {
    strategy: GuidanceStrategy,
    visited: BTreeSet<GuidanceKey>,
    seed_pool: VecDeque<GuidedSeed>,
    seed_pool_limit: usize,
    next_seed_index: usize,
}

impl GuidanceState {
    pub fn new(strategy: GuidanceStrategy, seed_pool_limit: usize) -> Self {
        Self {
            strategy,
            visited: BTreeSet::new(),
            seed_pool: VecDeque::new(),
            seed_pool_limit: seed_pool_limit.max(1),
            next_seed_index: 0,
        }
    }

    pub fn strategy(&self) -> GuidanceStrategy {
        self.strategy
    }

    pub fn select_seed(&mut self) -> Option<GeneratedTestCase> {
        if self.seed_pool.is_empty() {
            return None;
        }
        let idx = self.next_seed_index % self.seed_pool.len();
        self.next_seed_index = self.next_seed_index.wrapping_add(1);
        self.seed_pool.get(idx).map(|seed| seed.testcase.clone())
    }

    pub fn record_analysis(
        &mut self,
        testcase: &GeneratedTestCase,
        analysis: &TransitionAnalysis,
    ) -> GuidanceRecord {
        let total_items = match self.strategy {
            GuidanceStrategy::Transition => analysis.transitions.len(),
            GuidanceStrategy::Mode => analysis.states.len(),
        };
        let unique: Vec<GuidanceKey> = match self.strategy {
            GuidanceStrategy::Transition => unique_counted_transitions(&analysis.transitions)
                .into_iter()
                .map(GuidanceKey::Transition)
                .collect(),
            GuidanceStrategy::Mode => unique_counted_modes(&analysis.states)
                .into_iter()
                .map(GuidanceKey::Mode)
                .collect(),
        };

        let mut new_items = 0usize;
        for key in &unique {
            if self.visited.insert(key.clone()) {
                new_items += 1;
            }
        }

        if new_items > 0 {
            self.seed_pool.push_back(GuidedSeed {
                testcase: testcase.clone(),
                new_item_count: new_items,
            });
            self.trim_seed_pool();
        }

        GuidanceRecord {
            strategy: self.strategy,
            total_items,
            unique_items: unique.len(),
            new_items,
            visited_items: self.visited.len(),
            seed_pool_size: self.seed_pool.len(),
        }
    }

    fn trim_seed_pool(&mut self) {
        while self.seed_pool.len() > self.seed_pool_limit {
            let remove_idx = self
                .seed_pool
                .iter()
                .enumerate()
                .min_by_key(|(_, seed)| seed.new_item_count)
                .map(|(idx, _)| idx);
            if let Some(idx) = remove_idx {
                self.seed_pool.remove(idx);
            } else {
                break;
            }
        }
    }
}

/// Mix an SDModel-guided seed into a newly generated testcase.
/// The generated testcase keeps roughly the same test length: the selected
/// seed prefix replaces the same number of generated test instructions.
pub fn splice_guided_seed(
    generated: &mut GeneratedTestCase,
    seed: &GeneratedTestCase,
    window: usize,
) -> bool {
    if window == 0 {
        return false;
    }

    let mut changed = false;
    for (impl_ref, generated_block) in generated.test_insts.iter_mut() {
        let Some(seed_block) = seed.test_insts.get(impl_ref) else {
            continue;
        };
        if splice_block(generated_block, seed_block, window) {
            changed = true;
        }
    }
    changed
}

fn splice_block(generated: &mut InstructionBlock, seed: &InstructionBlock, window: usize) -> bool {
    if generated.len() == 0 || seed.len() == 0 {
        return false;
    }

    let take = window.min(generated.len()).min(seed.len());
    if take == 0 {
        return false;
    }

    let keep_generated = generated.len().saturating_sub(take);
    let mut next = InstructionBlock::new();
    next.extend_pairs(
        seed.iter_pairs()
            .take(take)
            .map(|(line, offset)| (line.clone(), *offset)),
    );
    next.extend_pairs(
        generated
            .iter_pairs()
            .take(keep_generated)
            .map(|(line, offset)| (line.clone(), *offset)),
    );
    *generated = next;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        extension_map::ExtensionMap,
        isa_base::ISABase,
        riscv_impls::RiscVImpl,
        riscv_impls_vec::{RandomConfigOverrides, TestCaseConfig},
    };
    use riscv_instruction_types::RegisterConfig;
    use std::collections::HashMap;

    #[test]
    fn splice_replaces_generated_prefix_with_seed_prefix() {
        let mut generated = testcase_with_lines(vec!["add x1, x2, x3", "sub x4, x5, x6"]);
        let seed = testcase_with_lines(vec!["div x1, x2, x3", "lw x4, 2(x5)"]);

        assert!(splice_guided_seed(&mut generated, &seed, 1));
        let lines = generated
            .test_insts
            .get(&RiscVImpl::Spike)
            .expect("spike block exists")
            .lines();
        assert_eq!(
            lines,
            ["div x1, x2, x3".to_string(), "add x1, x2, x3".to_string()]
        );
    }

    #[test]
    fn mode_guidance_records_modes_without_transition_order() {
        let testcase = testcase_with_lines(vec!["addi x1, x0, 0", "lui x2, 1"]);
        let first = analysis_with_states(vec![
            state("addi", &["boundary_value:int-zero"]),
            state("lui", &["boundary_value:int-one"]),
        ]);
        let second = analysis_with_states(vec![
            state("lui", &["boundary_value:int-one"]),
            state("addi", &["boundary_value:int-zero"]),
        ]);

        let mut guidance = GuidanceState::new(GuidanceStrategy::Mode, 4);
        let record = guidance.record_analysis(&testcase, &first);
        assert_eq!(record.strategy, GuidanceStrategy::Mode);
        assert_eq!(record.unique_items, 2);
        assert_eq!(record.new_items, 2);
        assert_eq!(record.seed_pool_size, 1);

        let record = guidance.record_analysis(&testcase, &second);
        assert_eq!(record.unique_items, 2);
        assert_eq!(record.new_items, 0);
        assert_eq!(record.seed_pool_size, 1);
    }

    #[test]
    fn transition_guidance_records_ordered_edges() {
        let testcase = testcase_with_lines(vec!["addi x1, x0, 0", "lui x2, 1"]);
        let first = analysis_with_states(vec![
            state("addi", &["boundary_value:int-zero"]),
            state("lui", &["boundary_value:int-one"]),
        ]);
        let second = analysis_with_states(vec![
            state("lui", &["boundary_value:int-one"]),
            state("addi", &["boundary_value:int-zero"]),
        ]);

        let mut guidance = GuidanceState::new(GuidanceStrategy::Transition, 4);
        let record = guidance.record_analysis(&testcase, &first);
        assert_eq!(record.strategy, GuidanceStrategy::Transition);
        assert_eq!(record.new_items, 1);
        assert_eq!(record.seed_pool_size, 1);

        let record = guidance.record_analysis(&testcase, &second);
        assert_eq!(record.new_items, 1);
        assert_eq!(record.seed_pool_size, 2);
    }

    fn analysis_with_states(states: Vec<ExecutionState>) -> TransitionAnalysis {
        let transitions = crate::sd_model::extract_transitions(&states);
        TransitionAnalysis {
            implementation: "spike".to_string(),
            summary: crate::sd_model::TransitionSummary {
                implementation: "spike".to_string(),
                total_states: states.len(),
                total_transitions: transitions.len(),
                unique_transitions: crate::sd_model::unique_counted_transitions(&transitions).len(),
            },
            states,
            transitions,
        }
    }

    fn state(opcode: &str, predicates: &[&str]) -> ExecutionState {
        ExecutionState {
            opcode: opcode.to_string(),
            predicates: predicates.iter().map(|item| item.to_string()).collect(),
        }
    }

    fn testcase_with_lines(lines: Vec<&str>) -> GeneratedTestCase {
        let mut test_insts = HashMap::new();
        test_insts.insert(
            RiscVImpl::Spike,
            InstructionBlock::from_parts(lines.into_iter().map(str::to_string).collect(), vec![]),
        );
        let mut init_insts = HashMap::new();
        init_insts.insert(RiscVImpl::Spike, InstructionBlock::new());
        let mut mem_range = HashMap::new();
        mem_range.insert(RiscVImpl::Spike, (0, 63));

        GeneratedTestCase {
            config: TestCaseConfig {
                isa_base: ISABase::Rv64,
                extension_insts_count: 0,
                extension_inst_scaling: None,
                mem_size: 64,
                mem_access_offset: (0, 0),
                test_register_config: RegisterConfig {
                    integer_register_range: (1, 31),
                    floating_point_register_range: (1, 31),
                    vector_register_range: (1, 31),
                },
                temp_register_range: (5, 7),
                data_sensitive_mode: false,
                data_sensitive_probability: 1.0,
                unaligned_access_required: None,
                random_config: RandomConfigOverrides::default(),
            },
            init_insts,
            test_insts,
            mem_range,
            extension_map: ExtensionMap {
                rv32: Vec::new(),
                rv64: Vec::new(),
            },
        }
    }
}
