//! RISC-V instruction types and register utilities

extern crate self as riscv_instruction_types;

use std::fmt::{self, Display};

use rand::seq::IndexedRandom as _;
use riscv_instruction_macros::{DeriveRandom, DeriveValidatedValue};
use crate::data_sensitive::{DataSensitiveConfig, FloatFormat};
use serde::{Deserialize, Serialize};

mod csr;
pub mod data_sensitive;
pub use csr::{ReadableCsr, WritableCsr};

/// Configuration for register constraints in random value generation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegisterConfig {
    /// Range for integer registers (x0-x31)
    pub integer_register_range: (u8, u8),
    /// Range for floating-point registers (f0-f31)
    pub floating_point_register_range: (u8, u8),
    /// Range for vector registers (v0-v31)
    pub vector_register_range: (u8, u8),
}

impl RegisterConfig {
    /// Create a new RegisterConfig with default values (full ranges)
    pub fn new() -> Self {
        Self {
            integer_register_range: (0, 31),
            floating_point_register_range: (0, 31),
            vector_register_range: (0, 31),
        }
    }

    /// Set integer register range
    pub fn with_integer_register_range(mut self, start: u8, end: u8) -> Self {
        self.integer_register_range = (start, end);
        self
    }

    /// Set floating-point register range
    pub fn with_floating_point_register_range(mut self, start: u8, end: u8) -> Self {
        self.floating_point_register_range = (start, end);
        self
    }

    /// Set vector register range
    pub fn with_vector_register_range(mut self, start: u8, end: u8) -> Self {
        self.vector_register_range = (start, end);
        self
    }
}

/// Configuration for memory-related constraints in random value generation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemConfig {
    /// Ranges for integer register values (e.g., "xs1" -> (0x1000, 0x2000))
    pub register_ranges: (u64, u64),
    /// Ranges for immediate/offset values (e.g., "imm" -> (-100, 100))
    pub immediate_ranges: (i64, i64),
    /// Ranges for register numbers used in memory instructions (e.g., x1-x31)
    pub register_number_range: (u8, u8),
}

impl MemConfig {
    /// Create a new MemConfig with default values
    pub fn new() -> Self {
        Self {
            register_ranges: (0x10000000, 0x20000000), // Default memory address range
            immediate_ranges: (-1000, 1000),           // Default immediate range
            register_number_range: (5, 7),             // Default to all integer registers
        }
    }

    /// Set register ranges
    pub fn with_register_ranges(mut self, start: u64, end: u64) -> Self {
        self.register_ranges = (start, end);
        self
    }

    /// Set immediate ranges
    pub fn with_immediate_ranges(mut self, start: i64, end: i64) -> Self {
        self.immediate_ranges = (start, end);
        self
    }

    /// Set register number range for memory instructions
    pub fn with_register_number_range(mut self, start: u8, end: u8) -> Self {
        self.register_number_range = (start, end);
        self
    }
}

/// CSR enable switches tied to privilege/extension domains.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CsrConfig {
    /// Machine-level CSRs (mstatus/misa/mcounteren/cycle/time/...)
    #[serde(default = "CsrConfig::default_machine_csrs")]
    pub enable_machine_csrs: bool,
    /// Supervisor/H-extension CSRs (satp/sstatus/sie/...)
    #[serde(default = "CsrConfig::default_supervisor_csrs")]
    pub enable_supervisor_csrs: bool,
    /// Hypervisor HS-level CSRs (hstatus/hedeleg/hgatp/...)
    #[serde(default = "CsrConfig::default_hypervisor_csrs")]
    pub enable_hypervisor_csrs: bool,
    /// Virtual Supervisor VS-level CSRs (vsstatus/vsatp/...)
    #[serde(default = "CsrConfig::default_virtual_supervisor_csrs")]
    pub enable_virtual_supervisor_csrs: bool,
    /// RNMI / Smrnmi CSRs (mnstatus/mnscratch/...)
    #[serde(default = "CsrConfig::default_rnmi_csrs")]
    pub enable_rnmi_csrs: bool,
    /// Floating-point CSRs (fflags/frm/fcsr)
    #[serde(default = "CsrConfig::default_floating_csrs")]
    pub enable_floating_csrs: bool,
    /// Vector CSRs (vtype/vl/vlenb)
    #[serde(default = "CsrConfig::default_vector_csrs")]
    pub enable_vector_csrs: bool,
    /// Debug-related CSRs (dcsr)
    #[serde(default = "CsrConfig::default_debug_csrs")]
    pub enable_debug_csrs: bool,
}

impl CsrConfig {
    pub fn new() -> Self {
        Self::default()
    }

    const fn default_machine_csrs() -> bool {
        true
    }

    const fn default_supervisor_csrs() -> bool {
        true
    }

    const fn default_hypervisor_csrs() -> bool {
        true
    }

    const fn default_virtual_supervisor_csrs() -> bool {
        true
    }

    const fn default_rnmi_csrs() -> bool {
        true
    }

    const fn default_floating_csrs() -> bool {
        true
    }

    const fn default_vector_csrs() -> bool {
        true
    }

    const fn default_debug_csrs() -> bool {
        true
    }

    pub fn with_machine_csrs(mut self, enable: bool) -> Self {
        self.enable_machine_csrs = enable;
        self
    }

    pub fn with_supervisor_csrs(mut self, enable: bool) -> Self {
        self.enable_supervisor_csrs = enable;
        self
    }

    pub fn with_hypervisor_csrs(mut self, enable: bool) -> Self {
        self.enable_hypervisor_csrs = enable;
        self
    }

    pub fn with_virtual_supervisor_csrs(mut self, enable: bool) -> Self {
        self.enable_virtual_supervisor_csrs = enable;
        self
    }

