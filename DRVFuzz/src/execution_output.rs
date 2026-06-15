use crate::error::{ContextBuildError, NormalizeError};
use crate::isa_base::ISABase;
use crate::riscv_impls::RiscVImpl;
use crate::utils::{
    MemoryContext, MemoryValueWidth, RegisterContext, extract_memory_context,
    extract_register_context,
};
use riscv_instruction_types::RegisterConfig;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct RegisterValue {
    pub name: String,
    pub value: u64,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct MemValue {
    pub addr: u64,
    pub value: u8,
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionInfo {
    pub user_instruction_index: usize,
    pub cause: String,
}

/// Execution output containing cumulative results for each user instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOutput {
    pub exceptions: Vec<ExceptionInfo>,
    pub register_write: Vec<Vec<RegisterValue>>,
    pub memory_write: Vec<Vec<MemValue>>,
    pub riscv_impl: RiscVImpl,
    pub isa_base: ISABase,
}

impl ExecutionOutput {
    /// Normalize the execution output by filtering registers and moving memory addresses to 0-based
    pub fn normalize(
        &mut self,
        mem_range: (u64, u64),
        test_register_config: &RegisterConfig,
        temp_register_num_range: (u8, u8),
    ) -> Result<(), NormalizeError> {
        // mem_range is inclusive [start, end]; both start and end are valid addresses
        let mem_start = mem_range.0; // inclusive start address
        let mem_end = mem_range.1; // inclusive end address

        // Filter registers: keep those in configured ranges and temporary registers
        // Note: all ranges in register_config are inclusive [min, max]
        let (temp_min, temp_max) = temp_register_num_range;

        for regs in &mut self.register_write {
            regs.retain(|reg_val| {
                let name = reg_val.name.trim();
                if name.is_empty() {
                    return false;
                }

                // Safety: We've checked that name is not empty, so chars().next() will return Some
                let first_char = match name.chars().next() {
                    Some(c) => c,
                    None => return false,
                };
                let index_str = &name[1..];

                let index: u8 = match index_str.parse() {
                    Ok(idx) => idx,
                    Err(_) => return false,
                };

                match first_char {
                    'x' => {
                        // Keep integer registers within the test register range
                        let (min, max) = test_register_config.integer_register_range;
                        let in_test_range = index >= min && index <= max;

                        // Keep integer registers within the temp register range
                        let in_temp_range = index >= temp_min && index <= temp_max;

                        in_test_range || in_temp_range
                    }
                    'f' => {
                        // floating_point_register_range is inclusive [min, max]
                        let (min, max) = test_register_config.floating_point_register_range;
                        index >= min && index <= max
                    }
                    'v' => {
                        // vector_register_range is inclusive [min, max]
                        let (min, max) = test_register_config.vector_register_range;
                        index >= min && index <= max
                    }
                    _ => false,
                }
            });
        }

        // Filter and translate memory addresses
        for mems in &mut self.memory_write {
            // Map addresses within user memory to 0-based offsets; keep out-of-range addresses as-is for diagnostics
            for mem_val in mems {
                if (mem_val.addr >= mem_start) && (mem_val.addr <= mem_end) {
                    mem_val.addr -= mem_start;
                }
            }
        }

        Ok(())
    }
}

/// Execution context for a single instruction, retaining only pre-execution register and memory state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionContext {
    /// Register state before instruction execution (only modified registers)
    pub registers_before: HashMap<String, u64>,

    /// Memory state before instruction execution (only modified memory)
    pub memory_before: BTreeMap<u64, u8>,

    /// Registers referenced by the instruction with their pre-execution values
    pub register_context: RegisterContext,

    /// Memory context referenced by the instruction with multi-width snapshots
    pub memory_context: MemoryContext,
}

fn format_optional_hex(value: Option<u64>) -> String {
    match value {
        Some(v) => format!("0x{:x}", v),
        None => "?".to_string(),
    }
}

fn format_optional_hex_byte(value: Option<u8>) -> String {
    match value {
        Some(v) => format!("0x{:02x}", v),
        None => "?".to_string(),
    }
}

