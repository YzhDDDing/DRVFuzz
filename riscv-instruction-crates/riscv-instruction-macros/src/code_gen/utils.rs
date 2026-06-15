use super::{CodeGenerator, InstructionVariant};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use riscv_instruction_parser::types::{ISABase, Instruction, Operand};
use syn::Ident;

impl CodeGenerator {
    /// Generate import statements
    pub fn generate_imports(&self) -> TokenStream {
        quote! {
            use std::fmt::{self, Display};
            pub use riscv_instruction_types::*;
            use riscv_instruction_macros::{DeriveValidatedValue, DeriveInstructionDisplay, DeriveRandom};
            use serde::{Deserialize, Serialize};
            use enum_iterator::Sequence;
        }
    }

    pub fn instruction_name_to_variant(&self, name: &str) -> String {
        name.replace('.', "_").replace('-', "_").to_uppercase()
    }

    pub fn operand_to_typed_struct(
        &self,
        operand: &Operand,
        isa_base: &ISABase,
        instruction: &riscv_instruction_parser::types::Instruction,
    ) -> String {
        let bit_length = operand.bit_lengths.get(isa_base).unwrap_or_else(|| {
            panic!(
                "No bit length defined for operand '{}' in ISA base '{}'",
                operand.name, isa_base
            )
        });
        let restrictions = operand.restrictions.as_ref();

        // Check whether this operand is a memory-offset operand
        let is_memory_aware = instruction
            .memory_access
            .as_ref()
            .map(|ma| ma.offset_operand.as_ref() == Some(&operand.name))
            .unwrap_or(false);

        // Check whether this operand is a memory base register
        let is_base_address_register = instruction
            .memory_access
            .as_ref()
            .map(|ma| ma.base_register_operand.as_ref() == Some(&operand.name))
            .unwrap_or(false);

        // operand_type must be present
        let operand_type = operand
            .operand_type
            .as_ref()
            .unwrap_or_else(|| panic!("No operand_type defined for operand '{}'", operand.name));

        match operand_type {
            riscv_instruction_parser::types::OperandType::IntegerRegister => {
                                        // If this is an unconstrained base register, use the special base-register type
                                        if is_base_address_register && restrictions.is_none() {
                                            return match *bit_length {
                                                5 => "BaseAddressRegister".to_string(),          // regular instructions: x0-x31
                                                3 => "CompressedBaseAddressRegister".to_string(), // compressed instructions: x8-x15
                                                _ => unreachable!("Invalid bit length for base address register: {}", bit_length),
                                            };
                                        }

                                        let base_type =
                                            self.get_register_base_type_from_operand_type(operand_type, *bit_length);

                                        if let Some(restriction) = restrictions {
                                            if !restriction.forbidden_values.is_empty()
                                                || restriction.multiple_of.is_some()
                                                || restriction.min_max.is_some()
                                                || restriction.odd_only.unwrap_or(false) {
                                                return self.generate_restricted_register_type_name(
                                                    base_type,
                                                    &operand.name,
                                                    restriction,
                                                );
                                            }
                                        }
                                        base_type.to_string()
                                    }
            riscv_instruction_parser::types::OperandType::FloatingPointRegister => {
                                        let base_type =
                                            self.get_register_base_type_from_operand_type(operand_type, *bit_length);

                                        if let Some(restriction) = restrictions {
                                            if !restriction.forbidden_values.is_empty()
                                                || restriction.multiple_of.is_some()
                                                || restriction.min_max.is_some()
                                                || restriction.odd_only.unwrap_or(false) {
                                                return self.generate_restricted_register_type_name(
                                                    base_type,
                                                    &operand.name,
                                                    restriction,
                                                );
                                            }
                                        }
                                        base_type.to_string()
                                    }
            riscv_instruction_parser::types::OperandType::VectorRegister => {
                                        if let Some(restriction) = restrictions {
                                            if !restriction.forbidden_values.is_empty()
                                                || restriction.multiple_of.is_some()
                                                || restriction.min_max.is_some()
                                                || restriction.odd_only.unwrap_or(false) {
                                                return self.generate_restricted_register_type_name(
                                                    "VectorRegister",
                                                    &operand.name,
                                                    restriction,
                                                );
                                            }
                                        }
                                        "VectorRegister".to_string()
                                    }
            riscv_instruction_parser::types::OperandType::CSRAddress => {
                self.map_csr_operand_type(instruction, operand)
            }
            riscv_instruction_parser::types::OperandType::RoundMode => "RoundingMode".to_string(),
            riscv_instruction_parser::types::OperandType::FenceMode => "FenceMode".to_string(),
            riscv_instruction_parser::types::OperandType::SignedInteger => {
                                        if let Some(restriction) = restrictions {
                                            if restriction.min_max.is_some()
                                                || restriction.multiple_of.is_some()
                                                || !restriction.forbidden_values.is_empty()
                                                || restriction.odd_only.unwrap_or(false)
                                            {
                                                return format!("{}< {} >", 
                                                    self.generate_restricted_immediate_type_name(
                                                        operand_type,
                                                        &operand.name,
                                                        *bit_length,
                                                        restriction,
                                                    ),
                                                    is_memory_aware
                                                );
                                            }
                                        }
                                        format!("Immediate<{}, {}>", bit_length, is_memory_aware)
                                    }
            riscv_instruction_parser::types::OperandType::UnsignedInteger => {
                                        // Special-case a 1-bit boolean
                                        if *bit_length == 1 {
                                            return "bool".to_string();
                                        }

                                        if let Some(restriction) = restrictions {
                                            if restriction.min_max.is_some()
                                                || restriction.multiple_of.is_some()
                                                || !restriction.forbidden_values.is_empty()
                                                || restriction.odd_only.unwrap_or(false)
                                            {
                                                return format!("{}< {} >", 
                                                    self.generate_restricted_immediate_type_name(
                                                        operand_type,
                                                        &operand.name,
                                                        *bit_length,
                                                        restriction,
                                                    ),
                                                    is_memory_aware
                                                );
                                            }
                                        }

                                        format!("UImmediate<{}, {}>", bit_length, is_memory_aware)
                                    }
            riscv_instruction_parser::types::OperandType::FliConstant => "FliConstant".to_string(),
            riscv_instruction_parser::types::OperandType::SavedRegListWithStackAdj => {
                        match isa_base {
                            ISABase::RV32 => "SavedRegListWithStackAdjRv32".to_string(),
                            ISABase::RV64 => "SavedRegListWithStackAdjRv64".to_string(),
                            // ISABase::RV128 => "SavedRegListWithStackAdjRv128".to_string(), // Placeholder for future RV128 support
                        }
                    }
            riscv_instruction_parser::types::OperandType::SavedIntegerRegister => {
                        match *bit_length {
                            3 => "CompressedSavedIntegerRegister".to_string(),
                            5 => "SavedIntegerRegister".to_string(),
                            _ => unreachable!("Invalid bit length for saved integer register: {}", bit_length),
                        }
                    }
            riscv_instruction_parser::types::OperandType::NotEqualCompressedSavedIntegerRegisterPair => "NotEqualCompressedSavedIntegerRegisterPair".to_string(),
        }
    }