    pub fn with_rnmi_csrs(mut self, enable: bool) -> Self {
        self.enable_rnmi_csrs = enable;
        self
    }

    pub fn with_floating_csrs(mut self, enable: bool) -> Self {
        self.enable_floating_csrs = enable;
        self
    }

    pub fn with_vector_csrs(mut self, enable: bool) -> Self {
        self.enable_vector_csrs = enable;
        self
    }

    pub fn with_debug_csrs(mut self, enable: bool) -> Self {
        self.enable_debug_csrs = enable;
        self
    }
}

impl Default for CsrConfig {
    fn default() -> Self {
        Self {
            enable_machine_csrs: Self::default_machine_csrs(),
            enable_supervisor_csrs: Self::default_supervisor_csrs(),
            enable_hypervisor_csrs: Self::default_hypervisor_csrs(),
            enable_virtual_supervisor_csrs: Self::default_virtual_supervisor_csrs(),
            enable_rnmi_csrs: Self::default_rnmi_csrs(),
            enable_floating_csrs: Self::default_floating_csrs(),
            enable_vector_csrs: Self::default_vector_csrs(),
            enable_debug_csrs: Self::default_debug_csrs(),
        }
    }
}

/// Configuration for constraining random value generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomConfig {
    /// Memory configuration for memory-related constraints
    #[serde(default)]
    pub mem_config: MemConfig,
    /// Register configuration for register range constraints
    #[serde(default)]
    pub register_config: RegisterConfig,
    /// Whether to append fflags-capturing post instructions after floating ops
    #[serde(default = "RandomConfig::default_capture_fflags")]
    pub capture_fflags: bool,
    /// CSR enable switches per privilege domain
    #[serde(default)]
    pub csr_config: CsrConfig,
    /// Data-sensitive generation settings
    #[serde(default)]
    pub data_sensitive: DataSensitiveConfig,
}

impl RandomConfig {
    /// Create a new RandomConfig with default values
    pub fn new() -> Self {
        Self {
            mem_config: MemConfig::new(),
            register_config: RegisterConfig::new(),
            capture_fflags: true,
            csr_config: CsrConfig::default(),
            data_sensitive: DataSensitiveConfig::default(),
        }
    }

    /// Set memory configuration
    pub fn with_mem_config(mut self, mem_config: MemConfig) -> Self {
        self.mem_config = mem_config;
        self
    }

    /// Set register configuration
    pub fn with_register_config(mut self, register_config: RegisterConfig) -> Self {
        self.register_config = register_config;
        self
    }

    /// Enable or disable automatic fflags capture for floating instructions
    pub fn with_capture_fflags(mut self, enabled: bool) -> Self {
        self.capture_fflags = enabled;
        self
    }

    /// Override the CSR enable toggles used during CSR randomization.
    pub fn with_csr_config(mut self, csr_config: CsrConfig) -> Self {
        self.csr_config = csr_config;
        self
    }

    /// Enable data-sensitive generation with a probability override.
    pub fn with_data_sensitive_mode(mut self, probability: f64) -> Self {
        self.data_sensitive = DataSensitiveConfig::enabled(probability);
        self
    }

    /// Access data-sensitive settings mutably.
    pub fn data_sensitive_mut(&mut self) -> &mut DataSensitiveConfig {
        &mut self.data_sensitive
    }

    /// Enable or disable machine-mode CSR selection when generating CSR instructions.
    pub fn with_machine_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_machine_csrs = enable;
        self
    }

    /// Enable or disable supervisor-mode CSR selection when generating CSR instructions.
    pub fn with_supervisor_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_supervisor_csrs = enable;
        self
    }

    /// Enable or disable hypervisor-mode CSR selection when generating CSR instructions.
    pub fn with_hypervisor_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_hypervisor_csrs = enable;
        self
    }

    /// Enable or disable virtual supervisor CSR selection (VS shadow registers).
    pub fn with_virtual_supervisor_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_virtual_supervisor_csrs = enable;
        self
    }

    /// Enable or disable RNMI CSR selection for Smrnmi scenarios.
    pub fn with_rnmi_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_rnmi_csrs = enable;
        self
    }

    /// Enable or disable floating-point CSR selection (fflags/frm/fcsr).
    pub fn with_floating_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_floating_csrs = enable;
        self
    }

    /// Enable or disable vector CSR selection (vtype/vstart/... ).
    pub fn with_vector_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_vector_csrs = enable;
        self
    }

    /// Enable or disable debug CSR selection (dcsr/dpc/dscratch*).
    pub fn with_debug_csrs(mut self, enable: bool) -> Self {
        self.csr_config.enable_debug_csrs = enable;
        self
    }

    const fn default_capture_fflags() -> bool {
        true
    }
}

impl Default for RandomConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned when generating random values fails due to configuration or validation.
#[derive(Debug, Clone)]
pub struct RandomGenerationError {
    ty: &'static str,
    message: String,
}

impl RandomGenerationError {
    /// Create a new error with the given type name and message.
    pub fn new(ty: &'static str, message: impl Into<String>) -> Self {
        Self {
            ty,
            message: message.into(),
        }
    }

    /// Convenience helper for errors caused by exhausting retries while respecting constraints.
    pub fn exhausted(ty: &'static str, attempts: usize, last_error: Option<String>) -> Self {
        let mut msg = format!("exhausted {attempts} attempts while generating a valid value");
        if let Some(err) = last_error {
            if !err.is_empty() {
                msg.push_str(": ");
                msg.push_str(&err);
            }
        }
        Self::new(ty, msg)
    }

    /// Attach additional context to the error message.
    pub fn with_context(self, context: impl Into<String>) -> Self {
        let mut msg = self.message;
        let context: String = context.into();
        if !context.is_empty() {
            if !msg.is_empty() {
                msg.push_str("; ");
            }
            msg.push_str(&context);
        }
        Self {
            message: msg,
            ..self
        }
    }

