use std::{
    collections::{BTreeSet, HashMap},
    convert::TryFrom,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use log::{info, warn};
use riscv_instruction::{
    csr_config_from_rv32_extensions, csr_config_from_rv64_extensions,
    separated_instructions::{RV32Extensions, RV64Extensions, RiscvInstruction},
};
use riscv_instruction_types::{
    CsrConfig, InstructionSequence, LoadImmediatePurpose, MemConfig, MemoryAccessInstruction,
    PseudoInstruction, RandomConfig, RegisterConfig,
};
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer},
};
use strum::IntoEnumIterator;

use crate::{
    data_sensitive::DataSensitiveInjector,
    error::{GenerationError, MemRangeError},
    execution_output::ExecutionContextOutput,
    extension_map::ExtensionMap,
    instruction::{
        GenerationOrder, generate_initialization_instructions_for_vec, generate_sequences_rv32,
        generate_sequences_rv64, remove_special_instructions,
    },
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
};

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, Hash)]
pub struct RiscVImplVec(Vec<RiscVImpl>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseConfig {
    pub isa_base: ISABase,
    pub extension_insts_count: usize,
    #[serde(default)]
    pub extension_inst_scaling: Option<ExtensionInstScaling>,
    pub mem_size: u64,
    #[serde(
        default = "default_mem_access_offset",
        deserialize_with = "deserialize_mem_access_offset"
    )]
    pub mem_access_offset: (i64, i64),
    pub test_register_config: RegisterConfig,
    pub temp_register_range: (u8, u8),
    #[serde(default)]
    pub data_sensitive_mode: bool,
    #[serde(default = "default_data_sensitive_probability")]
    pub data_sensitive_probability: f64,
    #[serde(default)]
    pub unaligned_access_required: Option<bool>,
    #[serde(default)]
    pub random_config: RandomConfigOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInstScaling {
    pub multiplier: f64,
    pub min_insts: usize,
    pub max_insts: usize,
}

impl ExtensionInstScaling {
    fn ensure_valid(&self) -> Result<(), GenerationError> {
        if self.min_insts > self.max_insts {
            return Err(GenerationError::InvalidExtensionScaling {
                min: self.min_insts,
                max: self.max_insts,
            });
        }
        Ok(())
    }

    fn apply(&self, base: usize) -> usize {
        let scaled = (base as f64 * self.multiplier).round();
        let scaled = scaled.clamp(0.0, usize::MAX as f64) as usize;
        scaled.clamp(self.min_insts, self.max_insts)
    }
}

const fn default_mem_access_offset() -> (i64, i64) {
    (0, 0)
}

fn default_data_sensitive_probability() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RandomConfigOverrides {
    #[serde(default)]
    pub capture_fflags: Option<bool>,
    #[serde(default)]
    pub csr_config: CsrConfigOverrides,
}

impl RandomConfigOverrides {
    fn apply(
        &self,
        default_capture_fflags: bool,
        default_csr_config: CsrConfig,
    ) -> (bool, CsrConfig) {
        let capture_fflags = self.capture_fflags.unwrap_or(default_capture_fflags);
        let csr_config = self.csr_config.apply(default_csr_config);
        (capture_fflags, csr_config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CsrConfigOverrides {
    pub enable_machine_csrs: Option<bool>,
    pub enable_supervisor_csrs: Option<bool>,
    pub enable_hypervisor_csrs: Option<bool>,
    pub enable_virtual_supervisor_csrs: Option<bool>,
    pub enable_rnmi_csrs: Option<bool>,
    pub enable_floating_csrs: Option<bool>,
    pub enable_vector_csrs: Option<bool>,
    pub enable_debug_csrs: Option<bool>,
}

impl CsrConfigOverrides {
    fn apply(&self, mut base: CsrConfig) -> CsrConfig {
        if let Some(enabled) = self.enable_machine_csrs {
            base.enable_machine_csrs = enabled;
        }
        if let Some(enabled) = self.enable_supervisor_csrs {
            base.enable_supervisor_csrs = enabled;
        }
        if let Some(enabled) = self.enable_hypervisor_csrs {
            base.enable_hypervisor_csrs = enabled;
        }
        if let Some(enabled) = self.enable_virtual_supervisor_csrs {
            base.enable_virtual_supervisor_csrs = enabled;
        }
        if let Some(enabled) = self.enable_rnmi_csrs {
            base.enable_rnmi_csrs = enabled;
        }
        if let Some(enabled) = self.enable_floating_csrs {
            base.enable_floating_csrs = enabled;
        }
        if let Some(enabled) = self.enable_vector_csrs {
            base.enable_vector_csrs = enabled;
        }
        if let Some(enabled) = self.enable_debug_csrs {
            base.enable_debug_csrs = enabled;
        }
        base
    }
}

fn deserialize_mem_access_offset<'de, D>(deserializer: D) -> Result<(i64, i64), D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OffsetRangeRepr {
        Single(i64),
        Pair([i64; 2]),
    }

    let repr = OffsetRangeRepr::deserialize(deserializer)?;
    let (min, max) = match repr {
        OffsetRangeRepr::Single(value) => (value, value),
        OffsetRangeRepr::Pair([start, end]) => (start, end),
    };

    if min > max {
        return Err(de::Error::custom(format!(
            "mem_access_offset lower bound {min} exceeds upper bound {max}"
        )));
    }

    Ok((min, max))
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq, Hash)]
pub struct InstructionBlock {
    pub lines: Vec<String>,
    #[serde(default)]
    pub mem_offsets: Vec<Option<i64>>,
}

