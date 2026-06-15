use super::{CodeGenerator, InstructionVariant};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use riscv_instruction_parser::types::{ISABase, ISAExtension, Instruction};
use std::collections::{HashMap, HashSet};
use syn::Ident;

impl CodeGenerator {
    pub fn generate_instruction_enums(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_enums = TokenStream::new();

        // Generate all instruction structs first
        let instruction_structs = self.generate_instruction_structs(analysis);
        all_enums.extend(instruction_structs);

        let shared_enums = self.generate_shared_instructions_enum(analysis);
        all_enums.extend(shared_enums);

        let specific_enums = self.generate_isa_specific_instructions_enum(analysis);
        all_enums.extend(specific_enums);

        let main_enums = self.generate_extension_main_enum(analysis);
        all_enums.extend(main_enums);

        all_enums
    }

    /// Generate shared-instruction enums
    pub fn generate_shared_instructions_enum(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_enums = TokenStream::new();
        for (extension, variants) in analysis {
            let shared_variants: Vec<_> = variants.iter().filter(|v| v.is_shared).collect();
            if shared_variants.is_empty() {
                continue;
            }

            let enum_name = Ident::new(
                &format!("{}SharedInstructions", extension),
                Span::call_site(),
            );
            let doc_comment = format!("Shared instructions for {} extension", extension);

            let variant_tokens = self.build_variants(&shared_variants, None);

            all_enums.extend(quote! {
                #[doc = #doc_comment]
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                #[rustfmt::skip]
                pub enum #enum_name {
                    #(#variant_tokens),*
                }
            });

            let offset_impl =
                self.generate_instruction_enum_offset_impl(&enum_name, &shared_variants);
            all_enums.extend(offset_impl);
        }
        all_enums
    }

    /// Generate ISA-specific instruction enums
    pub fn generate_isa_specific_instructions_enum(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_enums = TokenStream::new();
        for isa_base in &[ISABase::RV32, ISABase::RV64] {
            for (extension, variants) in analysis {
                let isa_specific_variants: Vec<_> = variants
                    .iter()
                    .filter(|v| !v.is_shared && v.isa_bases.contains(isa_base))
                    .collect();

                if isa_specific_variants.is_empty() {
                    continue;
                }

                let enum_name = Ident::new(
                    &format!("{}{}SpecificInstructions", isa_base, extension),
                    Span::call_site(),
                );
                let doc_comment = format!(
                    "{} specific instructions for {} extension",
                    isa_base, extension
                );

                let variant_tokens = self.build_variants(&isa_specific_variants, Some(isa_base));

                all_enums.extend(quote! {
                    #[doc = #doc_comment]
                    #[allow(non_camel_case_types)]
                    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                    #[rustfmt::skip]
                    pub enum #enum_name {
                        #(#variant_tokens),*
                    }
                });

                let offset_impl =
                    self.generate_instruction_enum_offset_impl(&enum_name, &isa_specific_variants);
                all_enums.extend(offset_impl);
            }
        }
        all_enums
    }

    /// Generate per-extension enums
    pub fn generate_extension_main_enum(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut main_enums = TokenStream::new();

        // Generate shared instruction enums
        let mut shared_variants = Vec::new();
        let mut shared_variant_idents = Vec::new();
        for (ext, vars) in analysis {
            if vars.iter().any(|v| v.is_shared) {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let enum_name =
                    Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                shared_variant_idents.push(ext_ident.clone());
                shared_variants.push(quote! { #ext_ident(#enum_name) });
            }
        }

        if !shared_variants.is_empty() {
            let shared_enum_name = Ident::new("SharedInstruction", Span::call_site());
            let shared_doc = "Instructions shared across all ISA bases, grouped by extension.";
            main_enums.extend(quote! {
                #[doc = #shared_doc]
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #shared_enum_name {
                    #(#shared_variants),*
                }
            });

            let offset_impl =
                self.generate_wrapper_enum_offset_impl(&shared_enum_name, &shared_variant_idents);
            main_enums.extend(offset_impl);
        }

        // Generate ISA-specific enums
        for isa_base in &[ISABase::RV32, ISABase::RV64] {
            let isa_base_str = isa_base.to_string();
            let specific_enum_name = Ident::new(
                &format!("{}SpecificInstruction", isa_base),
                Span::call_site(),
            );
            let doc_comment = format!(
                "{} specific instructions, grouped by extension.",
                isa_base_str
            );

            let mut extension_variants = Vec::new();
            let mut extension_variant_idents = Vec::new();
            for (ext, vars) in analysis {
                if vars
                    .iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(isa_base))
                {
                    let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                    let enum_name = Ident::new(
                        &format!("{}{}SpecificInstructions", isa_base, ext),
                        Span::call_site(),
                    );
                    extension_variant_idents.push(ext_ident.clone());
                    extension_variants.push(quote! { #ext_ident(#enum_name) });
                }
            }

            if !extension_variants.is_empty() {
                main_enums.extend(quote! {
                    #[doc = #doc_comment]
                    #[allow(non_camel_case_types)]
                    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                    pub enum #specific_enum_name {
                        #(#extension_variants),*
                    }
                });

                let offset_impl = self.generate_wrapper_enum_offset_impl(
                    &specific_enum_name,
                    &extension_variant_idents,
                );
                main_enums.extend(offset_impl);
            }
        }

        // Generate specific instruction enums
        let mut specific_variants = Vec::new();
        let mut specific_variant_idents = Vec::new();
        for isa_base in [ISABase::RV32, ISABase::RV64].iter() {
            let has_specific = analysis.values().any(|vars| {
                vars.iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(isa_base))
            });
            if has_specific {
                let isa_base_ident = Ident::new(&isa_base.to_string(), Span::call_site());
                let enum_name = Ident::new(
                    &format!("{}SpecificInstruction", isa_base),
                    Span::call_site(),
                );
                specific_variant_idents.push(isa_base_ident.clone());
                specific_variants.push(quote! { #isa_base_ident(#enum_name) });
            }
        }

        if !specific_variants.is_empty() {
            let specific_enum_name = Ident::new("SpecificInstruction", Span::call_site());
            let specific_doc = "ISA base specific instructions.";
            main_enums.extend(quote! {
                #[doc = #specific_doc]
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #specific_enum_name {
                    #(#specific_variants),*
                }
            });

            let offset_impl = self
                .generate_wrapper_enum_offset_impl(&specific_enum_name, &specific_variant_idents);
            main_enums.extend(offset_impl);
        }

        main_enums
    }

    /// Generate the top-level instruction enum
    pub fn generate_main_enum(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut variant_tokens = Vec::new();
        let mut variant_idents = Vec::new();

        // Check whether shared instructions exist
        if analysis
            .values()
            .any(|vars| vars.iter().any(|v| v.is_shared))
        {
            variant_tokens.push(quote! {
                #[doc = "Instructions shared across ISA bases"]
                Shared(SharedInstruction)
            });
            variant_idents.push(Ident::new("Shared", Span::call_site()));
        }

        // Check whether ISA-specific instructions exist
        if analysis
            .values()
            .any(|vars| vars.iter().any(|v| !v.is_shared))
        {
            variant_tokens.push(quote! {
                #[doc = "ISA base specific instructions"]
                Specific(SpecificInstruction)
            });
            variant_idents.push(Ident::new("Specific", Span::call_site()));
        }

        let enum_name = Ident::new("RiscvInstruction", Span::call_site());
        let enum_tokens = if !variant_tokens.is_empty() {
            quote! {
                /// Main RISC-V instruction enum.
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #enum_name {
                    #(#variant_tokens),*
                }
            }
        } else {
            quote! {
                /// Main RISC-V instruction enum.
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #enum_name {}
            }
        };

        let offset_impl = self.generate_wrapper_enum_offset_impl(&enum_name, &variant_idents);

        quote! {
            #enum_tokens
            #offset_impl
        }
    }

    /// Generate fully separated instruction enums (split by extension and ISA base)
    pub fn generate_separated_enums(&self) -> TokenStream {
        let mut all_enums = TokenStream::new();
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

        // Generate an enum for each combination
        for ((extension, isa_base), instructions) in by_extension_and_isa {
            let enum_name = Ident::new(
                &format!("{}{}Instructions", isa_base, extension),
                Span::call_site(),
            );
            let doc_comment = format!("{} {} instructions", isa_base, extension);

            let variants = instructions
                .iter()
                .map(|&inst| {
                    let variant_name = Ident::new(
                        &self.instruction_name_to_variant(&inst.name),
                        Span::call_site(),
                    );

                    let struct_name = format!(
                        "{}_{}_{}",
                        isa_base,
                        extension,
                        self.instruction_name_to_variant(&inst.name)
                    );
                    let struct_ident = Ident::new(&struct_name, Span::call_site());

                    quote! {
                        #variant_name(#struct_ident)
                    }
                })
                .collect::<Vec<_>>();

            all_enums.extend(quote! {
                #[doc = #doc_comment]
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                #[rustfmt::skip]
                pub enum #enum_name {
                    #(#variants),*
                }
            });

            let offset_impl =
                self.generate_separated_instruction_enum_offset_impl(&enum_name, &instructions);
            all_enums.extend(offset_impl);
        }

        all_enums
    }

    /// Generate the separated top-level enum
    pub fn generate_separated_main_enum(&self) -> TokenStream {
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

        // Build enums grouped by ISA base
        let mut isa_enums = Vec::new();
        for isa_base in &[ISABase::RV32, ISABase::RV64] {
            let mut extensions_for_isa: Vec<_> = by_extension_and_isa
                .keys()
                .filter(|(_, base)| base == isa_base)
                .map(|(ext, _)| *ext)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            extensions_for_isa.sort();

            if !extensions_for_isa.is_empty() {
                let isa_enum_name =
                    Ident::new(&format!("{}Instruction", isa_base), Span::call_site());
                let isa_doc = format!("{} instructions grouped by extension", isa_base);

                let mut extension_variants = Vec::new();
                let mut extension_variant_idents = Vec::new();
                for &ext in &extensions_for_isa {
                    let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                    let enum_name = Ident::new(
                        &format!("{}{}Instructions", isa_base, ext),
                        Span::call_site(),
                    );
                    extension_variant_idents.push(ext_ident.clone());
                    extension_variants.push(quote! { #ext_ident(#enum_name) });
                }

                isa_enums.push(quote! {
                    #[doc = #isa_doc]
                    #[allow(non_camel_case_types)]
                    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                    pub enum #isa_enum_name {
                        #(#extension_variants),*
                    }
                });

                let offset_impl = self
                    .generate_wrapper_enum_offset_impl(&isa_enum_name, &extension_variant_idents);
                isa_enums.push(offset_impl);
            }
        }

        // Generate the final top-level enum
        let mut main_variants = Vec::new();
        let mut main_variant_idents = Vec::new();
        for isa_base in &[ISABase::RV32, ISABase::RV64] {
            let has_instructions = by_extension_and_isa
                .keys()
                .any(|(_, base)| *base == *isa_base);
            if has_instructions {
                let isa_ident = Ident::new(&isa_base.to_string(), Span::call_site());
                let enum_name = Ident::new(&format!("{}Instruction", isa_base), Span::call_site());
                main_variant_idents.push(isa_ident.clone());
                main_variants.push(quote! { #isa_ident(#enum_name) });
            }
        }

        let mut result = TokenStream::new();
        result.extend(isa_enums.into_iter());

        let main_enum_name = Ident::new("RiscvInstruction", Span::call_site());
        if !main_variants.is_empty() {
            result.extend(quote! {
                /// Main RISC-V instruction enum, separated by ISA base.
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #main_enum_name {
                    #(#main_variants),*
                }
            });
        } else {
            result.extend(quote! {
                /// Main RISC-V instruction enum.
                #[allow(non_camel_case_types)]
                #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, DeriveInstructionDisplay, DeriveRandom)]
                pub enum #main_enum_name {}
            });
        }

        let offset_impl =
            self.generate_wrapper_enum_offset_impl(&main_enum_name, &main_variant_idents);
        result.extend(offset_impl);

        result
    }

    /// Generate extension enums and corresponding Random implementations
    pub fn generate_extension_enums_with_random(&self) -> TokenStream {
        let analysis = self.analyze_instruction_sharing(&self.instructions);

        let extension_enums = self.generate_extension_enums(&analysis);
        let extension_impls = self.generate_extension_random_impls(&analysis);

        quote! {
            #extension_enums
            #extension_impls
        }
    }

    /// Generate extension enum definitions
    fn generate_extension_enums(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_enums = TokenStream::new();

        // Collect extensions grouped by ISA base
        let mut rv32_extensions = HashSet::new();
        let mut rv64_extensions = HashSet::new();

        for (extension, variants) in analysis {
            for variant in variants {
                for &isa_base in &variant.isa_bases {
                    match isa_base {
                        ISABase::RV32 => {
                            rv32_extensions.insert(*extension);
                        }
                        ISABase::RV64 => {
                            rv64_extensions.insert(*extension);
                        }
                    }
                }
            }
        }

        // Generate the RV32Extensions enum
        if !rv32_extensions.is_empty() {
            let mut rv32_variants = rv32_extensions.into_iter().collect::<Vec<_>>();
            rv32_variants.sort();

            let rv32_enum_variants = rv32_variants.iter().map(|ext| {
                let ident = Ident::new(&ext.to_string(), Span::call_site());
                quote! { #ident }
            });

            all_enums.extend(quote! {
                /// Available extensions for RV32 ISA base
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
                #[allow(non_camel_case_types)]
                pub enum RV32Extensions {
                    #(#rv32_enum_variants),*
                }
            });
        }

        // Generate the RV64Extensions enum
        if !rv64_extensions.is_empty() {
            let mut rv64_variants = rv64_extensions.into_iter().collect::<Vec<_>>();
            rv64_variants.sort();

            let rv64_enum_variants = rv64_variants.iter().map(|ext| {
                let ident = Ident::new(&ext.to_string(), Span::call_site());
                quote! { #ident }
            });

            all_enums.extend(quote! {
                /// Available extensions for RV64 ISA base
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
                #[allow(non_camel_case_types)]
                pub enum RV64Extensions {
                    #(#rv64_enum_variants),*
                }
            });
        }

        all_enums
    }

    /// Generate Random implementations for extension enums
    fn generate_extension_random_impls(
        &self,
        analysis: &HashMap<ISAExtension, Vec<InstructionVariant>>,
    ) -> TokenStream {
        let mut all_impls = TokenStream::new();

        // Collect extensions grouped by ISA base
        let mut rv32_extensions = HashSet::new();
        let mut rv64_extensions = HashSet::new();

        for (extension, variants) in analysis {
            for variant in variants {
                for &isa_base in &variant.isa_bases {
                    match isa_base {
                        ISABase::RV32 => {
                            rv32_extensions.insert(*extension);
                        }
                        ISABase::RV64 => {
                            rv64_extensions.insert(*extension);
                        }
                    }
                }
            }
        }

        // Implement Random for RV32Extensions
        if !rv32_extensions.is_empty() {
            let mut rv32_variants = rv32_extensions.into_iter().collect::<Vec<_>>();
            rv32_variants.sort();

            let rv32_match_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());

                // Check whether the extension has shared and/or specific instructions
                let variants = analysis.get(ext).unwrap();
                let has_shared = variants.iter().any(|v| v.is_shared);
                let has_rv32_specific = variants
                    .iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(&ISABase::RV32));

                if has_shared && has_rv32_specific {
                    // Both shared and ISA-specific instructions
                    let specific_enum_name = Ident::new(
                        &format!("RV32{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV32Extensions::#ext_ident => {
                            if rng.random() {
                                Ok(RiscvInstruction::Shared(SharedInstruction::#ext_ident(
                                    #shared_enum_name::random_with_rng(rng, config)?
                                )))
                            } else {
                                Ok(RiscvInstruction::Specific(SpecificInstruction::RV32(
                                    RV32SpecificInstruction::#ext_ident(
                                        #specific_enum_name::random_with_rng(rng, config)?
                                    )
                                )))
                            }
                        }
                    }
                } else if has_shared {
                    // Only shared instructions
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV32Extensions::#ext_ident => {
                            Ok(RiscvInstruction::Shared(SharedInstruction::#ext_ident(
                                #shared_enum_name::random_with_rng(rng, config)?
                            )))
                        }
                    }
                } else if has_rv32_specific {
                    // Only ISA-specific instructions
                    let specific_enum_name = Ident::new(
                        &format!("RV32{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    quote! {
                        RV32Extensions::#ext_ident => {
                            Ok(RiscvInstruction::Specific(SpecificInstruction::RV32(
                                RV32SpecificInstruction::#ext_ident(
                                    #specific_enum_name::random_with_rng(rng, config)?
                                )
                            )))
                        }
                    }
                } else {
                    // Should not happen
                    quote! {
                        RV32Extensions::#ext_ident => Err(RandomGenerationError::new(
                            stringify!(RV32Extensions),
                            concat!("No instructions for extension: ", stringify!(#ext_ident)),
                        ))
                    }
                }
            });

            let rv32_sequence_match_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());

                let variants = analysis.get(ext).unwrap();
                let has_shared = variants.iter().any(|v| v.is_shared);
                let has_rv32_specific = variants
                    .iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(&ISABase::RV32));

                if has_shared && has_rv32_specific {
                    let specific_enum_name = Ident::new(
                        &format!("RV32{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV32Extensions::#ext_ident => {
                            if rng.random() {
                                let seq = #shared_enum_name::random_sequence_with_rng(rng, config)?;
                                let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                                Ok(InstructionSequence::with_full_instructions(
                                    pre_instructions,
                                    RiscvInstruction::Shared(SharedInstruction::#ext_ident(instruction)),
                                    post_instructions,
                                ))
                            } else {
                                let seq = #specific_enum_name::random_sequence_with_rng(rng, config)?;
                                let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                                Ok(InstructionSequence::with_full_instructions(
                                    pre_instructions,
                                    RiscvInstruction::Specific(SpecificInstruction::RV32(
                                        RV32SpecificInstruction::#ext_ident(instruction),
                                    )),
                                    post_instructions,
                                ))
                            }
                        }
                    }
                } else if has_shared {
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV32Extensions::#ext_ident => {
                            let seq = #shared_enum_name::random_sequence_with_rng(rng, config)?;
                            let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                            Ok(InstructionSequence::with_full_instructions(
                                pre_instructions,
                                RiscvInstruction::Shared(SharedInstruction::#ext_ident(instruction)),
                                post_instructions,
                            ))
                        }
                    }
                } else if has_rv32_specific {
                    let specific_enum_name = Ident::new(
                        &format!("RV32{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    quote! {
                        RV32Extensions::#ext_ident => {
                            let seq = #specific_enum_name::random_sequence_with_rng(rng, config)?;
                            let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                            Ok(InstructionSequence::with_full_instructions(
                                pre_instructions,
                                RiscvInstruction::Specific(SpecificInstruction::RV32(
                                    RV32SpecificInstruction::#ext_ident(instruction),
                                )),
                                post_instructions,
                            ))
                        }
                    }
                } else {
                    quote! {
                        RV32Extensions::#ext_ident => Err(RandomGenerationError::new(
                            stringify!(RV32Extensions),
                            concat!("No instructions for extension: ", stringify!(#ext_ident)),
                        ))
                    }
                }
            });

            let rv32_instruction_count_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let variants = analysis.get(ext).unwrap();
                let count = variants
                    .iter()
                    .filter(|v| v.isa_bases.contains(&ISABase::RV32))
                    .count();

                quote! {
                    RV32Extensions::#ext_ident => #count
                }
            });

            all_impls.extend(quote! {
                impl RV32Extensions {
                    /// Generate a random instruction for this extension
                    pub fn random_instruction<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<RiscvInstruction, RandomGenerationError> {
                        match self {
                            #(#rv32_match_arms),*
                        }
                    }

                    /// Generate a random instruction sequence (including necessary pseudo instructions) for this extension
                    pub fn random_sequence_with_rng<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<InstructionSequence<RiscvInstruction>, RandomGenerationError> {
                        match self {
                            #(#rv32_sequence_match_arms),*
                        }
                    }

                    /// Return the total number of RV32 instructions provided by this extension (shared + RV32-specific)
                    pub const fn instruction_count(&self) -> usize {
                        match self {
                            #(#rv32_instruction_count_arms),*
                        }
                    }
                }
            });
        }

        // Implement Random for RV64Extensions
        if !rv64_extensions.is_empty() {
            let mut rv64_variants = rv64_extensions.into_iter().collect::<Vec<_>>();
            rv64_variants.sort();

            let rv64_match_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());

                // Check whether the extension has shared and/or specific instructions
                let variants = analysis.get(ext).unwrap();
                let has_shared = variants.iter().any(|v| v.is_shared);
                let has_rv64_specific = variants
                    .iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(&ISABase::RV64));

                if has_shared && has_rv64_specific {
                    // Both shared and ISA-specific instructions
                    let specific_enum_name = Ident::new(
                        &format!("RV64{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV64Extensions::#ext_ident => {
                            if rng.random() {
                                Ok(RiscvInstruction::Shared(SharedInstruction::#ext_ident(
                                    #shared_enum_name::random_with_rng(rng, config)?
                                )))
                            } else {
                                Ok(RiscvInstruction::Specific(SpecificInstruction::RV64(
                                    RV64SpecificInstruction::#ext_ident(
                                        #specific_enum_name::random_with_rng(rng, config)?
                                    )
                                )))
                            }
                        }
                    }
                } else if has_shared {
                    // Only shared instructions
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV64Extensions::#ext_ident => {
                            Ok(RiscvInstruction::Shared(SharedInstruction::#ext_ident(
                                #shared_enum_name::random_with_rng(rng, config)?
                            )))
                        }
                    }
                } else if has_rv64_specific {
                    // Only ISA-specific instructions
                    let specific_enum_name = Ident::new(
                        &format!("RV64{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    quote! {
                        RV64Extensions::#ext_ident => {
                            Ok(RiscvInstruction::Specific(SpecificInstruction::RV64(
                                RV64SpecificInstruction::#ext_ident(
                                    #specific_enum_name::random_with_rng(rng, config)?
                                )
                            )))
                        }
                    }
                } else {
                    // Should not happen
                    quote! {
                        RV64Extensions::#ext_ident => Err(RandomGenerationError::new(
                            stringify!(RV64Extensions),
                            concat!("No instructions for extension: ", stringify!(#ext_ident)),
                        ))
                    }
                }
            });

            let rv64_sequence_match_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());

                let variants = analysis.get(ext).unwrap();
                let has_shared = variants.iter().any(|v| v.is_shared);
                let has_rv64_specific = variants
                    .iter()
                    .any(|v| !v.is_shared && v.isa_bases.contains(&ISABase::RV64));

                if has_shared && has_rv64_specific {
                    let specific_enum_name = Ident::new(
                        &format!("RV64{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV64Extensions::#ext_ident => {
                            if rng.random() {
                                let seq = #shared_enum_name::random_sequence_with_rng(rng, config)?;
                                let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                                Ok(InstructionSequence::with_full_instructions(
                                    pre_instructions,
                                    RiscvInstruction::Shared(SharedInstruction::#ext_ident(instruction)),
                                    post_instructions,
                                ))
                            } else {
                                let seq = #specific_enum_name::random_sequence_with_rng(rng, config)?;
                                let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                                Ok(InstructionSequence::with_full_instructions(
                                    pre_instructions,
                                    RiscvInstruction::Specific(SpecificInstruction::RV64(
                                        RV64SpecificInstruction::#ext_ident(instruction),
                                    )),
                                    post_instructions,
                                ))
                            }
                        }
                    }
                } else if has_shared {
                    let shared_enum_name =
                        Ident::new(&format!("{}SharedInstructions", ext), Span::call_site());
                    quote! {
                        RV64Extensions::#ext_ident => {
                            let seq = #shared_enum_name::random_sequence_with_rng(rng, config)?;
                            let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                            Ok(InstructionSequence::with_full_instructions(
                                pre_instructions,
                                RiscvInstruction::Shared(SharedInstruction::#ext_ident(instruction)),
                                post_instructions,
                            ))
                        }
                    }
                } else if has_rv64_specific {
                    let specific_enum_name = Ident::new(
                        &format!("RV64{}SpecificInstructions", ext),
                        Span::call_site(),
                    );
                    quote! {
                        RV64Extensions::#ext_ident => {
                            let seq = #specific_enum_name::random_sequence_with_rng(rng, config)?;
                            let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                            Ok(InstructionSequence::with_full_instructions(
                                pre_instructions,
                                RiscvInstruction::Specific(SpecificInstruction::RV64(
                                    RV64SpecificInstruction::#ext_ident(instruction),
                                )),
                                post_instructions,
                            ))
                        }
                    }
                } else {
                    quote! {
                        RV64Extensions::#ext_ident => Err(RandomGenerationError::new(
                            stringify!(RV64Extensions),
                            concat!("No instructions for extension: ", stringify!(#ext_ident)),
                        ))
                    }
                }
            });

            let rv64_instruction_count_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let variants = analysis.get(ext).unwrap();
                let count = variants
                    .iter()
                    .filter(|v| v.isa_bases.contains(&ISABase::RV64))
                    .count();

                quote! {
                    RV64Extensions::#ext_ident => #count
                }
            });

            all_impls.extend(quote! {
                impl RV64Extensions {
                    /// Generate a random instruction for this extension
                    pub fn random_instruction<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<RiscvInstruction, RandomGenerationError> {
                        match self {
                            #(#rv64_match_arms),*
                        }
                    }

                    /// Generate a random instruction sequence (including necessary pseudo instructions) for this extension
                    pub fn random_sequence_with_rng<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<InstructionSequence<RiscvInstruction>, RandomGenerationError> {
                        match self {
                            #(#rv64_sequence_match_arms),*
                        }
                    }

                    /// Return the total number of RV64 instructions provided by this extension (shared + RV64-specific)
                    pub const fn instruction_count(&self) -> usize {
                        match self {
                            #(#rv64_instruction_count_arms),*
                        }
                    }
                }
            });
        }

        all_impls
    }

    /// Generate separated-mode extension enums and Random implementations
    pub fn generate_separated_extension_enums_with_random(&self) -> TokenStream {
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

        let extension_enums = self.generate_separated_extension_enums(&by_extension_and_isa);
        let extension_impls = self.generate_separated_extension_random_impls(&by_extension_and_isa);

        quote! {
            #extension_enums
            #extension_impls
        }
    }

    /// Generate separated-mode extension enum definitions
    fn generate_separated_extension_enums(
        &self,
        by_extension_and_isa: &HashMap<(ISAExtension, ISABase), Vec<&Instruction>>,
    ) -> TokenStream {
        let mut all_enums = TokenStream::new();

        // Collect extensions grouped by ISA base
        let mut rv32_extensions = HashSet::new();
        let mut rv64_extensions = HashSet::new();

        for ((extension, isa_base), _) in by_extension_and_isa {
            match isa_base {
                ISABase::RV32 => {
                    rv32_extensions.insert(*extension);
                }
                ISABase::RV64 => {
                    rv64_extensions.insert(*extension);
                }
            }
        }

        // Generate the RV32Extensions enum
        if !rv32_extensions.is_empty() {
            let mut rv32_variants = rv32_extensions.into_iter().collect::<Vec<_>>();
            rv32_variants.sort();

            let rv32_enum_variants = rv32_variants.iter().map(|ext| {
                let ident = Ident::new(&ext.to_string(), Span::call_site());
                quote! { #ident }
            });

            all_enums.extend(quote! {
                /// Available extensions for RV32 ISA base
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
                #[allow(non_camel_case_types)]
                pub enum RV32Extensions {
                    #(#rv32_enum_variants),*
                }
            });
        }

        // Generate the RV64Extensions enum
        if !rv64_extensions.is_empty() {
            let mut rv64_variants = rv64_extensions.into_iter().collect::<Vec<_>>();
            rv64_variants.sort();

            let rv64_enum_variants = rv64_variants.iter().map(|ext| {
                let ident = Ident::new(&ext.to_string(), Span::call_site());
                quote! { #ident }
            });

            all_enums.extend(quote! {
                /// Available extensions for RV64 ISA base
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Sequence)]
                #[allow(non_camel_case_types)]
                pub enum RV64Extensions {
                    #(#rv64_enum_variants),*
                }
            });
        }

        all_enums
    }

    /// Generate Random implementations for separated-mode extension enums
    fn generate_separated_extension_random_impls(
        &self,
        by_extension_and_isa: &HashMap<(ISAExtension, ISABase), Vec<&Instruction>>,
    ) -> TokenStream {
        let mut all_impls = TokenStream::new();

        // Collect extensions grouped by ISA base
        let mut rv32_extensions = HashSet::new();
        let mut rv64_extensions = HashSet::new();

        for ((extension, isa_base), _) in by_extension_and_isa {
            match isa_base {
                ISABase::RV32 => {
                    rv32_extensions.insert(*extension);
                }
                ISABase::RV64 => {
                    rv64_extensions.insert(*extension);
                }
            }
        }

        // Implement Random for RV32Extensions
        if !rv32_extensions.is_empty() {
            let mut rv32_variants = rv32_extensions.into_iter().collect::<Vec<_>>();
            rv32_variants.sort();

            let rv32_match_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let enum_name = Ident::new(&format!("RV32{}Instructions", ext), Span::call_site());

                quote! {
                    RV32Extensions::#ext_ident => {
                        Ok(RiscvInstruction::RV32(RV32Instruction::#ext_ident(
                            #enum_name::random_with_rng(rng, config)?
                        )))
                    }
                }
            });

            let rv32_sequence_match_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let enum_name = Ident::new(&format!("RV32{}Instructions", ext), Span::call_site());

                quote! {
                    RV32Extensions::#ext_ident => {
                        let seq = #enum_name::random_sequence_with_rng(rng, config)?;
                        let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                        Ok(InstructionSequence::with_full_instructions(
                            pre_instructions,
                            RiscvInstruction::RV32(RV32Instruction::#ext_ident(instruction)),
                            post_instructions,
                        ))
                    }
                }
            });

            let rv32_instruction_count_arms = rv32_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let key = (ext.clone(), ISABase::RV32);
                let count = by_extension_and_isa
                    .get(&key)
                    .map(|instructions| instructions.len())
                    .unwrap_or(0);

                quote! {
                    RV32Extensions::#ext_ident => #count
                }
            });

            all_impls.extend(quote! {
                impl RV32Extensions {
                    /// Generate a random instruction for this extension
                    pub fn random_instruction<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<RiscvInstruction, RandomGenerationError> {
                        match self {
                            #(#rv32_match_arms),*
                        }
                    }

                    /// Generate a random instruction sequence (including necessary pseudo instructions) for this extension
                    pub fn random_sequence_with_rng<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<InstructionSequence<RiscvInstruction>, RandomGenerationError> {
                        match self {
                            #(#rv32_sequence_match_arms),*
                        }
                    }

                    /// Return the total number of RV32 instructions exposed by this extension
                    pub const fn instruction_count(&self) -> usize {
                        match self {
                            #(#rv32_instruction_count_arms),*
                        }
                    }
                }
            });
        }

        // Implement Random for RV64Extensions
        if !rv64_extensions.is_empty() {
            let mut rv64_variants = rv64_extensions.into_iter().collect::<Vec<_>>();
            rv64_variants.sort();

            let rv64_match_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let enum_name = Ident::new(&format!("RV64{}Instructions", ext), Span::call_site());

                quote! {
                    RV64Extensions::#ext_ident => {
                        Ok(RiscvInstruction::RV64(RV64Instruction::#ext_ident(
                            #enum_name::random_with_rng(rng, config)?
                        )))
                    }
                }
            });

            let rv64_sequence_match_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let enum_name = Ident::new(&format!("RV64{}Instructions", ext), Span::call_site());

                quote! {
                    RV64Extensions::#ext_ident => {
                        let seq = #enum_name::random_sequence_with_rng(rng, config)?;
                        let InstructionSequence { pre_instructions, instruction, post_instructions } = seq;
                        Ok(InstructionSequence::with_full_instructions(
                            pre_instructions,
                            RiscvInstruction::RV64(RV64Instruction::#ext_ident(instruction)),
                            post_instructions,
                        ))
                    }
                }
            });

            let rv64_instruction_count_arms = rv64_variants.iter().map(|ext| {
                let ext_ident = Ident::new(&ext.to_string(), Span::call_site());
                let key = (ext.clone(), ISABase::RV64);
                let count = by_extension_and_isa
                    .get(&key)
                    .map(|instructions| instructions.len())
                    .unwrap_or(0);

                quote! {
                    RV64Extensions::#ext_ident => #count
                }
            });

            all_impls.extend(quote! {
                impl RV64Extensions {
                    /// Generate a random instruction for this extension
                    pub fn random_instruction<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<RiscvInstruction, RandomGenerationError> {
                        match self {
                            #(#rv64_match_arms),*
                        }
                    }

                    /// Generate a random instruction sequence (including necessary pseudo instructions) for this extension
                    pub fn random_sequence_with_rng<R: rand::Rng>(
                        &self,
                        rng: &mut R,
                        config: &RandomConfig,
                    ) -> Result<InstructionSequence<RiscvInstruction>, RandomGenerationError> {
                        match self {
                            #(#rv64_sequence_match_arms),*
                        }
                    }

                    /// Return the total number of RV64 instructions exposed by this extension
                    pub const fn instruction_count(&self) -> usize {
                        match self {
                            #(#rv64_instruction_count_arms),*
                        }
                    }
                }
            });
        }

        all_impls
    }
}