    /// Returns the type name associated with this error.
    pub fn ty(&self) -> &'static str {
        self.ty
    }
}

impl fmt::Display for RandomGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "failed to generate random {}", self.ty)
        } else {
            write!(f, "failed to generate random {}: {}", self.ty, self.message)
        }
    }
}

impl std::error::Error for RandomGenerationError {}

pub trait Random {
    type Output;
    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError>;
}

impl Random for bool {
    type Output = bool;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for u8 {
    type Output = u8;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for u16 {
    type Output = u16;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for u32 {
    type Output = u32;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for u64 {
    type Output = u64;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for i8 {
    type Output = i8;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for i16 {
    type Output = i16;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for i32 {
    type Output = i32;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for i64 {
    type Output = i64;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Ok(rng.random())
    }
}

impl Random for String {
    type Output = String;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        // Generate a random label name, e.g., "label_123"
        Ok(format!("label_{}", rng.random_range(0u32..1000u32)))
    }
}

/// Trait for generating instruction sequences with random setup instructions
pub trait RandomInstructionSequence {
    type Output;
    fn random_sequence_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<InstructionSequence<Self::Output>, RandomGenerationError>;
}

/// Trait for types that have a memory-aware const generic parameter
pub trait MemoryAware {
    /// Returns true if this type is memory-aware (needs immediate range constraints)
    fn is_memory_aware() -> bool;
}

/// Trait implemented by instruction structs and enums that describe memory accesses.
///
/// Implementors can return the runtime value of their offset operand (if any).
pub trait MemoryAccessInstruction {
    /// Returns the offset operand value (in signed 64-bit form) when present.
    fn offset_operand_value(&self) -> Option<i64>;
}

/// Common trait for validated value types
pub trait ValidatedValue<T: 'static> {
    const MIN: T;
    const MAX: T;
    const MULTIPLE_OF: Option<T> = None;
    const FORBIDDEN: &'static [T] = &[];
    const ODD_ONLY: bool = false;
    type Error;
    fn new(value: T) -> Result<Self, Self::Error>
    where
        Self: Sized;
    fn get(&self) -> T;
    fn set(&mut self, value: T) -> Result<(), Self::Error>;
}

// --- Basic register and immediate types ---

/// Integer register identifier (x0-x31)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(min = "0", max = "31", name = "Integer register", display = "x{}")]
pub struct IntegerRegister(u8);

impl Random for IntegerRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let (min, max) = config.register_config.integer_register_range;
        if min > max {
            return Err(RandomGenerationError::new(
                stringify!(IntegerRegister),
                format!("invalid configured range {min}..={max}"),
            ));
        }
        let value = rng.random_range(min..=max);
        Self::new(value).map_err(|err| RandomGenerationError::new(stringify!(IntegerRegister), err))
    }
}

impl MemoryAware for IntegerRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

/// Base address register for memory access instructions (x0-x31)
/// This type uses MemConfig.register_number_range instead of RegisterConfig.integer_register_range
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(min = "0", max = "31", name = "Base address register", display = "x{}")]
pub struct BaseAddressRegister(u8);

impl Random for BaseAddressRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let (min, max) = config.mem_config.register_number_range;
        if min > max {
            return Err(RandomGenerationError::new(
                stringify!(BaseAddressRegister),
                format!("invalid configured range {min}..={max}"),
            ));
        }
        let value = rng.random_range(min..=max);
        Self::new(value)
            .map_err(|err| RandomGenerationError::new(stringify!(BaseAddressRegister), err))
    }
}

impl MemoryAware for BaseAddressRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

/// Compressed base address register for memory access instructions (x8-x15)
/// This type uses the intersection of x8-x15 and MemConfig.register_number_range
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(
    min = "8",
    max = "15",
    name = "Compressed base address register",
    display = "x{}"
)]
pub struct CompressedBaseAddressRegister(u8);

impl Random for CompressedBaseAddressRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let (config_min, config_max) = config.mem_config.register_number_range;

        // Compute the intersection of x8-x15 with the configured range
        let actual_min = Self::MIN.max(config_min);
        let actual_max = Self::MAX.min(config_max);

        // Check whether the intersection is non-empty
        if actual_min > actual_max {
            return Err(RandomGenerationError::new(
                stringify!(CompressedBaseAddressRegister),
                format!(
                    "no intersection between compressed register range (x8-x15) and configured range (x{}-x{})",
                    config_min, config_max
                ),
            ));
        }

        let value = rng.random_range(actual_min..=actual_max);
        Self::new(value).map_err(|err| {
            RandomGenerationError::new(stringify!(CompressedBaseAddressRegister), err)
        })
    }
}

impl MemoryAware for CompressedBaseAddressRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(
    min = "0",
    max = "11",
    name = "Saved Integer register",
    display = "s{}"
)]
pub struct SavedIntegerRegister(u8);

impl MemoryAware for SavedIntegerRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(
    min = "0",
    max = "7",
    name = "Compressed Saved Integer register",
    display = "s{}"
)]
pub struct CompressedSavedIntegerRegister(u8);

impl MemoryAware for CompressedSavedIntegerRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

const SAVED_INTEGER_REGISTER_TO_X: [u8; 12] = [8, 9, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27];
const COMPRESSED_SAVED_INTEGER_REGISTER_TO_X: [u8; 8] = [8, 9, 18, 19, 20, 21, 22, 23];