fn change_string(
    idx: usize,
    register_changes: &[Vec<RegisterValue>],
    memory_changes: &[Vec<MemValue>],
    registers_before: &HashMap<String, u64>,
    memory_before: &BTreeMap<u64, u8>,
) -> String {
    let mut changes = Vec::new();

    if let Some(regs) = register_changes.get(idx) {
        for reg in regs {
            let before = registers_before.get(&reg.name).copied();
            let after = Some(reg.value);
            changes.push(format!(
                "{}:{}→{}",
                reg.name,
                format_optional_hex(before),
                format_optional_hex(after)
            ));
        }
    }

    if let Some(mems) = memory_changes.get(idx) {
        let mut entries: Vec<_> = mems.iter().collect();
        entries.sort_by_key(|m| m.addr);

        let mem_write_strs: Vec<String> = entries
            .into_iter()
            .map(|mem| {
                let before = memory_before.get(&mem.addr).copied();
                format!(
                    "[0x{:x}]:{}→{}",
                    mem.addr,
                    format_optional_hex_byte(before),
                    format_optional_hex_byte(Some(mem.value))
                )
            })
            .collect();

        if !mem_write_strs.is_empty() {
            changes.push(format!("mem({})", mem_write_strs.join(",")));
        }
    }

    if changes.is_empty() {
        "-".to_string()
    } else {
        changes.join("; ")
    }
}

/// Context calculator that accumulates register and memory changes
/// to compute the execution context at any point in time
#[derive(Debug, Clone)]
struct ContextCalculator {
    registers: HashMap<String, u64>,
    memory: BTreeMap<u64, u8>,
}

impl ContextCalculator {
    fn new() -> Self {
        Self {
            registers: HashMap::new(),
            memory: BTreeMap::new(),
        }
    }

    fn apply_register_changes(&mut self, changes: &[RegisterValue]) {
        for change in changes {
            self.registers.insert(change.name.clone(), change.value);
        }
    }

    fn apply_memory_changes(&mut self, changes: &[MemValue]) {
        for change in changes {
            self.memory.insert(change.addr, change.value);
        }
    }

    fn get_all_registers(&self) -> Vec<RegisterValue> {
        self.registers
            .iter()
            .map(|(name, &value)| RegisterValue {
                name: name.clone(),
                value,
            })
            .collect()
    }

    fn get_all_memory(&self) -> Vec<MemValue> {
        self.memory
            .iter()
            .map(|(&addr, &value)| MemValue { addr, value })
            .collect()
    }
}

/// Execution output with full execution context (pre-computed to avoid repeated calculations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContextOutput {
    /// List of exceptions that occurred during execution
    pub exceptions: Vec<ExceptionInfo>,

    /// Register changes after each user instruction
    /// Length matches the number of user instructions
    pub register_changes: Vec<Vec<RegisterValue>>,

    /// Memory changes after each user instruction
    /// Length matches the number of user instructions
    pub memory_changes: Vec<Vec<MemValue>>,

    /// Execution context for each instruction (includes complete before/after state)
    pub contexts: Vec<InstructionContext>,

    /// Memory range allocated for this execution (inclusive [start, end])
    pub mem_range: (u64, u64),

    /// RISC-V implementation used for execution
    pub riscv_impl: RiscVImpl,

    /// ISA base (RV32 or RV64)
    pub isa_base: ISABase,
}

impl ExecutionContextOutput {
    /// Get the memory range recorded for execution
    pub fn mem_range(&self) -> (u64, u64) {
        self.mem_range
    }

    /// Get the number of instructions
    pub fn instruction_count(&self) -> usize {
        self.contexts.len()
    }
}