    /// Get the extension associated with a variant
    pub fn get_extension_for_variant(&self, variant: &InstructionVariant) -> String {
        variant.instruction.extension.to_string()
    }

    /// Build enum variants
    pub fn build_variants(
        &self,
        variants: &[&InstructionVariant],
        isa_base_override: Option<&ISABase>,
    ) -> Vec<TokenStream> {
        variants
            .iter()
            .map(|variant| {
                let variant_name = Ident::new(
                    &self.instruction_name_to_variant(&variant.instruction.name),
                    Span::call_site(),
                );

                // Create struct names with prefixes to avoid collisions
                let struct_name = if variant.is_shared {
                    format!(
                        "{}_Shared_{}",
                        self.get_extension_for_variant(variant),
                        self.instruction_name_to_variant(&variant.instruction.name)
                    )
                } else {
                    let isa_base = isa_base_override.unwrap_or_else(|| &variant.isa_bases[0]);
                    format!(
                        "{}_{}_{}",
                        isa_base,
                        self.get_extension_for_variant(variant),
                        self.instruction_name_to_variant(&variant.instruction.name)
                    )
                };

                let struct_ident = Ident::new(&struct_name, Span::call_site());

                quote! {
                    #variant_name(#struct_ident)
                }
            })
            .collect()
    }