fn choose_saved_register_index<R: rand::Rng>(
    rng: &mut R,
    config: &RandomConfig,
    physical_registers: &[u8],
    type_name: &'static str,
) -> Result<u8, RandomGenerationError> {
    let (config_min, config_max) = config.register_config.integer_register_range;
    if config_min > config_max {
        return Err(RandomGenerationError::new(
            type_name,
            format!(
                "invalid configured integer register range x{}-x{}",
                config_min, config_max
            ),
        ));
    }

    let mut candidates = Vec::new();
    for (logical_idx, &physical_idx) in physical_registers.iter().enumerate() {
        if (config_min..=config_max).contains(&physical_idx) {
            candidates.push(logical_idx as u8);
        }
    }

    if candidates.is_empty() {
        return Err(RandomGenerationError::new(
            type_name,
            format!(
                "no {} values overlap with configured integer register range (x{}-x{})",
                type_name, config_min, config_max
            ),
        ));
    }

    let choice = rng.random_range(0..candidates.len());
    Ok(candidates[choice])
}

impl Random for SavedIntegerRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        const TYPE_NAME: &str = stringify!(SavedIntegerRegister);
        let saved_idx =
            choose_saved_register_index(rng, config, &SAVED_INTEGER_REGISTER_TO_X, TYPE_NAME)?;

        Self::new(saved_idx).map_err(|err| RandomGenerationError::new(TYPE_NAME, err))
    }
}

impl Random for CompressedSavedIntegerRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        const TYPE_NAME: &str = stringify!(CompressedSavedIntegerRegister);
        let saved_idx = choose_saved_register_index(
            rng,
            config,
            &COMPRESSED_SAVED_INTEGER_REGISTER_TO_X,
            TYPE_NAME,
        )?;

        Self::new(saved_idx).map_err(|err| RandomGenerationError::new(TYPE_NAME, err))
    }
}

/// Floating-point register identifier (f0-f31)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(
    min = "0",
    max = "31",
    name = "Floating-point register",
    display = "f{}"
)]
pub struct FloatingPointRegister(u8);

impl Random for FloatingPointRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let (min, max) = config.register_config.floating_point_register_range;
        if min > max {
            return Err(RandomGenerationError::new(
                stringify!(FloatingPointRegister),
                format!("invalid configured range {min}..={max}"),
            ));
        }
        let value = rng.random_range(min..=max);
        Self::new(value)
            .map_err(|err| RandomGenerationError::new(stringify!(FloatingPointRegister), err))
    }
}

impl MemoryAware for FloatingPointRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue)]
#[validated(min = "0", max = "31", name = "Vector register", display = "v{}")]
pub struct VectorRegister(u8);

impl Random for VectorRegister {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let (min, max) = config.register_config.vector_register_range;
        if min > max {
            return Err(RandomGenerationError::new(
                stringify!(VectorRegister),
                format!("invalid configured range {min}..={max}"),
            ));
        }
        let value = rng.random_range(min..=max);
        Self::new(value).map_err(|err| RandomGenerationError::new(stringify!(VectorRegister), err))
    }
}

impl MemoryAware for VectorRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

/// Compressed integer register identifier (x8-x15)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(
    min = "8",
    max = "15",
    name = "Compressed integer register",
    skip_display
)]
pub struct CompressedIntegerRegister(u8);

impl MemoryAware for CompressedIntegerRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

impl Display for CompressedIntegerRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.0)
    }
}

/// Compressed floating-point register identifier (f8-f15)
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(
    min = "8",
    max = "15",
    name = "Compressed floating-point register",
    display = "f{}"
)]
pub struct CompressedFloatingPointRegister(u8);

impl MemoryAware for CompressedFloatingPointRegister {
    fn is_memory_aware() -> bool {
        false
    }
}

/// Immediate value with bit length validation
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(
    min = "-(1i32 << (BITS - 1))",
    max = "(1i32 << (BITS - 1)) - 1",
    name = "Immediate"
)]
pub struct Immediate<const BITS: u8, const MEM_AWARE: bool = false>(i32);

impl<const BITS: u8, const MEM_AWARE: bool> MemoryAware for Immediate<BITS, MEM_AWARE> {
    fn is_memory_aware() -> bool {
        MEM_AWARE
    }
}

/// Unsigned immediate value with bit length validation
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(
    min = "0",
    max = "((1u64 << BITS) - 1) as u32",
    name = "Unsigned immediate"
)]
pub struct UImmediate<const BITS: u8, const MEM_AWARE: bool = false>(u32);

impl<const BITS: u8, const MEM_AWARE: bool> MemoryAware for UImmediate<BITS, MEM_AWARE> {
    fn is_memory_aware() -> bool {
        MEM_AWARE
    }
}

/// CSR address
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(min = "0", max = "0xFFF", name = "CSR address", display = "0x{:x}")]
pub struct CSRAddress(u16);

impl MemoryAware for CSRAddress {
    fn is_memory_aware() -> bool {
        false
    }
}

/// Rounding mode
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(min = "0", max = "7", name = "Rounding mode", skip_display)]
pub struct RoundingMode(u8);

impl MemoryAware for RoundingMode {
    fn is_memory_aware() -> bool {
        false
    }
}

impl Display for RoundingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        let mnemonic = match value {
            0 => "rne",
            1 => "rtz",
            2 => "rdn",
            3 => "rup",
            4 => "rmm",
            5 => "dyn",
            6 => "dyn",
            7 => "dyn",
            _ => unreachable!(),
        };
        write!(f, "{}", mnemonic)
    }
}

impl RoundingMode {
    pub const RNE: Self = Self(0);
    pub const RTZ: Self = Self(1);
    pub const RDN: Self = Self(2);
    pub const RUP: Self = Self(3);
    pub const RMM: Self = Self(4);
    /// Returns the numeric encoding that should be written into `frm`.
    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Fence mode
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveValidatedValue, DeriveRandom,
)]
#[validated(
    min = "0",
    max = "15",
    name = "Fence mode",
    skip_display,
    forbidden = "0"
)]
pub struct FenceMode(u8);

