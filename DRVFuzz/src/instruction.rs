use crate::data_sensitive::DataSensitiveInjector;
use crate::error::GenerationError;
use crate::{
    isa_base::ISABase,
    riscv_impls::RiscVImpl,
    riscv_impls_vec::{InstructionBlock, RiscVImplVec},
};
use rand::Rng as _;
use rand::seq::SliceRandom;
use riscv_instruction::separated_instructions::*;
use riscv_instruction_types::RegisterConfig;
use riscv_instruction_types::{
    InstructionSequence, RandomConfig, RandomGenerationError, WritableCsr,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;

use log::warn;
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GenerationOrder {
    Sequential,
    RandomShuffle,
}

fn generate_sequences_for_extensions<E, R, F>(
    counts: &HashMap<E, usize>,
    order: GenerationOrder,
    rng: &mut R,
    config: &RandomConfig,
    mut data_sensitive: Option<&mut DataSensitiveInjector>,
    mut gen_fn: F,
) -> Result<Vec<InstructionSequence<RiscvInstruction>>, RandomGenerationError>
where
    E: Copy + Eq + std::hash::Hash + Debug,
    R: rand::Rng,
    F: FnMut(
        E,
        &mut R,
        &RandomConfig,
    ) -> Result<InstructionSequence<RiscvInstruction>, RandomGenerationError>,
{
    let mut instructions = Vec::new();

    for (&ext, &count) in counts {
        let mut generated_for_ext = 0usize;

        for attempt in 0..count {
            match gen_fn(ext, rng, config) {
                Ok(mut instr) => {
                    if let Some(injector) = data_sensitive.as_mut() {
                        if let Err(err) = injector.try_apply(&mut instr, rng) {
                            warn!("data-sensitive injection skipped: {}", err);
                        }
                    }
                    instructions.push(instr);
                    generated_for_ext += 1;
                }
                Err(err) => {
                    warn!(
                        "Random instruction generation failed for extension {:?} on attempt {} of {}: {:?}. Skipping remaining instructions for this extension.",
                        ext,
                        attempt + 1,
                        count,
                        err
                    );
                    break;
                }
            }
        }

        if generated_for_ext < count {
            warn!(
                "Generated {} out of {} instructions for extension {:?} due to generation errors.",
                generated_for_ext, count, ext
            );
        }
    }

    if order == GenerationOrder::RandomShuffle {
        instructions.shuffle(rng);
    }

    Ok(instructions)
}

pub fn generate_sequences_rv64<R: rand::Rng>(
    counts: &HashMap<RV64Extensions, usize>,
    order: GenerationOrder,
    rng: &mut R,
    config: &RandomConfig,
    data_sensitive: Option<&mut DataSensitiveInjector>,
) -> Result<Vec<InstructionSequence<RiscvInstruction>>, RandomGenerationError> {
    generate_sequences_for_extensions(
        counts,
        order,
        rng,
        config,
        data_sensitive,
        |ext, rng, config| ext.random_sequence_with_rng(rng, config),
    )
}

pub fn generate_sequences_rv32<R: rand::Rng>(
    counts: &HashMap<RV32Extensions, usize>,
    order: GenerationOrder,
    rng: &mut R,
    config: &RandomConfig,
    data_sensitive: Option<&mut DataSensitiveInjector>,
) -> Result<Vec<InstructionSequence<RiscvInstruction>>, RandomGenerationError> {
    generate_sequences_for_extensions(
        counts,
        order,
        rng,
        config,
        data_sensitive,
        |ext, rng, config| ext.random_sequence_with_rng(rng, config),
    )
}

/// Generate random initialization instructions.
///
/// Provides initialization for tests, including:
/// 1. Integer register initialization
/// 2. Floating-point register initialization (if supported)
/// 3. Memory initialization
pub fn generate_initialization_instructions_for_vec(
    riscv_impl_vec: &RiscVImplVec,
    isa_base: ISABase,
    mem_range: &HashMap<RiscVImpl, (u64, u64)>,
    register_config: &RegisterConfig,
    temp_register_num_range: (u8, u8),
) -> Result<HashMap<RiscVImpl, InstructionBlock>, GenerationError> {
    let mut rng = rand::rng();
    let extension_map = riscv_impl_vec.extension_map();
    let support_vector = match isa_base {
        ISABase::Rv32 => extension_map.rv32.contains(&RV32Extensions::V),
        ISABase::Rv64 => extension_map.rv64.contains(&RV64Extensions::V),
    };

    // Generate shared random integer register values
    let int_values: Vec<u64> = {
        let (int_start, int_end) = register_config.integer_register_range;
        (int_start..=int_end)
            .map(|_| match isa_base {
                ISABase::Rv32 => rng.random::<u32>() as u64,
                ISABase::Rv64 => rng.random::<u64>(),
            })
            .collect()
    };

    let fp_values: Vec<u64> = if extension_map.support_float(&isa_base) {
        let (fp_start, fp_end) = register_config.floating_point_register_range;
        (fp_start..=fp_end)
            .map(|_| match isa_base {
                ISABase::Rv32 => rng.random::<u32>() as u64,
                ISABase::Rv64 => rng.random::<u64>(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let vector_values: Vec<u64> = if support_vector {
        let (vec_start, vec_end) = register_config.vector_register_range;
        if vec_start > vec_end {
            Vec::new()
        } else {
            (vec_start..=vec_end)
                .map(|_| match isa_base {
                    ISABase::Rv32 => rng.random::<u32>() as u64,
                    ISABase::Rv64 => rng.random::<u64>(),
                })
                .collect()
        }
    } else {
        Vec::new()
    };

    // Use one implementation's range to determine how much memory to initialize
    // All implementations have the same size (inclusive range), only bases differ
    let word_size = isa_base.word_size() as u64;
    let word_count = mem_range
        .values()
        .next()
        .map(|(start, end)| (end - start + 1) / word_size)
        .ok_or(GenerationError::EmptyMemRange)?;

    // Generate shared memory initialization data (all implementations use the same sequence)
    let mem_data: Vec<u64> = (0..word_count)
        .map(|_| match isa_base {
            ISABase::Rv32 => rng.random::<u32>() as u64,
            ISABase::Rv64 => rng.random::<u64>(),
        })
        .collect();

    // Generate initialization instructions for each implementation
    let mut result = HashMap::new();

    // Obtain temporary register numbers
    let (temp_reg_start, temp_reg_end) = temp_register_num_range;
    let temp_regs: Vec<u8> = (temp_reg_start..=temp_reg_end).collect();

    // Ensure at least three temporary registers are available
    if temp_regs.len() < 3 {
        return Err(GenerationError::InsufficientTempRegisters {
            required: 3,
            available: temp_regs.len(),
        });
    }

    let temp_reg0 = temp_regs[0]; // used for floating-point init
    let temp_reg1 = temp_regs[1]; // used for memory base address
    let temp_reg2 = temp_regs[2]; // used for memory data

    for impl_ref in riscv_impl_vec.iter() {
        let mut block = InstructionBlock::new();

        // 1. Initialize integer registers
        let (int_start, _) = register_config.integer_register_range;
        for (i, &value) in int_values.iter().enumerate() {
            let reg_num = int_start + i as u8;
            block.push(format!("li x{}, {:#x}", reg_num, value as i64), None);
        }

        // 2. Initialize floating-point registers
        let (fp_start, _) = register_config.floating_point_register_range;
        for (i, &value) in fp_values.iter().enumerate() {
            let reg_num = fp_start + i as u8;
            block.push(format!("li x{}, {:#x}", temp_reg0, value as i64), None);

            match isa_base {
                ISABase::Rv32 => {
                    block.push(format!("fmv.w.x f{}, x{}", reg_num, temp_reg0), None);
                }
                ISABase::Rv64 => {
                    block.push(format!("fmv.d.x f{}, x{}", reg_num, temp_reg0), None);
                }
            }
        }

        if support_vector && !vector_values.is_empty() {
            let vector_element_width = match isa_base {
                ISABase::Rv32 => "e32",
                ISABase::Rv64 => "e64",
            };
            block.push(
                format!("vsetvli x{}, x0, {}, m1", temp_reg1, vector_element_width),
                None,
            );

            let (vec_start, _) = register_config.vector_register_range;
            for (i, &value) in vector_values.iter().enumerate() {
                let vreg_num = vec_start + i as u8;
                let signed_value = match isa_base {
                    ISABase::Rv32 => (value as u32) as i64,
                    ISABase::Rv64 => value as i64,
                };
                block.push(format!("li x{}, {:#x}", temp_reg0, signed_value), None);
                block.push(format!("vmv.v.x v{}, x{}", vreg_num, temp_reg0), None);
            }
        }

        // 3. Initialize memory (inclusive range [start, end])
        if !mem_data.is_empty() {
            let (start, _end) =
                mem_range
                    .get(impl_ref)
                    .ok_or_else(|| GenerationError::MemRangeNotFound {
                        impl_name: format!("{:?}", impl_ref),
                    })?;

            // Optimization: load the base address once, then store with offsets to reduce instruction count
            block.push(format!("li x{}, {:#x}", temp_reg1, *start as i64), None);

            // Iterate over each word to initialize
            for (i, &data) in mem_data.iter().enumerate() {
                let offset = (i as u64) * word_size;
                block.push(format!("li x{}, {:#x}", temp_reg2, data as i64), None);

                match isa_base {
                    ISABase::Rv32 => {
                        let offset_i64 = i64::try_from(offset).ok();
                        block.push(
                            format!("sw x{}, {}(x{})", temp_reg2, offset, temp_reg1),
                            offset_i64,
                        );
                    }
                    ISABase::Rv64 => {
                        let offset_i64 = i64::try_from(offset).ok();
                        block.push(
                            format!("sd x{}, {}(x{})", temp_reg2, offset, temp_reg1),
                            offset_i64,
                        );
                    }
                }
            }
        }

        for &temp_reg in &temp_regs {
            block.push(format!("li x{}, 0", temp_reg), None);
        }

        result.insert(*impl_ref, block);
    }

    Ok(result)
}

/// Generate restore instructions from a register context to reconstruct state during replay.
pub fn generate_register_context_restore_instructions(
    register_context: &HashMap<String, u64>,
    isa_base: ISABase,
    temp_register_num_range: (u8, u8),
) -> Result<Vec<String>, GenerationError> {
    if register_context.is_empty() {
        return Ok(Vec::new());
    }

    let (temp_start, temp_end) = temp_register_num_range;
    let temp_registers: Vec<u8> = (temp_start..=temp_end).collect();

    let needs_float_helper = register_context.keys().any(|name| name.starts_with('f'));
    if needs_float_helper && temp_registers.is_empty() {
        return Err(GenerationError::InsufficientTempRegisters {
            required: 1,
            available: 0,
        });
    }

    let float_helper = temp_registers.get(0).copied();

    let mut entries: Vec<(&String, &u64)> = register_context.iter().collect();
    entries.sort_by(|(left, _), (right, _)| {
        fn reg_key(name: &str) -> (char, u8) {
            let mut chars = name.chars();
            if let Some(prefix) = chars.next() {
                if let Ok(index) = chars.as_str().parse::<u8>() {
                    return (prefix, index);
                }
                return (prefix, 0);
            }
            ('~', 0)
        }

        reg_key(left).cmp(&reg_key(right))
    });

    let mut instructions = Vec::new();

    for (name, &value) in entries {
        let mut chars = name.chars();
        let prefix = match chars.next() {
            Some(p) => p,
            None => continue,
        };

        let digits = chars.as_str();
        let parsed_index = digits.parse::<u8>();

        match prefix {
            'x' => {
                let reg_index = match parsed_index {
                    Ok(idx) => idx,
                    Err(err) => {
                        warn!(
                            "skip register context '{}' due to parse error: {}",
                            name, err
                        );
                        continue;
                    }
                };

                if reg_index == 0 {
                    // x0 is always zero; no need to restore
                    continue;
                }

                let signed_value = if matches!(isa_base, ISABase::Rv32) {
                    (value as u32) as i64
                } else {
                    value as i64
                };

                instructions.push(format!("li x{}, {:#x}", reg_index, signed_value));
            }
            'f' => {
                let helper = match float_helper {
                    Some(reg) => reg,
                    None => {
                        return Err(GenerationError::InsufficientTempRegisters {
                            required: 1,
                            available: 0,
                        });
                    }
                };

                let reg_index = match parsed_index {
                    Ok(idx) => idx,
                    Err(err) => {
                        warn!("skip float context '{}' due to parse error: {}", name, err);
                        continue;
                    }
                };

                let signed_value = if matches!(isa_base, ISABase::Rv32) {
                    (value as u32) as i64
                } else {
                    value as i64
                };

                instructions.push(format!("li x{}, {:#x}", helper, signed_value));
                match isa_base {
                    ISABase::Rv32 => {
                        instructions.push(format!("fmv.w.x f{}, x{}", reg_index, helper));
                    }
                    ISABase::Rv64 => {
                        instructions.push(format!("fmv.d.x f{}, x{}", reg_index, helper));
                    }
                }
            }
            'v' => {
                // There is no simple way to restore vector registers yet; skip and log
                warn!(
                    "skip vector register '{}' while generating context recovery instructions",
                    name
                );
            }
            _ => {
                warn!(
                    "skip unsupported register '{}' while generating context recovery instructions",
                    name
                );
            }
        }
    }

    Ok(instructions)
}

/// Generate restore instructions from a memory context, writing bytes back using temp registers.
pub fn generate_memory_context_restore_instructions(
    memory_context: &BTreeMap<u64, u8>,
    mem_start: u64,
    temp_register_num_range: (u8, u8),
) -> Result<Vec<String>, GenerationError> {
    if memory_context.is_empty() {
        return Ok(Vec::new());
    }

    let (temp_start, temp_end) = temp_register_num_range;
    let temp_registers: Vec<u8> = (temp_start..=temp_end).collect();

    if temp_registers.len() < 2 {
        return Err(GenerationError::InsufficientTempRegisters {
            required: 2,
            available: temp_registers.len(),
        });
    }

    let addr_reg = temp_registers[0];
    let value_reg = temp_registers[1];
    let mut instructions = Vec::new();

    for (&offset, &byte_value) in memory_context {
        let absolute_addr =
            mem_start
                .checked_add(offset)
                .ok_or(GenerationError::MemoryAddressOutOfRange {
                    addr: mem_start.saturating_add(offset),
                })?;

        instructions.push(format!("li x{}, {:#x}", addr_reg, absolute_addr as i64));
        instructions.push(format!(
            "li x{}, {:#x}",
            value_reg,
            i64::from(byte_value as i8)
        ));
        instructions.push(format!("sb x{}, 0(x{})", value_reg, addr_reg));
    }

    Ok(instructions)
}

pub fn remove_special_instructions(
    instructions: Vec<InstructionSequence<RiscvInstruction>>,
) -> Vec<InstructionSequence<RiscvInstruction>> {
    instructions
        .into_iter()
        .filter(|instruction| {
            match &instruction.instruction {
                RiscvInstruction::RV32(rv32_instr) => {
                    match rv32_instr {
                        // Group RV32 instructions by extension and match specific instructions
                        RV32Instruction::I(instr) => {
                            match instr {
                                RV32IInstructions::JAL(_)
                                | RV32IInstructions::JALR(_)
                                | RV32IInstructions::BEQ(_)
                                | RV32IInstructions::BNE(_)
                                | RV32IInstructions::BLT(_)
                                | RV32IInstructions::BGE(_)
                                | RV32IInstructions::BLTU(_)
                                | RV32IInstructions::BGEU(_)
                                | RV32IInstructions::ECALL(_)
                                | RV32IInstructions::EBREAK(_)
                                | RV32IInstructions::MRET(_)
                                // Drop WFI even though it's not a branch; it usually waits for events or interrupts
                                | RV32IInstructions::WFI(_)
                                |RV32IInstructions::AUIPC(_)
                                 => false,
                                _ => true, // Keep other non-branch RV32I instructions
                            }
                        }
                        RV32Instruction::C(instr) => {
                            match instr {
                                RV32CInstructions::C_J(_)
                                | RV32CInstructions::C_ADDI4SPN(_)
                                | RV32CInstructions::C_JAL(_)
                                | RV32CInstructions::C_JR(_)
                                | RV32CInstructions::C_JALR(_)
                                | RV32CInstructions::C_BEQZ(_)
                                | RV32CInstructions::C_BNEZ(_) => false,
                                _ => true, // Keep other non-branch RV32C instructions
                            }
                        }
                        RV32Instruction::S(instr) => {
                            match instr {
                                RV32SInstructions::SRET(_) => false,
                                RV32SInstructions::SFENCE_VMA(_) => false, // Indirectly affects control flow
                            }
                        }
                        RV32Instruction::Sdext(instr) => match instr {
                            RV32SdextInstructions::DRET(_) => false,
                        },
                        RV32Instruction::Smrnmi(instr) => match instr {
                            RV32SmrnmiInstructions::MNRET(_) => false,
                        },
                        RV32Instruction::Zalrsc(instr) => {
                            // LR/SC are not branches but often pair with branches to retry atomics
                            match instr {
                                RV32ZalrscInstructions::LR_W(_)
                                | RV32ZalrscInstructions::SC_W(_) => false,
                            }
                        }
                        RV32Instruction::V(instr) => {
                            // VSETVLI/VSETVL/VSETIVLI change vector configuration and affect control flow
                            match instr {
                                RV32VInstructions::VSETVLI(_)
                                | RV32VInstructions::VSETVL(_)
                                | RV32VInstructions::VSETIVLI(_) => false,
                                _ => true, // Keep other RV32V instructions
                            }
                        }
                        // Handle other instructions that may indirectly affect control flow
                        RV32Instruction::Svinval(instr) => {
                            // Svinval instructions invalidate TLB/cache and can indirectly affect fetch
                            match instr {
                                RV32SvinvalInstructions::SFENCE_W_INVAL(_)
                                | RV32SvinvalInstructions::HINVAL_VVMA(_)
                                | RV32SvinvalInstructions::SFENCE_INVAL_IR(_)
                                | RV32SvinvalInstructions::HINVAL_GVMA(_)
                                | RV32SvinvalInstructions::SINVAL_VMA(_) => false,
                            }
                        }
                        RV32Instruction::Zicsr(instr) => !rv32_zicsr_writes_forbidden_csr(instr),
                        RV32Instruction::H(instr) => {
                            // Hypervisor instructions manage virtual memory and can indirectly affect fetch
                            match instr {
                                RV32HInstructions::HFENCE_GVMA(_)
                                | RV32HInstructions::HFENCE_VVMA(_) => false,
                                _ => true,
                            }
                        }
                        RV32Instruction::Zawrs(instr) => {
                            // Zawrs instructions wait on reservation sets and may pause execution
                            match instr {
                                RV32ZawrsInstructions::WRS_STO(_)
                                | RV32ZawrsInstructions::WRS_NTO(_) => false,
                            }
                        }
                        // Other extensions (Zicond, Zicfilp, Zicbom, Zicboz, Zcmop, Zimop, Zcmp, Zvbb, Zvks, Zvkned, Zvknha, Zbkx, Zbb, Zbc, Zabha, Zacas, Zknh, Zks, Zkne, Zfbfmin, Zvfbfwma, Zcd, F, D, Q, B)
                        // typically do not change control flow, so keep them by default.
                        // Add finer-grained filters per extension if needed.
                        _ => true, // Keep all other RV32 extension instructions
                    }
                }
                RiscvInstruction::RV64(rv64_instr) => {
                    match rv64_instr {
                        // Group RV64 instructions by extension and match specific instructions
                        RV64Instruction::I(instr) => {
                            match instr {
                                RV64IInstructions::JAL(_)
                                | RV64IInstructions::JALR(_)
                                | RV64IInstructions::BEQ(_)
                                | RV64IInstructions::BNE(_)
                                | RV64IInstructions::BLT(_)
                                | RV64IInstructions::BGE(_)
                                | RV64IInstructions::BLTU(_)
                                | RV64IInstructions::BGEU(_)
                                | RV64IInstructions::ECALL(_)
                                | RV64IInstructions::EBREAK(_)
                                | RV64IInstructions::MRET(_)
                                | RV64IInstructions::WFI(_)
                                | RV64IInstructions::AUIPC(_) => false,
                                _ => true, // Keep other non-branch RV64I instructions
                            }
                        }
                        RV64Instruction::C(instr) => {
                            match instr {
                                RV64CInstructions::C_J(_) | RV64CInstructions::C_ADDI4SPN(_) => {
                                    false
                                }
                                RV64CInstructions::C_JALR(_)
                                | RV64CInstructions::C_JR(_)
                                | RV64CInstructions::C_BEQZ(_)
                                | RV64CInstructions::C_BNEZ(_) => false,
                                // Although C_LDSP, C_LD, C_SDSP, C_SD touch the stack, treat them as non-control-flow here
                                _ => true, // Keep other non-branch RV64C instructions
                            }
                        }
                        RV64Instruction::S(instr) => {
                            match instr {
                                RV64SInstructions::SRET(_) => false,
                                RV64SInstructions::SFENCE_VMA(_) => false, // Indirectly affects control flow
                            }
                        }
                        RV64Instruction::Sdext(instr) => match instr {
                            RV64SdextInstructions::DRET(_) => false,
                        },
                        RV64Instruction::Smrnmi(instr) => match instr {
                            RV64SmrnmiInstructions::MNRET(_) => false,
                        },
                        RV64Instruction::Zalrsc(instr) => {
                            // Treat LR/SC the same way
                            match instr {
                                RV64ZalrscInstructions::LR_W(_)
                                | RV64ZalrscInstructions::SC_W(_)
                                | RV64ZalrscInstructions::LR_D(_)
                                | RV64ZalrscInstructions::SC_D(_) => false,
                            }
                        }
                        RV64Instruction::V(instr) => {
                            // VSETVLI/VSETVL/VSETIVLI change vector configuration and affect control flow
                            match instr {
                                RV64VInstructions::VSETVLI(_)
                                | RV64VInstructions::VSETVL(_)
                                | RV64VInstructions::VSETIVLI(_) => false,
                                _ => true, // Keep other RV64V instructions
                            }
                        }
                        // Handle other instructions that may indirectly affect control flow
                        RV64Instruction::Svinval(instr) => {
                            // Svinval instructions
                            match instr {
                                RV64SvinvalInstructions::SFENCE_W_INVAL(_)
                                | RV64SvinvalInstructions::HINVAL_VVMA(_)
                                | RV64SvinvalInstructions::SFENCE_INVAL_IR(_)
                                | RV64SvinvalInstructions::HINVAL_GVMA(_)
                                | RV64SvinvalInstructions::SINVAL_VMA(_) => false,
                            }
                        }
                        RV64Instruction::Zicsr(instr) => !rv64_zicsr_writes_forbidden_csr(instr),
                        RV64Instruction::H(instr) => {
                            // Hypervisor instructions
                            match instr {
                                RV64HInstructions::HFENCE_GVMA(_)
                                | RV64HInstructions::HFENCE_VVMA(_) => false,
                                _ => true,
                            }
                        }
                        RV64Instruction::Zawrs(instr) => {
                            // Zawrs instructions
                            match instr {
                                RV64ZawrsInstructions::WRS_STO(_)
                                | RV64ZawrsInstructions::WRS_NTO(_) => false,
                            }
                        }
                        // Keep other extensions by default
                        _ => true, // Keep all other RV64 extension instructions
                    }
                }
            }
        })
        .collect()
}

fn rv32_zicsr_writes_forbidden_csr(instr: &RV32ZicsrInstructions) -> bool {
    use RV32ZicsrInstructions::*;

    let (csr, reads_csr, writes_csr) = match instr {
        CSRRW(inner) => (inner.csr, inner.xd.get() != 0, true),
        CSRRWI(inner) => (inner.csr, inner.xd.get() != 0, true),
        CSRRS(inner) => {
            let writes = inner.xs1.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRSI(inner) => {
            let writes = inner.uimm.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRC(inner) => {
            let writes = inner.xs1.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRCI(inner) => {
            let writes = inner.uimm.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
    };

    csr_access_is_forbidden(csr, reads_csr, writes_csr)
}

fn rv64_zicsr_writes_forbidden_csr(instr: &RV64ZicsrInstructions) -> bool {
    use RV64ZicsrInstructions::*;

    let (csr, reads_csr, writes_csr) = match instr {
        CSRRW(inner) => (inner.csr, inner.xd.get() != 0, true),
        CSRRWI(inner) => (inner.csr, inner.xd.get() != 0, true),
        CSRRS(inner) => {
            let writes = inner.xs1.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRSI(inner) => {
            let writes = inner.uimm.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRC(inner) => {
            let writes = inner.xs1.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
        CSRRCI(inner) => {
            let writes = inner.uimm.get() != 0;
            (inner.csr, inner.xd.get() != 0, writes)
        }
    };

    csr_access_is_forbidden(csr, reads_csr, writes_csr)
}

fn csr_access_is_forbidden(csr: WritableCsr, reads_csr: bool, _writes_csr: bool) -> bool {
    if reads_csr
        && matches!(
            csr,
            WritableCsr::Menvcfg | WritableCsr::Menvcfgh | WritableCsr::Mcounteren
        )
    {
        return true;
    }

    matches!(
        csr,
        WritableCsr::Mtvec
            | WritableCsr::Misa
            | WritableCsr::Mstatus
            | WritableCsr::Mie
            | WritableCsr::Mip
            | WritableCsr::Mepc
            | WritableCsr::Sstatus
            | WritableCsr::Sie
            | WritableCsr::Sip
            | WritableCsr::Hie
            | WritableCsr::Hip
            | WritableCsr::Hvip
            | WritableCsr::Vsstatus
            | WritableCsr::Vsie
            | WritableCsr::Vsip
            | WritableCsr::Pmpcfg0
            | WritableCsr::Pmpcfg1
            | WritableCsr::Pmpcfg2
            | WritableCsr::Pmpcfg3
            | WritableCsr::Pmpaddr0
            | WritableCsr::Pmpaddr1
            | WritableCsr::Pmpaddr2
            | WritableCsr::Pmpaddr3
            | WritableCsr::Pmpaddr4
            | WritableCsr::Pmpaddr5
            | WritableCsr::Pmpaddr6
            | WritableCsr::Pmpaddr7
            | WritableCsr::Pmpaddr8
            | WritableCsr::Pmpaddr9
            | WritableCsr::Pmpaddr10
            | WritableCsr::Pmpaddr11
            | WritableCsr::Pmpaddr12
            | WritableCsr::Pmpaddr13
            | WritableCsr::Pmpaddr14
            | WritableCsr::Pmpaddr15
    )
}