impl ExecutionContextOutput {
    pub fn from_execution_output(
        output: ExecutionOutput,
        mem_range: (u64, u64),
        user_instructions: &[String],
        instruction_offsets: &[Option<i64>],
    ) -> Result<Self, ContextBuildError> {
        let ExecutionOutput {
            exceptions,
            register_write: register_changes,
            memory_write: memory_changes,
            riscv_impl,
            isa_base,
        } = output;

        if user_instructions.len() != instruction_offsets.len() {
            return Err(ContextBuildError::InstructionMetadataLengthMismatch {
                instructions: user_instructions.len(),
                metadata: instruction_offsets.len(),
            });
        }

        let mut calculator = ContextCalculator::new();
        let mut contexts = Vec::new();

        let num_instructions = register_changes.len();
        if memory_changes.len() != num_instructions {
            return Err(ContextBuildError::WriteVectorLengthMismatch {
                registers: num_instructions,
                memory: memory_changes.len(),
            });
        }

        contexts.reserve(num_instructions);

        for (idx, (regs, mems)) in register_changes
            .iter()
            .zip(memory_changes.iter())
            .enumerate()
        {
            // Record state before execution (current calculator state)
            let registers_before: HashMap<String, u64> = calculator
                .get_all_registers()
                .into_iter()
                .map(|r| (r.name, r.value))
                .collect();

            let memory_before: BTreeMap<u64, u8> = calculator
                .get_all_memory()
                .into_iter()
                .map(|m| (m.addr, m.value))
                .collect();

            let instruction = user_instructions
                .get(idx)
                .ok_or(ContextBuildError::MissingInstruction { index: idx })?;
            let offset_hint = instruction_offsets.get(idx).and_then(|opt| *opt);

            let register_context =
                extract_register_context(instruction, &registers_before)?.unwrap_or_default();
            let memory_context = extract_memory_context(
                instruction,
                offset_hint,
                &registers_before,
                &memory_before,
                isa_base,
                mem_range,
            )?
            .unwrap_or_default();

            // Apply changes from current instruction
            calculator.apply_register_changes(regs);
            calculator.apply_memory_changes(mems);

            contexts.push(InstructionContext {
                registers_before,
                memory_before,
                register_context,
                memory_context,
            });
        }

        Ok(Self {
            exceptions,
            register_changes,
            memory_changes,
            contexts,
            mem_range,
            riscv_impl,
            isa_base,
        })
    }
}