impl MemoryAware for FenceMode {
    fn is_memory_aware() -> bool {
        false
    }
}

impl Display for FenceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        let mut mnemonic = String::new();
        if (value & 8) != 0 {
            mnemonic.push('i');
        }
        if (value & 4) != 0 {
            mnemonic.push('o');
        }
        if (value & 2) != 0 {
            mnemonic.push('r');
        }
        if (value & 1) != 0 {
            mnemonic.push('w');
        }
        write!(f, "{}", mnemonic)
    }
}

/// Represents the 32 hardware constants loadable by the RISC-V Zfa `fli` instruction.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, DeriveRandom)]
pub enum FliConstant {
    /// Index 0: -1.0
    NegativeOne,
    /// Index 1: smallest normal number
    MinNormal,
    /// Index 2: 1.0 * 2^-16
    PowerOfTwoN16,
    /// Index 3: 1.0 * 2^-15
    PowerOfTwoN15,
    /// Index 4: 1.0 * 2^-8
    PowerOfTwoN8,
    /// Index 5: 1.0 * 2^-7
    PowerOfTwoN7,
    /// Index 6: 1.0 * 2^-4
    PowerOfTwoN4,
    /// Index 7: 1.0 * 2^-3
    PowerOfTwoN3,
    /// Index 8: 0.25
    Point25,
    /// Index 9: 0.3125
    Point3125,
    /// Index 10: 0.375
    Point375,
    /// Index 11: 0.4375
    Point4375,
    /// Index 12: 0.5
    Point5,
    /// Index 13: 0.625
    Point625,
    /// Index 14: 0.75
    Point75,
    /// Index 15: 0.875
    Point875,
    /// Index 16: 1.0
    One,
    /// Index 17: 1.25
    OnePoint25,
    /// Index 18: 1.5
    OnePoint5,
    /// Index 19: 1.75
    OnePoint75,
    /// Index 20: 2.0
    Two,
    /// Index 21: 2.5
    TwoPoint5,
    /// Index 22: 3.0
    Three,
    /// Index 23: 4.0
    Four,
    /// Index 24: 8.0
    Eight,
    /// Index 25: 16.0
    Sixteen,
    /// Index 26: 2^7
    PowerOfTwo7,
    /// Index 27: 2^8
    PowerOfTwo8,
    /// Index 28: 2^15
    PowerOfTwo15,
    /// Index 29: 2^16
    PowerOfTwo16,
    /// Index 30: +inf
    PositiveInfinity,
    /// Index 31: NaN
    NaN,
}

// Updated Display implementation
impl fmt::Display for FliConstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Return the literal string consistent with the assembly file
        let literal = match self {
            Self::NegativeOne => "-1.0",
            // Change: use the literal \"min\" string directly
            Self::MinNormal => "min",
            // Change: use hexadecimal floating literal strings directly
            Self::PowerOfTwoN16 => "0x1.0p-16",
            Self::PowerOfTwoN15 => "0x1.0p-15",
            Self::PowerOfTwoN8 => "0x1.0p-8",
            Self::PowerOfTwoN7 => "0x1.0p-7",
            Self::PowerOfTwoN4 => "0x1.0p-4",
            Self::PowerOfTwoN3 => "0x1.0p-3",
            Self::Point25 => "0.25",
            Self::Point3125 => "0.3125",
            Self::Point375 => "0.375",
            Self::Point4375 => "0.4375",
            Self::Point5 => "0.5",
            Self::Point625 => "0.625",
            Self::Point75 => "0.75",
            Self::Point875 => "0.875",
            Self::One => "1.0",
            Self::OnePoint25 => "1.25",
            Self::OnePoint5 => "1.5",
            Self::OnePoint75 => "1.75",
            Self::Two => "2.0",
            Self::TwoPoint5 => "2.5",
            Self::Three => "3.0",
            Self::Four => "4.0",
            Self::Eight => "8.0",
            Self::Sixteen => "16.0",
            Self::PowerOfTwo7 => "128.0",
            Self::PowerOfTwo8 => "256.0",
            Self::PowerOfTwo15 => "32768.0",
            Self::PowerOfTwo16 => "65536.0",
            Self::PositiveInfinity => "inf",
            Self::NaN => "nan",
        };
        write!(f, "{}", literal)
    }
}

impl MemoryAware for FliConstant {
    fn is_memory_aware() -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedRegListError {
    InvalidRegisterRange,
    InvalidStackAdjustment,
    MismatchedStackSize,
}

impl fmt::Display for SavedRegListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRegisterRange => write!(f, "Invalid register range, must start from s0"),
            Self::InvalidStackAdjustment => write!(f, "Invalid stack adjustment value"),
            Self::MismatchedStackSize => write!(f, "Stack adjustment doesn't match register count"),
        }
    }
}

impl std::error::Error for SavedRegListError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRegListWithStackAdj<const XLEN: u8> {
    /// End of the saved integer register list (None means only ra; Some(n) means s0 through s[n])
    pub saved_register_end: Option<SavedIntegerRegister>,
    /// Stack adjustment (must be positive and 16-byte aligned)
    pub stack_adj: u32,
}

/// Register list for the RV32I architecture
pub type SavedRegListWithStackAdjRv32 = SavedRegListWithStackAdj<32>;
/// Register list for the RV64I architecture
pub type SavedRegListWithStackAdjRv64 = SavedRegListWithStackAdj<64>;

