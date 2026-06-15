use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident};

/// Generate implementation for the Random trait
pub fn impl_random_derive(input_ast: &DeriveInput) -> TokenStream {
    let name: &Ident = &input_ast.ident;
    let generics = &input_ast.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let random_impl =
        generate_random_impl(input_ast, name, &impl_generics, &ty_generics, &where_clause);
    let random_sequence_impl =
        generate_random_sequence_impl(name, &impl_generics, &ty_generics, &where_clause);

    quote! {
        #random_impl
        #random_sequence_impl
    }
}

/// Generate the Random trait implementation
fn generate_random_impl(
    input_ast: &DeriveInput,
    name: &Ident,
    impl_generics: &syn::ImplGenerics,
    ty_generics: &syn::TypeGenerics,
    where_clause: &Option<&syn::WhereClause>,
) -> TokenStream {
    let random_trait_ident = quote!(::riscv_instruction_types::Random);
    let random_config_ident = quote!(::riscv_instruction_types::RandomConfig);
    let random_error_ident = quote!(::riscv_instruction_types::RandomGenerationError);

    match &input_ast.data {
        Data::Enum(data_enum) => {
            let variants_data = &data_enum.variants;

            if variants_data.is_empty() {
                return syn::Error::new_spanned(
                    name,
                    "Cannot derive Random for an enum with no variants",
                )
                .to_compile_error();
            }

            let num_variants = variants_data.len();
            let mut match_arms = TokenStream::new();

            for (idx, variant) in variants_data.iter().enumerate() {
                let variant_ident: &Ident = &variant.ident;
                let mut field_initializers = TokenStream::new();

                match &variant.fields {
                    Fields::Named(fields_named) => {
                        for field in fields_named.named.iter() {
                            let field_name_ident = field
                                .ident
                                .as_ref()
                                .expect("Named field must have an identifier");
                            let field_ty = &field.ty;
                            field_initializers.extend(quote! {
                                #field_name_ident: <#field_ty as #random_trait_ident>::random_with_rng(rng, config)?,
                            });
                        }
                        match_arms.extend(quote! {
                            #idx => Ok(#name::#variant_ident { #field_initializers }),
                        });
                    }
                    Fields::Unnamed(fields_unnamed) => {
                        let mut tuple_fields = TokenStream::new();
                        for field in fields_unnamed.unnamed.iter() {
                            let field_ty = &field.ty;
                            tuple_fields.extend(quote! {
                                <#field_ty as #random_trait_ident>::random_with_rng(rng, config)?,
                            });
                        }
                        match_arms.extend(quote! {
                            #idx => Ok(#name::#variant_ident(#tuple_fields)),
                        });
                    }
                    Fields::Unit => {
                        match_arms.extend(quote! {
                            #idx => Ok(#name::#variant_ident),
                        });
                    }
                }
            }

            quote! {
                impl #impl_generics #random_trait_ident for #name #ty_generics #where_clause {
                    type Output = Self;

                    fn random_with_rng<R: rand::Rng>(
                        rng: &mut R,
                        config: &#random_config_ident,
                    ) -> Result<Self::Output, #random_error_ident> {
                        let variant_idx = rng.gen_range(0..#num_variants);
                        match variant_idx {
                            #match_arms
                            _=> Err(#random_error_ident::new(
                                stringify!(#name),
                                "variant index out of bounds, this should never happen if num_variants is correct",
                            )),
                        }
                    }
                }
            }
        }
        Data::Struct(data_struct) => {
            match &data_struct.fields {
                Fields::Named(fields_named) => {
                    // Generate random values for each named field
                    let field_initializers = fields_named.named.iter().map(|field| {
                        let field_name = field.ident.as_ref().unwrap();
                        let field_ty = &field.ty;
                        quote! {
                            #field_name: <#field_ty as #random_trait_ident>::random_with_rng(rng, config)?
                        }
                    });

                    quote! {
                        impl #impl_generics #random_trait_ident for #name #ty_generics #where_clause {
                            type Output = Self;

                            fn random_with_rng<R: rand::Rng>(
                                rng: &mut R,
                                config: &#random_config_ident,
                            ) -> Result<Self::Output, #random_error_ident> {
                                Ok(Self {
                                    #(#field_initializers),*
                                })
                            }
                        }
                    }
                }
                Fields::Unnamed(fields_unnamed) => {
                    // For tuple structs, check whether this is a ValidatedValue newtype
                    if fields_unnamed.unnamed.len() == 1 {
                        // Possibly a ValidatedValue newtype; generate a tailored random impl
                        quote! {
                            impl #impl_generics #random_trait_ident for #name #ty_generics #where_clause {
                                type Output = Self;

                                fn random_with_rng<R: rand::Rng>(
                                    rng: &mut R,
                                    config: &#random_config_ident,
                                ) -> Result<Self::Output, #random_error_ident> {
                                    use rand::Rng;

                                    // Check for register types and apply RegisterConfig
                                    let (range_start, range_end) = {
                                        let type_name = stringify!(#name);

                                        // Compressed base registers: intersect x8–x15 with MemConfig.register_number_range
                                        if type_name.contains("CompressedBaseAddressRegister") {
                                            let (config_min, config_max) = config.mem_config.register_number_range;
                                            let actual_min = Self::MIN.max(config_min as _);
                                            let actual_max = Self::MAX.min(config_max as _);
                                            (actual_min, actual_max)
                                        }
                                        // BaseAddressRegister: use MemConfig.register_number_range
                                        else if type_name.contains("BaseAddressRegister") {
                                            let (config_min, config_max) = config.mem_config.register_number_range;
                                            let actual_min = Self::MIN.max(config_min as _);
                                            let actual_max = Self::MAX.min(config_max as _);
                                            (actual_min, actual_max)
                                        }
                                        // Integer registers (all variants)
                                        else if type_name.contains("IntegerRegister") {
                                            let (config_min, config_max) = config.register_config.integer_register_range;
                                            let actual_min = Self::MIN.max(config_min as _);
                                            let actual_max = Self::MAX.min(config_max as _);
                                            (actual_min, actual_max)
                                        }
                                        // Floating-point registers
                                        else if type_name.contains("FloatingPointRegister") {
                                            let (config_min, config_max) = config.register_config.floating_point_register_range;
                                            let actual_min = Self::MIN.max(config_min as _);
                                            let actual_max = Self::MAX.min(config_max as _);
                                            (actual_min, actual_max)
                                        }
                                        // Vector registers
                                        else if type_name.contains("VectorRegister") {
                                            let (config_min, config_max) = config.register_config.vector_register_range;
                                            let actual_min = Self::MIN.max(config_min as _);
                                            let actual_max = Self::MAX.min(config_max as _);
                                            (actual_min, actual_max)
                                        }
                                        // Memory-aware immediates
                                        else if Self::is_memory_aware() {
                                            // Use configured immediate ranges for memory-aware immediates
                                            let (min_imm, max_imm) = config.mem_config.immediate_ranges;

                                            // For unsigned immediates, clamp the minimum to zero
                                            let type_name = stringify!(#name);
                                            let actual_min = if type_name.contains("UImmediate") {
                                                // Unsigned: if min_imm < 0 use 0, otherwise intersect
                                                let config_min = if min_imm < 0 { 0 } else { min_imm as _ };
                                                Self::MIN.max(config_min)
                                            } else {
                                                // Signed: intersect directly
                                                Self::MIN.max(min_imm as _)
                                            };

                                            let actual_max = Self::MAX.min(max_imm as _);
                                            (actual_min, actual_max)
                                        } else {
                                            // Non-register, non-memory-aware types use their intrinsic range
                                            (Self::MIN, Self::MAX)
                                        }
                                    };

                                    if range_start > range_end {
                                        return Err(#random_error_ident::new(
                                            stringify!(#name),
                                            format!(
                                                "invalid combined range [{}, {}] computed from configuration",
                                                range_start, range_end
                                            ),
                                        ));
                                    }

                                    const MAX_ATTEMPTS: usize = 10000;
                                    let mut last_error: Option<String> = None;
                                    for _ in 0..MAX_ATTEMPTS {
                                        let random_value = if let Some(multiple) = Self::MULTIPLE_OF {
                                            let min_multiple = if range_start >= 0 {
                                                (range_start + multiple - 1) / multiple
                                            } else {
                                                range_start / multiple
                                            };
                                            let max_multiple = range_end / multiple;

                                            if min_multiple <= max_multiple {
                                                let multiple_factor = if min_multiple == max_multiple {
                                                    min_multiple
                                                } else {
                                                    rng.gen_range(min_multiple..=max_multiple)
                                                };
                                                multiple_factor * multiple
                                            } else {
                                                if range_start == range_end {
                                                    range_start
                                                } else {
                                                    rng.gen_range(range_start..=range_end)
                                                }
                                            }
                                        } else if Self::ODD_ONLY {
                                            // Generate an odd value and ensure bounds are odd
                                            let odd_start = if range_start % 2 == 0 {
                                                range_start + 1
                                            } else {
                                                range_start
                                            };

                                            // Ensure upper bound is odd
                                            let odd_end = if range_end % 2 == 0 {
                                                range_end - 1
                                            } else {
                                                range_end
                                            };

                                            if odd_start <= odd_end {
                                                // Count available odd values
                                                let odd_count = (odd_end - odd_start) / 2 + 1;
                                                let random_index = rng.gen_range(0..odd_count);
                                                odd_start + random_index * 2
                                            } else {
                                                // No valid odd values; fall back to base range
                                                if range_start == range_end {
                                                    range_start
                                                } else {
                                                    rng.gen_range(range_start..=range_end)
                                                }
                                            }
                                        } else {
                                            if range_start == range_end {
                                                range_start
                                            } else {
                                                rng.gen_range(range_start..=range_end)
                                            }
                                        };

                                        if Self::FORBIDDEN.contains(&random_value) {
                                            continue;
                                        }

                                        match Self::new(random_value) {
                                            Ok(instance) => return Ok(instance),
                                            Err(err) => {
                                                last_error = Some(err);
                                            }
                                        }
                                    }
                                    Err(#random_error_ident::exhausted(
                                        stringify!(#name),
                                        MAX_ATTEMPTS,
                                        last_error,
                                    ))
                                }
                            }
                        }
                    } else {
                        // For multi-field tuple structs, generate random values per field
                        let field_initializers = fields_unnamed.unnamed.iter().map(|field| {
                            let field_ty = &field.ty;
                            quote! {
                                <#field_ty as #random_trait_ident>::random_with_rng(rng, config)?
                            }
                        });

                        quote! {
                            impl #impl_generics #random_trait_ident for #name #ty_generics #where_clause {
                                type Output = Self;

                                fn random_with_rng<R: rand::Rng>(
                                    rng: &mut R,
                                    config: &#random_config_ident,
                                ) -> Result<Self::Output, #random_error_ident> {
                                    Ok(Self(#(#field_initializers),*))
                                }
                            }
                        }
                    }
                }
                Fields::Unit => {
                    // For unit structs, just return the struct
                    quote! {
                        impl #impl_generics #random_trait_ident for #name #ty_generics #where_clause {
                            type Output = Self;

                            fn random_with_rng<R: rand::Rng>(
                                _rng: &mut R,
                                _config: &#random_config_ident,
                            ) -> Result<Self::Output, #random_error_ident> {
                                Ok(Self)
                            }
                        }
                    }
                }
            }
        }
        Data::Union(_) => {
            syn::Error::new_spanned(name, "Random derive macro cannot be used on unions.")
                .to_compile_error()
        }
    }
}