    fn map_csr_operand_type(&self, instruction: &Instruction, operand: &Operand) -> String {
        if operand.name.to_ascii_lowercase() != "csr" {
            return "CSRAddress".to_string();
        }

        match instruction.name.as_str() {
            "csrrw" | "csrrs" | "csrrc" | "csrrwi" | "csrrsi" | "csrrci" => {
                "WritableCsr".to_string()
            }
            _ => "ReadableCsr".to_string(),
        }
    }

    pub fn generate_instruction_enum_offset_impl(
        &self,
        enum_name: &Ident,
        variants: &[&InstructionVariant],
    ) -> TokenStream {
        if variants.is_empty() {
            return quote! {
                impl MemoryAccessInstruction for #enum_name {
                    fn offset_operand_value(&self) -> Option<i64> {
                        None
                    }
                }
            };
        }

        let match_arms = variants
            .iter()
            .map(|variant| {
                let variant_ident = Ident::new(
                    &self.instruction_name_to_variant(&variant.instruction.name),
                    Span::call_site(),
                );

                if variant.instruction.memory_access.is_some() {
                    quote! {
                        Self::#variant_ident(inner) => MemoryAccessInstruction::offset_operand_value(inner)
                    }
                } else {
                    quote! {
                        Self::#variant_ident(_) => None
                    }
                }
            })
            .collect::<Vec<_>>();

        quote! {
            impl MemoryAccessInstruction for #enum_name {
                fn offset_operand_value(&self) -> Option<i64> {
                    match self {
                        #(#match_arms),*
                    }
                }
            }
        }
    }

    pub fn generate_wrapper_enum_offset_impl(
        &self,
        enum_name: &Ident,
        variant_idents: &[Ident],
    ) -> TokenStream {
        if variant_idents.is_empty() {
            return quote! {
                impl MemoryAccessInstruction for #enum_name {
                    fn offset_operand_value(&self) -> Option<i64> {
                        None
                    }
                }
            };
        }

        let match_arms = variant_idents
            .iter()
            .map(|variant_ident| {
                quote! {
                    Self::#variant_ident(inner) => MemoryAccessInstruction::offset_operand_value(inner)
                }
            })
            .collect::<Vec<_>>();

        quote! {
            impl MemoryAccessInstruction for #enum_name {
                fn offset_operand_value(&self) -> Option<i64> {
                    match self {
                        #(#match_arms),*
                    }
                }
            }
        }
    }

    pub fn generate_separated_instruction_enum_offset_impl(
        &self,
        enum_name: &Ident,
        instructions: &[&Instruction],
    ) -> TokenStream {
        if instructions.is_empty() {
            return quote! {
                impl MemoryAccessInstruction for #enum_name {
                    fn offset_operand_value(&self) -> Option<i64> {
                        None
                    }
                }
            };
        }

        let match_arms = instructions
            .iter()
            .map(|inst| {
                let variant_ident = Ident::new(
                    &self.instruction_name_to_variant(&inst.name),
                    Span::call_site(),
                );

                if inst.memory_access.is_some() {
                    quote! {
                        Self::#variant_ident(inner) => MemoryAccessInstruction::offset_operand_value(inner)
                    }
                } else {
                    quote! {
                        Self::#variant_ident(_) => None
                    }
                }
            })
            .collect::<Vec<_>>();

        quote! {
            impl MemoryAccessInstruction for #enum_name {
                fn offset_operand_value(&self) -> Option<i64> {
                    match self {
                        #(#match_arms),*
                    }
                }
            }
        }
    }
}