impl<const XLEN: u8> SavedRegListWithStackAdj<XLEN> {
    /// Create a new list that automatically includes ra and s0 through s[saved_end]
    pub fn new(saved_end: u8, stack_adj: u32) -> Result<Self, SavedRegListError> {
        // Enforce that saved_register_end cannot be 10
        if saved_end == 10 {
            return Err(SavedRegListError::InvalidRegisterRange);
        }

        let saved_register_end = Some(
            SavedIntegerRegister::new(saved_end)
                .map_err(|_| SavedRegListError::InvalidRegisterRange)?,
        );

        // Validate stack adjustment value
        if stack_adj == 0 || stack_adj % 16 != 0 {
            return Err(SavedRegListError::InvalidStackAdjustment);
        }

        let instance = Self {
            saved_register_end,
            stack_adj,
        };

        // Validate that the stack size is reasonable
        if !instance.is_valid_combination() {
            return Err(SavedRegListError::MismatchedStackSize);
        }

        Ok(instance)
    }

    /// Case with only ra saved
    pub fn ra_only(stack_adj: u32) -> Result<Self, SavedRegListError> {
        if stack_adj == 0 || stack_adj % 16 != 0 {
            return Err(SavedRegListError::InvalidStackAdjustment);
        }

        let instance = Self {
            saved_register_end: None,
            stack_adj,
        };

        // Validate that the stack size is reasonable
        if !instance.is_valid_combination() {
            return Err(SavedRegListError::MismatchedStackSize);
        }

        Ok(instance)
    }

    /// Get the number of registers (including ra)
    pub fn register_count(&self) -> u8 {
        match self.saved_register_end {
            None => 1,                        // only ra
            Some(end) => 1 + (end.get() + 1), // ra plus s0 through s[end]
        }
    }

    /// Whether only ra is saved
    pub fn is_ra_only(&self) -> bool {
        self.saved_register_end.is_none()
    }

    /// Get rlist encoding based on the register count (used to determine stack_adj_base)
    fn get_rlist_encoding(&self) -> u8 {
        match self.saved_register_end {
            None => 4,                  // {ra}
            Some(end) => 5 + end.get(), // 5 + s[end] index
        }
    }

    /// Get the base stack size and allowed stack_adj values
    fn get_stack_adj_info(&self) -> (u32, Vec<u32>) {
        let rlist = self.get_rlist_encoding();

        match XLEN {
            32 => {
                // RV32I architecture
                match rlist {
                    4..=7 => (16, vec![16, 32, 48, 64]),   // {ra} through {ra, s0-s2}
                    8..=11 => (32, vec![32, 48, 64, 80]),  // {ra, s0-s3} through {ra, s0-s6}
                    12..=15 => (48, vec![48, 64, 80, 96]), // {ra, s0-s7} through {ra, s0-s10}
                    16 => (64, vec![64, 80, 96, 112]),     // {ra, s0-s11}
                    _ => (16, vec![16, 32, 48, 64]),       // Should be unreachable for valid rlist
                }
            }
            64 => {
                // RV64I architecture
                match rlist {
                    4..=5 => (16, vec![16, 32, 48, 64]),      // {ra} through {ra, s0}
                    6..=7 => (32, vec![32, 48, 64, 80]),      // {ra, s0-s1} through {ra, s0-s2}
                    8..=9 => (48, vec![48, 64, 80, 96]),      // {ra, s0-s3} through {ra, s0-s4}
                    10..=11 => (64, vec![64, 80, 96, 112]),   // {ra, s0-s5} through {ra, s0-s6}
                    12..=13 => (80, vec![80, 96, 112, 128]),  // {ra, s0-s7} through {ra, s0-s8}
                    14..=15 => (96, vec![96, 112, 128, 144]), // {ra, s0-s9} through {ra, s0-s10}
                    16 => (112, vec![112, 128, 144, 160]),    // {ra, s0-s11}
                    _ => (16, vec![16, 32, 48, 64]), // Should be unreachable for valid rlist
                }
            }
            _ => panic!("Unsupported XLEN: {}", XLEN),
        }
    }

    /// Validate whether the register count and stack adjustment combination is valid
    pub fn is_valid_combination(&self) -> bool {
        let (_, valid_adjustments) = self.get_stack_adj_info();
        valid_adjustments.contains(&self.stack_adj)
    }

    /// Get all valid stack adjustment values
    pub fn valid_stack_adjustments(&self) -> Vec<u32> {
        let (_, valid_adjustments) = self.get_stack_adj_info();
        valid_adjustments
    }

    pub fn get_saved_reg_list_string(&self) -> String {
        // Generate the register list string like the Display implementation
        let mut reg_list = String::new();
        reg_list.push_str("{ra");
        if let Some(end) = self.saved_register_end {
            let end_val = end.get();
            if end_val == 0 {
                reg_list.push_str(", s0");
            } else {
                reg_list.push_str(&format!(", s0-s{}", end_val));
            }
        }
        reg_list.push_str("}");
        reg_list
    }

    pub fn get_stack_adjustment(&self) -> u32 {
        self.stack_adj
    }
}

impl<const XLEN: u8> Display for SavedRegListWithStackAdj<XLEN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{")?;

        // Always include ra
        write!(f, "ra")?;

        // If not ra-only, add s registers
        if let Some(end) = self.saved_register_end {
            let end_val = end.get();
            if end_val == 0 {
                write!(f, ", s0")?;
            } else {
                write!(f, ", s0-s{}", end_val)?;
            }
        }

        write!(f, "}}, {}", self.stack_adj)
    }
}

impl<const XLEN: u8> Random for SavedRegListWithStackAdj<XLEN> {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        _config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        const TYPE_NAME: &str = stringify!(SavedRegListWithStackAdj);

        // Randomly choose how many registers are saved
        let end_reg_options: [Option<u8>; 12] = [
            None,
            Some(0),
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            Some(7),
            Some(8),
            Some(9),
            Some(11),
        ];

        let saved_end = *end_reg_options.choose(rng).ok_or_else(|| {
            RandomGenerationError::new(TYPE_NAME, "no register options available")
        })?;