impl InstructionBlock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_parts(lines: Vec<String>, mem_offsets: Vec<Option<i64>>) -> Self {
        let mut block = Self { lines, mem_offsets };
        block.align_lengths();
        block
    }

    pub fn push(&mut self, line: impl Into<String>, offset: Option<i64>) {
        self.lines.push(line.into());
        self.mem_offsets.push(offset);
        debug_assert_eq!(self.lines.len(), self.mem_offsets.len());
    }

    pub fn extend_pairs<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (String, Option<i64>)>,
    {
        for (line, offset) in iter {
            self.push(line, offset);
        }
    }

    pub fn extend_from(&mut self, other: &InstructionBlock) {
        self.lines.extend(other.lines.iter().cloned());
        self.mem_offsets.extend(other.mem_offsets.iter().cloned());
        debug_assert_eq!(self.lines.len(), self.mem_offsets.len());
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn offsets(&self) -> &[Option<i64>] {
        &self.mem_offsets
    }

    pub fn iter_pairs(&self) -> impl Iterator<Item = (&String, &Option<i64>)> {
        self.lines.iter().zip(self.mem_offsets.iter())
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    fn align_lengths(&mut self) {
        if self.mem_offsets.len() < self.lines.len() {
            self.mem_offsets.resize(self.lines.len(), None);
        } else if self.mem_offsets.len() > self.lines.len() {
            self.mem_offsets.truncate(self.lines.len());
        }
    }
}

trait ExtensionWithInstructionCount {
    fn total_instruction_count(&self) -> usize;
}

impl ExtensionWithInstructionCount for RV32Extensions {
    fn total_instruction_count(&self) -> usize {
        self.instruction_count()
    }
}

impl ExtensionWithInstructionCount for RV64Extensions {
    fn total_instruction_count(&self) -> usize {
        self.instruction_count()
    }
}

fn build_extension_instruction_counts<T>(
    extensions: &[T],
    scaling: Option<&ExtensionInstScaling>,
    fallback_count: usize,
) -> Result<HashMap<T, usize>, GenerationError>
where
    T: Clone + Eq + std::hash::Hash + ExtensionWithInstructionCount,
{
    if let Some(cfg) = scaling {
        cfg.ensure_valid()?;
        Ok(extensions
            .iter()
            .map(|ext| (ext.clone(), cfg.apply(ext.total_instruction_count())))
            .collect())
    } else {
        Ok(extensions
            .iter()
            .map(|ext| (ext.clone(), fallback_count))
            .collect())
    }
}

impl<'de> Deserialize<'de> for InstructionBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawBlock {
            #[serde(default)]
            lines: Vec<String>,
            #[serde(default)]
            mem_offsets: Vec<Option<i64>>,
        }

        let mut raw = RawBlock::deserialize(deserializer)?;
        if raw.mem_offsets.len() < raw.lines.len() {
            raw.mem_offsets.resize(raw.lines.len(), None);
        } else if raw.mem_offsets.len() > raw.lines.len() {
            raw.mem_offsets.truncate(raw.lines.len());
        }

        Ok(InstructionBlock {
            lines: raw.lines,
            mem_offsets: raw.mem_offsets,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTestCase {
    pub config: TestCaseConfig,
    pub init_insts: HashMap<RiscVImpl, InstructionBlock>,
    pub test_insts: HashMap<RiscVImpl, InstructionBlock>,
    pub mem_range: HashMap<RiscVImpl, (u64, u64)>,
    pub extension_map: ExtensionMap,
}

impl GeneratedTestCase {
    fn combined_block_of(&self, impl_ref: &RiscVImpl) -> Option<InstructionBlock> {
        let mut block = InstructionBlock::new();
        block.extend_from(self.init_insts.get(impl_ref)?);
        block.extend_from(self.test_insts.get(impl_ref)?);
        Some(block)
    }

    pub fn combined_insts(&self) -> HashMap<RiscVImpl, Vec<String>> {
        let mut combined = HashMap::new();
        for impl_ref in self.init_insts.keys() {
            if let Some(block) = self.combined_block_of(impl_ref) {
                combined.insert(*impl_ref, block.lines);
            }
        }
        combined
    }

    pub fn combined_instruction_offsets(&self) -> HashMap<RiscVImpl, Vec<Option<i64>>> {
        let mut combined = HashMap::new();
        for impl_ref in self.init_insts.keys() {
            if let Some(block) = self.combined_block_of(impl_ref) {
                combined.insert(*impl_ref, block.mem_offsets);
            }
        }
        combined
    }

    pub fn combined_insts_of(&self, impl_ref: &RiscVImpl) -> Option<Vec<String>> {
        self.combined_block_of(impl_ref).map(|block| block.lines)
    }

    pub fn combined_instruction_offsets_of(
        &self,
        impl_ref: &RiscVImpl,
    ) -> Option<Vec<Option<i64>>> {
        self.combined_block_of(impl_ref)
            .map(|block| block.mem_offsets)
    }

    pub fn without_test_indices(&self, remove: &BTreeSet<usize>) -> Self {
        if remove.is_empty() {
            return self.clone();
        }

        let mut new_case = self.clone();
        for block in new_case.test_insts.values_mut() {
            let mut next = InstructionBlock::new();
            for (idx, (line, offset)) in block.iter_pairs().enumerate() {
                if !remove.contains(&idx) {
                    next.push(line.clone(), *offset);
                }
            }
            *block = next;
        }
        new_case
    }
}

impl RiscVImplVec {
    pub fn from_impls<I>(impls: I) -> Self
    where
        I: IntoIterator<Item = RiscVImpl>,
    {
        let mut unique: Vec<RiscVImpl> = impls.into_iter().collect();
        unique.sort_unstable();
        unique.dedup();
        RiscVImplVec(unique)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RiscVImpl> {
        self.0.iter()
    }

    pub fn all(isa_base: ISABase) -> Self {
        match isa_base {
            ISABase::Rv32 => RiscVImplVec(
                RiscVImpl::iter()
                    .filter(|r| r.supported_isa_bases().contains(&ISABase::Rv32))
                    .collect(),
            ),
            ISABase::Rv64 => RiscVImplVec(
                RiscVImpl::iter()
                    .filter(|r| r.supported_isa_bases().contains(&ISABase::Rv64))
                    .collect(),
            ),
        }
    }

    pub fn filter_by_unaligned_access_requirement(
        &self,
        requirement: Option<bool>,
        overrides: &HashMap<RiscVImpl, bool>,
    ) -> Self {
        if let Some(required) = requirement {
            let filtered: Vec<RiscVImpl> = self
                .0
                .iter()
                .copied()
                .filter(|impl_ref| impl_ref.supports_unaligned_requirement(required, overrides))
                .collect();
            RiscVImplVec(filtered)
        } else {
            self.clone()
        }
    }

    pub fn extension_map(&self) -> ExtensionMap {
        if self.0.is_empty() {
            return ExtensionMap {
                rv32: Vec::new(),
                rv64: Vec::new(),
            };
        }

        let first_ext = self.0[0].extension_map();
        let mut rv32_intersection = first_ext.rv32;
        let mut rv64_intersection = first_ext.rv64;

        for impl_ref in &self.0[1..] {
            let ext = impl_ref.extension_map();
            rv32_intersection = rv32_intersection
                .into_iter()
                .filter(|e| ext.rv32.contains(e))
                .collect();
            rv64_intersection = rv64_intersection
                .into_iter()
                .filter(|e| ext.rv64.contains(e))
                .collect();
        }

        ExtensionMap {
            rv32: rv32_intersection,
            rv64: rv64_intersection,
        }
    }

    /// Get the smallest available user-memory size among all implementations.
    ///
    /// Returns the size in bytes, not an address range.
    pub fn min_user_mem_size(&self) -> Result<u64, MemRangeError> {
        self.0
            .iter()
            .map(|impl_ref| {
                // user_mem_range() returns an inclusive range [start, end]
                let (start, end) = impl_ref.user_mem_range();
                // Inclusive size is end - start + 1, but implementations return end as the first byte after the region, so use end - start.
                end - start
            })
            .min()
            .ok_or(MemRangeError::NoImplementations)
    }

    /// For a given ISA base and requested memory size, compute a valid memory range for each implementation.
    ///
    /// # Requirements
    /// 1. Requested memory size must not exceed the smallest available size across implementations.
    /// 2. Requested size must be word-aligned (a multiple of word_size).
    /// 3. Requested size must be at least the maximum instruction access width for the ISA base.
    ///
    /// # Guarantees
    /// 1. All implementations receive the same memory size.
    /// 2. Ranges are aligned and valid.
    /// 3. Ranges are large enough to run any instruction.
    ///
    /// # Note
    /// Callers must keep accesses within the range. For example, if the range is [0x80000000, 0x8000001F] (32 bytes),
    /// then in `LW x1, 0(x2)` the value of x2 should be within [0x80000000, 0x8000001C] so that
    /// 0x8000001C + 4 = 0x80000020 does not overflow. MemConfig's register_ranges enforce this.
    ///
    /// # Return value
    /// HashMap values are inclusive ranges [start, end]; both start and end are valid addresses.
    pub fn mem_range(
        &self,
        isa_base: ISABase,
        mem_size: u64,
    ) -> Result<HashMap<RiscVImpl, (u64, u64)>, MemRangeError> {
        // Ensure requested size does not exceed the minimum available across implementations
        let min_mem_size = self.min_user_mem_size()?;
        if mem_size > min_mem_size {
            return Err(MemRangeError::MemorySizeExceedsAvailable {
                requested: mem_size,
                available: min_mem_size,
            });
        }

        // Ensure requested size is word-aligned (multiple of word_size)
        let word_size = isa_base.word_size() as u64;
        if mem_size % word_size != 0 {
            return Err(MemRangeError::MemoryNotWordAligned {
                size: mem_size,
                word_size,
            });
        }

        // Ensure requested size is at least the maximum instruction access width for this ISA base
        let max_access_width = isa_base.instruction_max_access_width() as u64;
        if mem_size < max_access_width {
            return Err(MemRangeError::MemorySizeTooSmall {
                size: mem_size,
                width: max_access_width,
            });
        }

        let mut result = HashMap::new();
        for impl_ref in &self.0 {
            let (_start, end) = impl_ref.user_mem_range();
            // Allocate mem_size bytes from the end of the implementation's memory, aligning to the word boundary
            let unaligned_offset = end - mem_size;
            let aligned_offset = (unaligned_offset / word_size) * word_size;

            // Compute the inclusive end address: start + mem_size - 1
            // Example: start=0x80000000, mem_size=32 => end=0x8000001F (32 bytes: 0x00 to 0x1F)
            let mem_end = aligned_offset + mem_size - 1;

            // Store the inclusive range [aligned_offset, mem_end]
            result.insert(*impl_ref, (aligned_offset, mem_end));
        }

        // Ensure all ranges have equal size (inclusive size: end - start + 1)
        for (start, end) in result.values() {
            let actual_size = end - start + 1;
            if actual_size != mem_size {
                return Err(MemRangeError::MemoryRangeSizeMismatch {
                    actual: actual_size,
                    expected: mem_size,
                });
            }
        }

        Ok(result)
    }

    pub fn generate_random_testcase(
        &self,
        testcase_config: TestCaseConfig,
    ) -> Result<GeneratedTestCase, GenerationError> {
        let extension_map = self.extension_map();
        let mut rng = rand::rng();

        let mem_range = self.mem_range(testcase_config.isa_base, testcase_config.mem_size)?;

        let max_access_width = testcase_config.isa_base.instruction_max_access_width() as u64;
        let (offset_min, offset_max) = testcase_config.mem_access_offset;
        let register_min = if offset_min < 0 {
            let neg = offset_min.checked_neg().ok_or_else(|| {
                GenerationError::InvalidMemAccessOffset {
                    min: offset_min,
                    max: offset_max,
                    mem_size: testcase_config.mem_size,
                    width: max_access_width,
                }
            })?;
            u64::try_from(neg).map_err(|_| GenerationError::InvalidMemAccessOffset {
                min: offset_min,
                max: offset_max,
                mem_size: testcase_config.mem_size,
                width: max_access_width,
            })?
        } else {
            0
        };
        let mem_upper_limit = testcase_config.mem_size.checked_sub(1).ok_or_else(|| {
            GenerationError::InvalidMemAccessOffset {
                min: offset_min,
                max: offset_max,
                mem_size: testcase_config.mem_size,
                width: max_access_width,
            }
        })?;
        let access_limit_i128 = i128::from(testcase_config.mem_size)
            - i128::from(max_access_width)
            - i128::from(offset_max);
        if access_limit_i128 < 0 {
            return Err(GenerationError::InvalidMemAccessOffset {
                min: offset_min,
                max: offset_max,
                mem_size: testcase_config.mem_size,
                width: max_access_width,
            });
        }
        let register_max_i128 = access_limit_i128.min(i128::from(mem_upper_limit));
        if register_max_i128 < i128::from(register_min) {
            return Err(GenerationError::InvalidMemAccessOffset {
                min: offset_min,
                max: offset_max,
                mem_size: testcase_config.mem_size,
                width: max_access_width,
            });
        }
        let register_max = register_max_i128 as u64;

        let default_capture_fflags = match testcase_config.isa_base {
            ISABase::Rv32 => extension_map.rv32.contains(&RV32Extensions::Zicsr),
            ISABase::Rv64 => extension_map.rv64.contains(&RV64Extensions::Zicsr),
        };

        let default_csr_config = match testcase_config.isa_base {
            ISABase::Rv32 => csr_config_from_rv32_extensions(&extension_map.rv32),
            ISABase::Rv64 => csr_config_from_rv64_extensions(&extension_map.rv64),
        };
        let (capture_fflags, csr_config) = testcase_config
            .random_config
            .apply(default_capture_fflags, default_csr_config);

        let mem_config = MemConfig {
            // register_ranges: inclusive [min, max], ensuring register/immediate combinations stay within configured memory
            register_ranges: (register_min, register_max),

            // immediate_ranges: inclusive [min, max], allowing random offsets within the configured range
            immediate_ranges: (offset_min, offset_max),

            // register_number_range: inclusive [5, 7], the temporary register index range
            register_number_range: testcase_config.temp_register_range,
        };

        let config = RandomConfig::new()
            .with_mem_config(mem_config)
            .with_register_config(testcase_config.test_register_config.clone())
            .with_capture_fflags(capture_fflags)
            .with_csr_config(csr_config);

        match testcase_config.isa_base {
            ISABase::Rv32 => {
                info!("Extensions for RV32: {:?}", extension_map.rv32);
                let counts = build_extension_instruction_counts(
                    &extension_map.rv32,
                    testcase_config.extension_inst_scaling.as_ref(),
                    testcase_config.extension_insts_count,
                )?;

                let mut data_sensitive = if testcase_config.data_sensitive_mode {
                    match DataSensitiveInjector::with_isa_plan(
                        testcase_config.temp_register_range,
                        testcase_config.data_sensitive_probability,
                        testcase_config.isa_base,
                    ) {
                        Ok(injector) => Some(injector),
                        Err(err) => {
                            warn!(
                                "data-sensitive mode disabled because temp range {:?} is invalid: {}",
                                testcase_config.temp_register_range, err
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                let mut insts = generate_sequences_rv32(
                    &counts,
                    GenerationOrder::RandomShuffle,
                    &mut rng,
                    &config,
                    data_sensitive.as_mut(),
                )?;

                insts = remove_special_instructions(insts);

                let init_insts = generate_initialization_instructions_for_vec(
                    &self,
                    testcase_config.isa_base,
                    &mem_range,
                    &testcase_config.test_register_config,
                    testcase_config.temp_register_range,
                )?;

                let mut test_insts_map = HashMap::new();
                for impl_ref in &self.0 {
                    let (start, _end) = mem_range.get(impl_ref).copied().ok_or_else(|| {
                        GenerationError::MemRangeNotFound {
                            impl_name: format!("{:?}", impl_ref),
                        }
                    })?;
                    let mem_offset = i64::try_from(start)
                        .map_err(|_| GenerationError::MemoryAddressOutOfRange { addr: start })?;

                    let mut block = InstructionBlock::new();
                    for seq in &insts {
                        let mut inst = seq.clone();
                        offset_mem_base_immediates(&mut inst, mem_offset);
                        block.extend_pairs(sequence_to_instruction_lines(&inst));
                    }

                    test_insts_map.insert(impl_ref.clone(), block);
                }

                Ok(GeneratedTestCase {
                    config: testcase_config,
                    init_insts,
                    test_insts: test_insts_map,
                    mem_range,
                    extension_map: extension_map.clone(),
                })
            }
            ISABase::Rv64 => {
                info!("Extensions for RV64: {:?}", extension_map.rv64);
                let counts = build_extension_instruction_counts(
                    &extension_map.rv64,
                    testcase_config.extension_inst_scaling.as_ref(),
                    testcase_config.extension_insts_count,
                )?;

                let mut data_sensitive = if testcase_config.data_sensitive_mode {
                    match DataSensitiveInjector::with_isa_plan(
                        testcase_config.temp_register_range,
                        testcase_config.data_sensitive_probability,
                        testcase_config.isa_base,
                    ) {
                        Ok(injector) => Some(injector),
                        Err(err) => {
                            warn!(
                                "data-sensitive mode disabled because temp range {:?} is invalid: {}",
                                testcase_config.temp_register_range, err
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                let mut insts = generate_sequences_rv64(
                    &counts,
                    GenerationOrder::RandomShuffle,
                    &mut rng,
                    &config,
                    data_sensitive.as_mut(),
                )?;

                insts = remove_special_instructions(insts);

                let init_insts = generate_initialization_instructions_for_vec(
                    &self,
                    testcase_config.isa_base,
                    &mem_range,
                    &testcase_config.test_register_config,
                    testcase_config.temp_register_range,
                )?;

                let mut test_insts_map = HashMap::new();
                for impl_ref in &self.0 {
                    let (start, _end) = mem_range.get(impl_ref).copied().ok_or_else(|| {
                        GenerationError::MemRangeNotFound {
                            impl_name: format!("{:?}", impl_ref),
                        }
                    })?;
                    let mem_offset = i64::try_from(start)
                        .map_err(|_| GenerationError::MemoryAddressOutOfRange { addr: start })?;

                    let mut block = InstructionBlock::new();
                    for seq in &insts {
                        let mut inst = seq.clone();
                        offset_mem_base_immediates(&mut inst, mem_offset);
                        block.extend_pairs(sequence_to_instruction_lines(&inst));
                    }

                    test_insts_map.insert(impl_ref.clone(), block);
                }

                Ok(GeneratedTestCase {
                    config: testcase_config,
                    init_insts,
                    test_insts: test_insts_map,
                    mem_range,
                    extension_map,
                })
            }
        }
    }

    pub fn execute<P: AsRef<Path>>(
        &self,
        testcase: &GeneratedTestCase,
        run_dir: P,
        timeouts: Option<&HashMap<RiscVImpl, Duration>>,
    ) -> Result<HashMap<RiscVImpl, ExecutionContextOutput>, Box<dyn std::error::Error>> {
        let mut result = HashMap::new();
        let combined = testcase.combined_insts();
        let combined_offsets = testcase.combined_instruction_offsets();
        let run_root = run_dir.as_ref();
        let mut durations: Vec<(RiscVImpl, Duration)> = Vec::new();
        for (impl_ref, insts) in &combined {
            let dir = run_root.join(impl_ref.to_string());
            let timeout = timeouts.and_then(|map| map.get(impl_ref).copied());
            let start = Instant::now();
            let allow_unaligned = testcase
                .config
                .unaligned_access_required
                .unwrap_or_else(|| impl_ref.default_unaligned_access_support());
            let mut output = impl_ref.execute_with_extension_override(
                dir,
                testcase.config.isa_base,
                insts,
                timeout,
                Some(&testcase.extension_map),
                allow_unaligned,
            )?;
            let elapsed = start.elapsed();
            let range = testcase.mem_range.get(impl_ref).copied().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Memory range not found for implementation: {:?}", impl_ref),
                )
            })?;
            output.normalize(
                range,
                &testcase.config.test_register_config,
                testcase.config.temp_register_range,
            )?;
            let offsets = combined_offsets.get(impl_ref).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "instruction offset metadata missing for implementation {:?}",
                        impl_ref
                    ),
                )
            })?;
            let context_output =
                ExecutionContextOutput::from_execution_output(output, range, insts, offsets)?;
            result.insert(impl_ref.clone(), context_output);
            durations.push((impl_ref.clone(), elapsed));
        }

        if !durations.is_empty() {
            durations.sort_by_key(|(impl_ref, _)| impl_ref.clone());
            let mut report = String::from(
                "# Execution Timings\n\n| Implementation | Duration (s) |\n| --- | --- |\n",
            );
            for (impl_ref, duration) in durations {
                report.push_str(&format!(
                    "| {} | {:.3} |\n",
                    impl_ref,
                    duration.as_secs_f64()
                ));
            }
            let timing_path = run_root.join("execution_timings.md");
            fs::write(timing_path, report)?;
        }

        Ok(result)
    }
}

fn offset_mem_base_immediates<T>(sequence: &mut InstructionSequence<T>, offset: i64) {
    if offset == 0 {
        return;
    }

    adjust_base_address_immediates(&mut sequence.pre_instructions, offset);
}

fn adjust_base_address_immediates(pseudos: &mut [PseudoInstruction], offset: i64) {
    for pseudo in pseudos {
        if let PseudoInstruction::LoadImmediate {
            immediate, purpose, ..
        } = pseudo
        {
            if *purpose == LoadImmediatePurpose::BaseAddress {
                *immediate = immediate.saturating_add(offset);
            }
        }
    }
}

fn sequence_to_instruction_lines(
    sequence: &InstructionSequence<RiscvInstruction>,
) -> Vec<(String, Option<i64>)> {
    let mut lines = Vec::new();
    for pseudo in &sequence.pre_instructions {
        if let PseudoInstruction::Comment(text) = pseudo {
            append_inline_comment(&mut lines, text);
            continue;
        }
        lines.push((pseudo.to_string(), None));
    }

    lines.push((
        sequence.instruction.to_string(),
        sequence.instruction.offset_operand_value(),
    ));

    for pseudo in &sequence.post_instructions {
        if let PseudoInstruction::Comment(text) = pseudo {
            append_inline_comment(&mut lines, text);
            continue;
        }
        lines.push((pseudo.to_string(), None));
    }

    lines
}

fn append_inline_comment(lines: &mut Vec<(String, Option<i64>)>, text: &str) {
    if let Some((line, _)) = lines.last_mut() {
        line.push_str(" # ");
        line.push_str(text);
    } else {
        lines.push((format!(" # {}", text), None));
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use riscv_instruction_types::{IntegerRegister, ValidatedValue};

    fn li(rd: u8, immediate: i64, purpose: LoadImmediatePurpose) -> PseudoInstruction {
        PseudoInstruction::LoadImmediate {
            rd: IntegerRegister::new(rd).expect("valid register"),
            immediate,
            purpose,
        }
    }

    #[test]
    fn offsets_only_base_address_li() {
        let mut seq = InstructionSequence::with_full_instructions(
            vec![
                li(5, 10, LoadImmediatePurpose::BaseAddress),
                li(6, 20, LoadImmediatePurpose::Generic),
            ],
            String::from("sw x1, 0(x5)"),
            vec![li(7, 30, LoadImmediatePurpose::BaseAddress)],
        );

        offset_mem_base_immediates(&mut seq, 0x100);

        assert_eq!(
            seq.pre_instructions[0],
            li(5, 0x100 + 10, LoadImmediatePurpose::BaseAddress)
        );
        assert_eq!(
            seq.pre_instructions[1],
            li(6, 20, LoadImmediatePurpose::Generic)
        );
        assert_eq!(
            seq.post_instructions[0],
            li(7, 30, LoadImmediatePurpose::BaseAddress)
        );
    }

    #[test]
    fn handles_zero_offset_without_change() {
        let mut seq = InstructionSequence::with_pre_instructions(
            vec![li(2, 0x200, LoadImmediatePurpose::BaseAddress)],
            String::from("sd x3, 0(sp)"),
        );

        offset_mem_base_immediates(&mut seq, 0);

        assert_eq!(
            seq.pre_instructions[0],
            li(2, 0x200, LoadImmediatePurpose::BaseAddress)
        );
    }

    #[test]
    fn ignores_non_baseaddress_purpose() {
        let mut seq = InstructionSequence::with_pre_instructions(
            vec![li(8, 123, LoadImmediatePurpose::CsrValue)],
            String::from("csrrw x1, mstatus, x8"),
        );

        offset_mem_base_immediates(&mut seq, 0x10);

        assert_eq!(
            seq.pre_instructions[0],
            li(8, 123, LoadImmediatePurpose::CsrValue)
        );
    }

    #[test]
    fn random_overrides_apply_defaults_and_overrides() {
        let base_csr = CsrConfig {
            enable_machine_csrs: false,
            enable_supervisor_csrs: false,
            enable_hypervisor_csrs: false,
            enable_virtual_supervisor_csrs: false,
            enable_rnmi_csrs: false,
            enable_floating_csrs: false,
            enable_vector_csrs: false,
            enable_debug_csrs: false,
        };
        let overrides = RandomConfigOverrides {
            capture_fflags: Some(false),
            csr_config: CsrConfigOverrides {
                enable_machine_csrs: Some(true),
                enable_debug_csrs: Some(true),
                ..Default::default()
            },
        };

        let (capture_fflags, csr_config) = overrides.apply(true, base_csr);

        assert!(!capture_fflags);
        assert!(csr_config.enable_machine_csrs);
        assert!(csr_config.enable_debug_csrs);
        assert!(!csr_config.enable_supervisor_csrs);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::execution_output::generate_execution_context_report;
    use std::{collections::HashMap, env::current_dir};

    #[test]
    fn test_all_rv32() {
        let overrides = HashMap::new();
        let riscv_impl_vec = RiscVImplVec::all(ISABase::Rv32)
            .filter_by_unaligned_access_requirement(Some(false), &overrides);
        let testcase = riscv_impl_vec
            .generate_random_testcase(TestCaseConfig {
                isa_base: ISABase::Rv32,
                extension_insts_count: 100,
                extension_inst_scaling: None,
                mem_size: 32,
                mem_access_offset: (0, 0),
                test_register_config: RegisterConfig {
                    integer_register_range: (16, 25),
                    floating_point_register_range: (16, 25),
                    vector_register_range: (16, 25),
                },
                temp_register_range: (10, 15),
                data_sensitive_mode: false,
                data_sensitive_probability: 1.0,
                unaligned_access_required: Some(false),
                random_config: RandomConfigOverrides::default(),
            })
            .unwrap();

        let run_dir = current_dir().unwrap().join("temp").join("test_all_rv32");
        let execution_output = riscv_impl_vec
            .execute(&testcase, run_dir.clone(), None)
            .unwrap();
        for (r_i, e_o) in execution_output {
            let md_file = run_dir.join(r_i.to_string()).with_extension("md");
            if let Some(insts) = testcase.combined_insts_of(&r_i) {
                generate_execution_context_report(&e_o, md_file, &insts).unwrap();
            }
        }
    }

    #[test]
    fn test_all_rv64() {
        let overrides = HashMap::new();
        let riscv_impl_vec = RiscVImplVec::all(ISABase::Rv64)
            .filter_by_unaligned_access_requirement(Some(false), &overrides);
        let testcase = riscv_impl_vec
            .generate_random_testcase(TestCaseConfig {
                isa_base: ISABase::Rv64,
                extension_insts_count: 100,
                extension_inst_scaling: None,
                mem_size: 64,
                mem_access_offset: (0, 0),
                test_register_config: RegisterConfig {
                    integer_register_range: (16, 25),
                    floating_point_register_range: (16, 25),
                    vector_register_range: (16, 25),
                },
                temp_register_range: (10, 15),
                data_sensitive_mode: false,
                data_sensitive_probability: 1.0,
                unaligned_access_required: Some(false),
                random_config: RandomConfigOverrides::default(),
            })
            .unwrap();

        let run_dir = current_dir().unwrap().join("temp").join("test_all_rv64");
        let execution_output = riscv_impl_vec
            .execute(&testcase, run_dir.clone(), None)
            .unwrap();
        for (r_i, e_o) in execution_output {
            let md_file = run_dir.join(r_i.to_string()).with_extension("md");
            if let Some(insts) = testcase.combined_insts_of(&r_i) {
                generate_execution_context_report(&e_o, md_file, &insts).unwrap();
            }
        }
    }

    #[test]
    fn print_pairwise_extension_supersets() {
        use std::collections::HashSet;
        use strum::IntoEnumIterator;

        let all_impls: Vec<RiscVImpl> = RiscVImpl::iter().collect();

        for isa in [ISABase::Rv32, ISABase::Rv64] {
            let supporting: Vec<RiscVImpl> = all_impls
                .iter()
                .copied()
                .filter(|impl_ref| impl_ref.supported_isa_bases().contains(&isa))
                .collect();

            if supporting.len() < 2 {
                continue;
            }

            match isa {
                ISABase::Rv32 => {
                    let baseline_map = RiscVImplVec::from_impls(supporting.clone()).extension_map();
                    let baseline_vec = baseline_map.rv32;
                    let baseline_len = baseline_vec.len();
                    let baseline_set: HashSet<_> = baseline_vec.iter().copied().collect();

                    for (idx, left) in supporting.iter().enumerate() {
                        for right in supporting.iter().skip(idx + 1) {
                            let pair_map =
                                RiscVImplVec::from_impls(vec![*left, *right]).extension_map();
                            let pair_vec = pair_map.rv32;

                            if pair_vec.len() <= baseline_len {
                                continue;
                            }

                            let pair_set: HashSet<_> = pair_vec.iter().copied().collect();
                            let extras: Vec<_> =
                                pair_set.difference(&baseline_set).copied().collect();

                            if extras.is_empty() {
                                continue;
                            }

                            let shared = {
                                let mut shared: Vec<_> =
                                    pair_set.iter().map(|ext| format!("{:?}", ext)).collect();
                                shared.sort();
                                shared.join(", ")
                            };
                            let extras = {
                                let mut extras: Vec<_> =
                                    extras.into_iter().map(|ext| format!("{:?}", ext)).collect();
                                extras.sort();
                                extras.join(", ")
                            };

                            info!(
                                "[rv32] {} + {} -> {} shared extensions: [{}]; extra vs baseline: [{}]",
                                left,
                                right,
                                pair_set.len(),
                                shared,
                                extras
                            );
                        }
                    }
                }
                ISABase::Rv64 => {
                    let baseline_map = RiscVImplVec::from_impls(supporting.clone()).extension_map();
                    let baseline_vec = baseline_map.rv64;
                    let baseline_len = baseline_vec.len();
                    let baseline_set: HashSet<_> = baseline_vec.iter().copied().collect();

                    for (idx, left) in supporting.iter().enumerate() {
                        for right in supporting.iter().skip(idx + 1) {
                            let pair_map =
                                RiscVImplVec::from_impls(vec![*left, *right]).extension_map();
                            let pair_vec = pair_map.rv64;

                            if pair_vec.len() <= baseline_len {
                                continue;
                            }

                            let pair_set: HashSet<_> = pair_vec.iter().copied().collect();
                            let extras: Vec<_> =
                                pair_set.difference(&baseline_set).copied().collect();

                            if extras.is_empty() {
                                continue;
                            }

                            let shared = {
                                let mut shared: Vec<_> =
                                    pair_set.iter().map(|ext| format!("{:?}", ext)).collect();
                                shared.sort();
                                shared.join(", ")
                            };
                            let extras = {
                                let mut extras: Vec<_> =
                                    extras.into_iter().map(|ext| format!("{:?}", ext)).collect();
                                extras.sort();
                                extras.join(", ")
                            };

                            info!(
                                "[rv64] {} + {} -> {} shared extensions: [{}]; extra vs baseline: [{}]",
                                left,
                                right,
                                pair_set.len(),
                                shared,
                                extras
                            );
                        }
                    }
                }
            }
        }
    }
}