/// Render a Markdown report for ExecutionContextOutput (using precomputed context)
pub fn generate_execution_context_report<P: AsRef<Path>>(
    output: &ExecutionContextOutput,
    path: P,
    user_instructions: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    let mut report = fs::File::create(path)?;

    // Heading
    writeln!(
        report,
        "# {:?} Execution Output Detail Report\n",
        output.riscv_impl
    )?;

    // Basic information
    writeln!(report, "## 📋 Basic Info\n")?;
    writeln!(report, "| Item | Value |")?;
    writeln!(report, "|------|-------|")?;
    writeln!(
        report,
        "| RISC-V Implementation | {:?} |",
        output.riscv_impl
    )?;
    writeln!(report, "| ISA Base | {:?} |", output.isa_base)?;
    writeln!(
        report,
        "| Instruction Count | {} |",
        user_instructions.len()
    )?;
    writeln!(report, "| Exception Count | {} |", output.exceptions.len())?;
    writeln!(
        report,
        "| User Memory Range | 0x{:016x} - 0x{:016x} |",
        output.mem_range().0,
        output.mem_range().1
    )?;
    writeln!(report)?;

    // Exception summary table
    if !output.exceptions.is_empty() {
        writeln!(report, "## ⚠️ Exception Summary\n")?;
        writeln!(
            report,
            "A total of {} exceptions occurred during execution:\n",
            output.exceptions.len()
        )?;
        writeln!(
            report,
            "| Instruction Index | Instruction | Exception Type | Register Context | Memory Context |"
        )?;
        writeln!(
            report,
            "|---------|------|---------|------------|------------|"
        )?;

        for exc in &output.exceptions {
            let idx = exc.user_instruction_index;
            let inst = user_instructions
                .get(idx)
                .ok_or(ContextBuildError::MissingInstruction { index: idx })?;
            let ctxt = output
                .contexts
                .get(idx)
                .ok_or(ContextBuildError::MissingContext { index: idx })?;

            // Retrieve pre-execution state from the precomputed context
            let reg_ctx = format_register_context(&ctxt.register_context);
            let mem_ctx = format_memory_context(&ctxt.memory_context);

            writeln!(
                report,
                "| {} | `{}` | {} | {} | {} |",
                idx, inst, exc.cause, reg_ctx, mem_ctx
            )?;
        }
        writeln!(report)?;

        // Exception counts by type
        writeln!(report, "### Exception Type Stats\n")?;
        let mut exc_type_count: HashMap<String, usize> = HashMap::new();
        for exc in &output.exceptions {
            *exc_type_count.entry(exc.cause.clone()).or_insert(0) += 1;
        }

        let mut sorted_types: Vec<_> = exc_type_count.iter().collect();
        sorted_types.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

        writeln!(report, "| Exception Type | Count | Percentage |")?;
        writeln!(report, "|---------------|-------|------------|")?;
        for (exc_type, count) in sorted_types {
            let percentage = (*count as f64 / output.exceptions.len() as f64) * 100.0;
            writeln!(report, "| {} | {} | {:.2}% |", exc_type, count, percentage)?;
        }
        writeln!(report)?;
    }

    // Execution trace summary table
    writeln!(report, "## 📊 Execution Trace Summary\n")?;
    writeln!(
        report,
        "> Memory-context addresses are shown as 0-based offsets relative to the start of user memory.\n"
    )?;
    writeln!(
        report,
        "| Instruction Index | Instruction | Changes | Register Context | Memory Context | Exception |"
    )?;
    writeln!(
        report,
        "|---------|------|-----------|------------|------------|------|"
    )?;

    // Build a mapping from instruction index to its exceptions
    let exception_map: std::collections::HashMap<
        usize,
        Vec<&crate::execution_output::ExceptionInfo>,
    > = output
        .exceptions
        .iter()
        .fold(std::collections::HashMap::new(), |mut map, exc| {
            map.entry(exc.user_instruction_index)
                .or_insert_with(Vec::new)
                .push(exc);
            map
        });

    for (idx, inst) in user_instructions.iter().enumerate() {
        let ctx = output
            .contexts
            .get(idx)
            .ok_or(ContextBuildError::MissingContext { index: idx })?;
        let context = format_register_context(&ctx.register_context);
        let memory_context = format_memory_context(&ctx.memory_context);
        if let Some(excs) = exception_map.get(&idx) {
            let exc_info = excs
                .iter()
                .map(|e| e.cause.clone())
                .collect::<Vec<_>>()
                .join("; ");

            writeln!(
                report,
                "| {} | `{}` | Exception | {} | {} | {} |",
                idx, inst, context, memory_context, exc_info
            )?;
        } else {
            let changes = change_string(
                idx,
                &output.register_changes,
                &output.memory_changes,
                &ctx.registers_before,
                &ctx.memory_before,
            );

            writeln!(
                report,
                "| {} | `{}` | {} | {} | {} |",
                idx, inst, changes, context, memory_context
            )?;
        }
    }
    writeln!(report)?;

    // Statistics summary
    writeln!(report, "## 📈 Statistics Summary\n")?;

    let write_count = output
        .register_changes
        .iter()
        .zip(output.memory_changes.iter())
        .filter(|(regs, mems)| !regs.is_empty() || !mems.is_empty())
        .count();

    writeln!(report, "| Metric | Value |")?;
    writeln!(report, "|--------|-------|")?;
    writeln!(
        report,
        "| Instructions with writes | {} ({:.2}%) |",
        write_count,
        (write_count as f64 / user_instructions.len() as f64) * 100.0
    )?;
    writeln!(
        report,
        "| Instructions with exceptions | {} ({:.2}%) |",
        output.exceptions.len(),
        (output.exceptions.len() as f64 / user_instructions.len() as f64) * 100.0
    )?;
    writeln!(
        report,
        "| Instructions without writes or exceptions | {} ({:.2}%) |",
        user_instructions.len() - output.exceptions.len(),
        ((user_instructions.len() - output.exceptions.len()) as f64
            / user_instructions.len() as f64)
            * 100.0
    )?;

    Ok(())
}

fn format_register_context(context: &RegisterContext) -> String {
    if context.entries.is_empty() {
        "-".to_string()
    } else {
        context
            .entries
            .iter()
            .map(|entry| format!("{}=0x{:x}", entry.name, entry.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_memory_context(context: &MemoryContext) -> String {
    if context.base_register.is_empty() || context.entries.is_empty() {
        "-".to_string()
    } else {
        let mut parts = Vec::new();
        parts.push(format!(
            "{}=0x{:x}",
            context.base_register, context.base_offset
        ));

        for entry in &context.entries {
            let label = match entry.width {
                MemoryValueWidth::Byte => "byte",
                MemoryValueWidth::Half => "half",
                MemoryValueWidth::Word => "word",
                MemoryValueWidth::Dword => "dword",
            };
            let addresses = entry
                .addresses
                .iter()
                .map(|addr| format!("0x{:x}", addr))
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("{} [{}]=0x{:x}", label, addresses, entry.value));
        }

        parts.join("; ")
    }
}