        let saved_register_end = match saved_end {
            None => None,
            Some(end) => Some(SavedIntegerRegister::new(end).map_err(|err| {
                RandomGenerationError::new(stringify!(SavedIntegerRegister), err)
            })?),
        };

        // Create a temporary instance to obtain valid stack_adj values
        let temp_instance = Self {
            saved_register_end,
            stack_adj: 16, // temporary value
        };

        let valid_adjustments = temp_instance.valid_stack_adjustments();
        let stack_adj = *valid_adjustments.choose(rng).ok_or_else(|| {
            RandomGenerationError::new(TYPE_NAME, "no valid stack adjustments available")
        })?;

        let result = match saved_end {
            None => Self::ra_only(stack_adj),
            Some(end) => Self::new(end, stack_adj),
        };

        result.map_err(|err| RandomGenerationError::new(TYPE_NAME, err.to_string()))
    }
}

impl<const XLEN: u8> MemoryAware for SavedRegListWithStackAdj<XLEN> {
    fn is_memory_aware() -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotEqualCompressedSavedIntegerRegisterPair {
    pub reg1: CompressedSavedIntegerRegister,
    pub reg2: CompressedSavedIntegerRegister,
}

impl Random for NotEqualCompressedSavedIntegerRegisterPair {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        const TYPE_NAME: &str = stringify!(NotEqualCompressedSavedIntegerRegisterPair);
        const MAX_ATTEMPTS: usize = 256;

        let reg1 = CompressedSavedIntegerRegister::random_with_rng(rng, config)?;
        for attempt in 0..MAX_ATTEMPTS {
            let reg2 = CompressedSavedIntegerRegister::random_with_rng(rng, config)?;
            if reg2 != reg1 {
                return Ok(Self { reg1, reg2 });
            }

            if attempt + 1 == MAX_ATTEMPTS {
                return Err(RandomGenerationError::exhausted(
                    TYPE_NAME,
                    MAX_ATTEMPTS,
                    Some(format!("unable to find register distinct from {}", reg1)),
                ));
            }
        }

        Err(RandomGenerationError::new(
            TYPE_NAME,
            "unexpected failure while generating distinct registers",
        ))
    }
}

impl MemoryAware for NotEqualCompressedSavedIntegerRegisterPair {
    fn is_memory_aware() -> bool {
        false
    }
}

impl Display for NotEqualCompressedSavedIntegerRegisterPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.reg1, self.reg2)
    }
}

// --- Pseudo Instructions ---

/// Purpose metadata for `li` pseudo instructions injected ahead of real instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadImmediatePurpose {
    /// Prepare a base address register used by a subsequent memory access.
    BaseAddress,
    /// Seed the source register value consumed by a CSR write instruction.
    CsrValue,
    /// Generic/unspecified `li` usage.
    Generic,
}

/// RISC-V pseudo instructions for common operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PseudoInstruction {
    /// Load address - la rd, symbol (expands to auipc + addi)
    LoadAddress { rd: IntegerRegister, address: u64 },
    /// Load immediate - li rd, imm (expands to lui + addi or just addi)
    LoadImmediate {
        rd: IntegerRegister,
        immediate: i64,
        purpose: LoadImmediatePurpose,
    },
    /// Load address of label - lla rd, label (expands to auipc + addi)
    LoadLabelAddress { rd: IntegerRegister, label: String },
    /// Read fflags CSR into a general-purpose register via csrrs
    ReadFflags { rd: IntegerRegister },
    /// Move a temporary integer register into an FP register without rounding.
    MoveIntegerToFloat {
        target: FloatingPointRegister,
        source: IntegerRegister,
        format: FloatFormat,
    },
    /// Set the floating-point rounding mode CSR.
    SetRoundingMode { mode: RoundingMode },
    /// Inline comment used to annotate generated instruction streams.
    Comment(String),
}

impl Random for PseudoInstruction {
    type Output = Self;

    fn random_with_rng<R: rand::Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        let variant = rng.random_range(0..3);
        match variant {
            0 => Ok(PseudoInstruction::LoadAddress {
                rd: IntegerRegister::random_with_rng(rng, config)?,
                address: rng.random::<u64>() & 0xFFFFFFFF, // 32-bit address
            }),
            1 => Ok(PseudoInstruction::LoadImmediate {
                rd: IntegerRegister::random_with_rng(rng, config)?,
                immediate: rng.random::<i32>() as i64, // 32-bit immediate
                purpose: LoadImmediatePurpose::Generic,
            }),
            _ => Ok(PseudoInstruction::LoadLabelAddress {
                rd: IntegerRegister::random_with_rng(rng, config)?,
                label: String::random_with_rng(rng, config)?,
            }),
        }
    }
}

impl Display for PseudoInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PseudoInstruction::LoadAddress { rd, address } => {
                write!(f, "la {}, 0x{:x}", rd, address)
            }
            PseudoInstruction::LoadImmediate { rd, immediate, .. } => {
                write!(f, "li {}, {}", rd, immediate)
            }
            PseudoInstruction::LoadLabelAddress { rd, label } => {
                write!(f, "lla {}, {}", rd, label)
            }
            PseudoInstruction::ReadFflags { rd } => write!(f, "csrrs {}, fflags, x0", rd),
            PseudoInstruction::MoveIntegerToFloat {
                target,
                source,
                format,
            } => {
                let mnemonic = match format {
                    FloatFormat::Half | FloatFormat::BFloat16 => "fmv.h.x",
                    FloatFormat::Single => "fmv.w.x",
                    FloatFormat::Double => "fmv.d.x",
                    FloatFormat::Quad => "fmv.q.x",
                };
                write!(f, "{} {}, {}", mnemonic, target, source)
            }
            PseudoInstruction::SetRoundingMode { mode } => {
                write!(f, "csrrw x0, frm, {}", mode.value())
            }
            PseudoInstruction::Comment(text) => write!(f, "# {}", text),
        }
    }
}

