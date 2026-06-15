use super::{CodeGenerator, InstructionVariant};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use riscv_instruction_parser::types::{ISABase, ISAExtension, Instruction};
use std::collections::{HashMap, HashSet};
use syn::Ident;

impl CodeGenerator {
    /// Generate all instruction structs
    pub fn generate_instruction_structs(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_structs = TokenStream::new();
        let mut processed_instructions = HashSet::new();

        for (extension, variants) in analysis {
            for variant in variants {
                let inst_name = &variant.instruction.name;

                // Create struct names with prefixes to avoid collisions
                let struct_name_base = self.instruction_name_to_variant(inst_name);
                let struct_name = if variant.is_shared {
                    format!("{}_Shared_{}", extension, struct_name_base)
                } else {
                    let isa_base = &variant.isa_bases[0];
                    format!("{}_{}_{}", isa_base, extension, struct_name_base)
                };

                if processed_instructions.contains(&struct_name) {
                    continue;
                }
                processed_instructions.insert(struct_name.clone());

                let struct_ident = Ident::new(&struct_name, Span::call_site());
                let doc_comment = format!("{} instruction for {} extension", inst_name, extension);
                let memory_access_info = variant.instruction.memory_access.as_ref();

                let attribute_token = match &variant.instruction.assembly_syntax {
                    riscv_instruction_parser::types::AssemblySyntax::RustCode(code) => {
                        let code_str = code.as_str();
                        quote! { #[asm_code(#code_str)] }
                    }
                    riscv_instruction_parser::types::AssemblySyntax::Format(format_str) => {
                        quote! { #[asm(#format_str)] }
                    }
                };

                if variant.instruction.operands.is_empty() {
                    all_structs.extend(quote! {
                        #[doc = #doc_comment]
                        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                        #attribute_token
                        pub struct #struct_ident;
                    });
                } else {
                    let isa_base = if variant.is_shared {
                        &variant.isa_bases[0] // Use the first ISA base for shared instructions
                    } else {
                        &variant.isa_bases[0]
                    };

                    let fields = variant
                        .instruction
                        .operands
                        .iter()
                        .map(|op| {
                            let op_name = Ident::new(&op.name, Span::call_site());
                            let op_type_str =
                                self.operand_to_typed_struct(op, isa_base, &variant.instruction);
                            let op_type: TokenStream = op_type_str
                                .parse()
                                .expect("Failed to parse operand type string");
                            quote! { pub #op_name: #op_type }
                        })
                        .collect::<Vec<_>>();

                    all_structs.extend(quote! {
                        #[doc = #doc_comment]
                        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                        #attribute_token
                        pub struct #struct_ident {
                            #(#fields),*
                        }
                    });
                }

                if let Some(memory_access) = memory_access_info {
                    let offset_method = if let Some(offset_operand) = &memory_access.offset_operand
                    {
                        let offset_ident = Ident::new(offset_operand, Span::call_site());
                        let operand_info = variant
                            .instruction
                            .operands
                            .iter()
                            .find(|op| op.name == *offset_operand);

                        if let Some(op) = operand_info {
                            let is_bool_operand = matches!(
                                op.operand_type,
                                Some(riscv_instruction_parser::types::OperandType::UnsignedInteger)
                            ) && op.bit_lengths.values().all(|len| *len == 1);

                            let value_expr = if is_bool_operand {
                                quote! { self.#offset_ident as i64 }
                            } else {
                                quote! { self.#offset_ident.get() as i64 }
                            };

                            quote! {
                                fn offset_operand_value(&self) -> Option<i64> {
                                    Some(#value_expr)
                                }
                            }
                        } else {
                            quote! {
                                fn offset_operand_value(&self) -> Option<i64> {
                                    None
                                }
                            }
                        }
                    } else {
                        quote! {
                            fn offset_operand_value(&self) -> Option<i64> {
                                None
                            }
                        }
                    };

                    all_structs.extend(quote! {
                        impl MemoryAccessInstruction for #struct_ident {
                            #offset_method
                        }
                    });
                }
            }
        }

        all_structs
    }

    /// Generate separated instruction structs
    pub fn generate_separated_instruction_structs(&self) -> TokenStream {
        let mut all_structs = TokenStream::new();
        let mut by_extension_and_isa: HashMap<(ISAExtension, ISABase), Vec<&Instruction>> =
            HashMap::new();

        // Group by extension and ISA base
        for inst in &self.instructions {
            for &isa_base in &inst.isa_bases {
                by_extension_and_isa
                    .entry((inst.extension, isa_base))
                    .or_default()
                    .push(inst);
            }
        }

        let mut processed_instructions = HashSet::new();

        for ((extension, isa_base), instructions) in by_extension_and_isa {
            for inst in instructions {
                let struct_name = format!(
                    "{}_{}_{}",
                    isa_base,
                    extension,
                    self.instruction_name_to_variant(&inst.name)
                );

                if processed_instructions.contains(&struct_name) {
                    continue;
                }
                processed_instructions.insert(struct_name.clone());

                let struct_ident = Ident::new(&struct_name, Span::call_site());
                let doc_comment = format!("{} {} instruction: {}", isa_base, extension, inst.name);
                let memory_access_info = inst.memory_access.as_ref();

                let attribute_token = match &inst.assembly_syntax {
                    riscv_instruction_parser::types::AssemblySyntax::RustCode(code) => {
                        let code_str = code.as_str();
                        quote! { #[asm_code(#code_str)] }
                    }
                    riscv_instruction_parser::types::AssemblySyntax::Format(format_str) => {
                        quote! { #[asm(#format_str)] }
                    }
                };

                if inst.operands.is_empty() {
                    all_structs.extend(quote! {
                        #[doc = #doc_comment]
                        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                        #attribute_token
                        pub struct #struct_ident;
                    });
                } else {
                    let fields = inst
                        .operands
                        .iter()
                        .map(|op| {
                            let op_name = Ident::new(&op.name, Span::call_site());
                            let op_type_str = self.operand_to_typed_struct(op, &isa_base, inst);
                            let op_type: TokenStream = op_type_str
                                .parse()
                                .expect("Failed to parse operand type string");
                            quote! { pub #op_name: #op_type }
                        })
                        .collect::<Vec<_>>();

                    all_structs.extend(quote! {
                        #[doc = #doc_comment]
                        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                        #attribute_token
                        pub struct #struct_ident {
                            #(#fields),*
                        }
                    });
                }

                if let Some(memory_access) = memory_access_info {
                    let offset_method = if let Some(offset_operand) = &memory_access.offset_operand
                    {
                        let offset_ident = Ident::new(offset_operand, Span::call_site());
                        let operand_info =
                            inst.operands.iter().find(|op| op.name == *offset_operand);

                        if let Some(op) = operand_info {
                            let is_bool_operand = matches!(
                                op.operand_type,
                                Some(riscv_instruction_parser::types::OperandType::UnsignedInteger)
                            ) && op.bit_lengths.values().all(|len| *len == 1);

                            let value_expr = if is_bool_operand {
                                quote! { self.#offset_ident as i64 }
                            } else {
                                quote! { self.#offset_ident.get() as i64 }
                            };

                            quote! {
                                fn offset_operand_value(&self) -> Option<i64> {
                                    Some(#value_expr)
                                }
                            }
                        } else {
                            quote! {
                                fn offset_operand_value(&self) -> Option<i64> {
                                    None
                                }
                            }
                        }
                    } else {
                        quote! {
                            fn offset_operand_value(&self) -> Option<i64> {
                                None
                            }
                        }
                    };

                    all_structs.extend(quote! {
                        impl MemoryAccessInstruction for #struct_ident {
                            #offset_method
                        }
                    });
                }
            }
        }

        all_structs
    }
}