/// Generate the RandomInstructionSequence trait implementation
fn generate_random_sequence_impl(
    name: &Ident,
    impl_generics: &syn::ImplGenerics,
    ty_generics: &syn::TypeGenerics,
    where_clause: &Option<&syn::WhereClause>,
) -> TokenStream {
    let random_sequence_trait_ident = quote!(::riscv_instruction_types::RandomInstructionSequence);
    let random_trait_ident = quote!(::riscv_instruction_types::Random);
    let random_config_ident = quote!(::riscv_instruction_types::RandomConfig);
    let random_error_ident = quote!(::riscv_instruction_types::RandomGenerationError);
    let writable_csr_ident = quote!(::riscv_instruction_types::WritableCsr);

    quote! {
        impl #impl_generics #random_sequence_trait_ident for #name #ty_generics #where_clause {
            type Output = Self;

            fn random_sequence_with_rng<R: rand::Rng>(
                rng: &mut R,
                config: &#random_config_ident
            ) -> Result<InstructionSequence<Self::Output>, #random_error_ident> {
                // Generate the main instruction
                // BaseAddressRegister types already honor MemConfig.register_number_range
                let main_instruction = <Self as #random_trait_ident>::random_with_rng(rng, config)?;

                let instruction_str = format!("{}", main_instruction);
                let instruction_trimmed = instruction_str.trim();

                // Generate any prerequisite pseudo-instructions
                let pre_instructions = {
                    use rand::Rng;
                    use regex::Regex;

                    // Detect memory-access patterns (register inside parentheses), e.g., "lw x1, 0(x2)"
                    let memory_access_pattern = Regex::new(r"\([^)]*\b(sp|x\d+)\b[^)]*\)").unwrap();
                    let csr_rs1_pattern = Regex::new(r"^csrr(?:w|s|c)\s+[^,]+,\s+([a-z0-9_]+),\s+(x\d+)\b").unwrap();
                    let mut pre_instructions = Vec::new();

                    // Only generate pseudo-ops when a memory-access pattern exists
                    if memory_access_pattern.is_match(instruction_trimmed) {
                        // Extract the register inside parentheses (sp or xN)
                        let register_in_parentheses_pattern = Regex::new(r"\([^)]*\b(sp|x\d+)\b[^)]*\)").unwrap();

                        for cap in register_in_parentheses_pattern.captures_iter(instruction_trimmed) {
                            if let Some(reg_match) = cap.get(1) {
                                let reg_name = reg_match.as_str();

                                // Pick a random base value from config
                                let (min_val, max_val) = config.mem_config.register_ranges;
                                let random_value = if min_val == max_val {
                                    min_val as i64
                                } else {
                                    rng.gen_range(min_val as i64..=max_val as i64)
                                };

                                // Parse the register number
                                let reg_num = if reg_name == "sp" {
                                    2 // sp is x2
                                } else {
                                    reg_name[1..].parse::<u8>().unwrap_or(0)
                                };

                                if reg_num != 0 {
                                    if let Ok(target_reg) = IntegerRegister::new(reg_num) {
                                        pre_instructions.push(PseudoInstruction::LoadImmediate {
                                            rd: target_reg,
                                            immediate: random_value,
                                            purpose: ::riscv_instruction_types::LoadImmediatePurpose::BaseAddress,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // CSR write ops need a source register; inject a random value
                    if let Some(csr_cap) = csr_rs1_pattern.captures(instruction_trimmed) {
                        let csr_name = csr_cap.get(1).map(|m| m.as_str());
                        let reg_name = csr_cap.get(2).map(|m| m.as_str());
                        if let (Some(csr_name), Some(reg_name)) = (csr_name, reg_name) {
                            if let Some(csr_variant) = #writable_csr_ident::from_name(csr_name) {
                                if let Ok(reg_num) = reg_name[1..].parse::<u8>() {
                                    if reg_num != 0 {
                                        if let Ok(target_reg) = IntegerRegister::new(reg_num) {
                                            let random_value = csr_variant.random_legal_value(rng) as i64;
                                            pre_instructions.push(PseudoInstruction::LoadImmediate {
                                                rd: target_reg,
                                                immediate: random_value,
                                                purpose: ::riscv_instruction_types::LoadImmediatePurpose::CsrValue,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Deduplicate pre-instructions targeting the same register
                    if !pre_instructions.is_empty() {
                        pre_instructions.sort_by_key(|instr| match instr {
                            PseudoInstruction::LoadImmediate { rd, .. } => rd.get(),
                            PseudoInstruction::LoadAddress { rd, .. } => rd.get(),
                            PseudoInstruction::LoadLabelAddress { rd, .. } => rd.get(),
                            PseudoInstruction::ReadFflags { rd } => rd.get(),
                            PseudoInstruction::MoveIntegerToFloat { source, .. } => source.get(),
                            PseudoInstruction::SetRoundingMode { .. } => u8::MAX,
                            PseudoInstruction::Comment(_) => u8::MAX,
                        });
                        pre_instructions.dedup_by_key(|instr| match instr {
                            PseudoInstruction::LoadImmediate { rd, .. } => rd.get(),
                            PseudoInstruction::LoadAddress { rd, .. } => rd.get(),
                            PseudoInstruction::LoadLabelAddress { rd, .. } => rd.get(),
                            PseudoInstruction::ReadFflags { rd } => rd.get(),
                            PseudoInstruction::MoveIntegerToFloat { source, .. } => source.get(),
                            PseudoInstruction::SetRoundingMode { .. } => u8::MAX,
                            PseudoInstruction::Comment(_) => u8::MAX,
                        });
                    }

                    pre_instructions
                };

                let post_instructions = {
                    use regex::Regex;

                    let mut post = Vec::new();
                    let fp_register_pattern =
                        Regex::new(r"\bf(?:[12][0-9]|3[01]|[0-9])\b").unwrap();

                    if config.capture_fflags && fp_register_pattern.is_match(&instruction_str) {
                        let target_reg = <::riscv_instruction_types::IntegerRegister as #random_trait_ident>::random_with_rng(rng, config)?;
                        post.push(PseudoInstruction::ReadFflags { rd: target_reg });
                    }

                    post
                };

                Ok(InstructionSequence::with_full_instructions(
                    pre_instructions,
                    main_instruction,
                    post_instructions,
                ))
            }
        }
    }
}