/// Instruction sequence containing zero or more pre instructions followed by a single actual instruction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionSequence<T> {
    /// Vector of pre instructions executed before the main instruction (can be empty)
    pub pre_instructions: Vec<PseudoInstruction>,
    /// The final actual instruction
    pub instruction: T,
    /// Vector of pseudo instructions executed after the main instruction
    pub post_instructions: Vec<PseudoInstruction>,
}

impl<T> InstructionSequence<T> {
    /// Create a new instruction sequence with only the main instruction (no pseudo instructions)
    pub fn new(instruction: T) -> Self {
        Self {
            pre_instructions: Vec::new(),
            instruction,
            post_instructions: Vec::new(),
        }
    }

    /// Create a new instruction sequence with pre instructions
    pub fn with_pre_instructions(pre_instructions: Vec<PseudoInstruction>, instruction: T) -> Self {
        Self {
            pre_instructions,
            instruction,
            post_instructions: Vec::new(),
        }
    }

    /// Create a new instruction sequence with both pre and post pseudo instructions
    pub fn with_full_instructions(
        pre_instructions: Vec<PseudoInstruction>,
        instruction: T,
        post_instructions: Vec<PseudoInstruction>,
    ) -> Self {
        Self {
            pre_instructions,
            instruction,
            post_instructions,
        }
    }

    /// Add a pre instruction to the sequence
    pub fn add_pre_instruction(&mut self, pseudo_instruction: PseudoInstruction) {
        self.pre_instructions.push(pseudo_instruction);
    }

    /// Add a post pseudo instruction to the sequence
    pub fn add_post_instruction(&mut self, pseudo_instruction: PseudoInstruction) {
        self.post_instructions.push(pseudo_instruction);
    }

    /// Get a reference to the main instruction
    pub fn main_instruction(&self) -> &T {
        &self.instruction
    }

    /// Get a reference to the pre instructions
    pub fn pre_instructions(&self) -> &[PseudoInstruction] {
        &self.pre_instructions
    }

    /// Get a reference to the post pseudo instructions
    pub fn post_instructions(&self) -> &[PseudoInstruction] {
        &self.post_instructions
    }

    /// Add a value to all LoadImmediate pseudo instructions' immediate values
    pub fn add_to_load_immediate(&mut self, value: i64) {
        for pseudo in &mut self.pre_instructions {
            if let PseudoInstruction::LoadImmediate { immediate, .. } = pseudo {
                *immediate = immediate.saturating_add(value);
            }
        }
        for pseudo in &mut self.post_instructions {
            if let PseudoInstruction::LoadImmediate { immediate, .. } = pseudo {
                *immediate = immediate.saturating_add(value);
            }
        }
    }
}

impl<T: Display> Display for InstructionSequence<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write each pseudo instruction on a separate line
        for pseudo in &self.pre_instructions {
            writeln!(f, "{}", pseudo)?;
        }
        // Write the final instruction
        write!(f, "{}", self.instruction)?;

        if self.post_instructions.is_empty() {
            return Ok(());
        }

        writeln!(f)?;
        for (idx, pseudo) in self.post_instructions.iter().enumerate() {
            if idx + 1 == self.post_instructions.len() {
                write!(f, "{}", pseudo)?;
            } else {
                writeln!(f, "{}", pseudo)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_register_reports_configured_range_errors() {
        let mut rng = rand::rng();
        let config = RandomConfig::new()
            .with_register_config(RegisterConfig::new().with_integer_register_range(10, 5));

        let err = IntegerRegister::random_with_rng(&mut rng, &config)
            .expect_err("expected configuration error");
        assert_eq!(err.ty(), "IntegerRegister");
        assert!(err.to_string().contains("invalid configured range"));
    }

    #[test]
    fn saved_integer_register_detects_disjoint_ranges() {
        let mut rng = rand::rng();
        let config = RandomConfig::new()
            .with_register_config(RegisterConfig::new().with_integer_register_range(0, 7));

        let err = SavedIntegerRegister::random_with_rng(&mut rng, &config)
            .expect_err("expected disjoint range error");
        assert_eq!(err.ty(), "SavedIntegerRegister");
        assert!(
            err.to_string()
                .contains("no SavedIntegerRegister values overlap")
        );
    }

    #[test]
    fn saved_integer_register_respects_physical_mapping() {
        let mut rng = rand::rng();
        let config = RandomConfig::new()
            .with_register_config(RegisterConfig::new().with_integer_register_range(18, 18));

        let reg = SavedIntegerRegister::random_with_rng(&mut rng, &config)
            .expect("expected valid saved register");
        assert_eq!(reg.get(), 2);
    }

    #[test]
    fn compressed_saved_integer_register_detects_disjoint_ranges() {
        let mut rng = rand::rng();
        let config = RandomConfig::new()
            .with_register_config(RegisterConfig::new().with_integer_register_range(24, 31));

        let err = CompressedSavedIntegerRegister::random_with_rng(&mut rng, &config)
            .expect_err("expected disjoint range error");
        assert_eq!(err.ty(), "CompressedSavedIntegerRegister");
        assert!(
            err.to_string()
                .contains("no CompressedSavedIntegerRegister values overlap")
        );
    }

    #[test]
    fn compressed_saved_integer_register_respects_physical_mapping() {
        let mut rng = rand::rng();
        let config = RandomConfig::new()
            .with_register_config(RegisterConfig::new().with_integer_register_range(21, 21));

        let reg = CompressedSavedIntegerRegister::random_with_rng(&mut rng, &config)
            .expect("expected valid compressed saved register");
        assert_eq!(reg.get(), 5);
    }
}
