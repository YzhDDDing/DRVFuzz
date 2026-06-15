use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Implement Display for enums; supports `#[asm("...")]` and `#[asm_code(...)]` for custom formatting
pub fn impl_instruction_display(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;

    match &ast.data {
        Data::Enum(data) => impl_enum_display(name, data),
        Data::Struct(data) => impl_struct_display(name, data, &ast.attrs),
        _ => {
            quote! { compile_error!("InstructionDisplay can only be derived for enums and structs"); }
        }
    }
}

/// Implement Display for enums
fn impl_enum_display(name: &syn::Ident, data: &syn::DataEnum) -> TokenStream {
    let variants = &data.variants;

    let match_arms = variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let fields = &variant.fields;

        let field_names: Vec<_> = match fields {
            Fields::Named(fields) => fields
                .named
                .iter()
                .map(|f| f.ident.as_ref().unwrap().clone())
                .collect(),
            Fields::Unit => vec![],
            Fields::Unnamed(fields) => (0..fields.unnamed.len())
                .map(|i| syn::Ident::new(&format!("field_{}", i), proc_macro2::Span::call_site()))
                .collect(),
        };

        let match_pattern = match fields {
            Fields::Named(_) => quote! { Self::#variant_ident { #(#field_names),* } },
            Fields::Unit => quote! { Self::#variant_ident },
            Fields::Unnamed(fields) => {
                if fields.unnamed.len() == 1 {
                    quote! { Self::#variant_ident(#(#field_names),*) }
                } else {
                    quote! { Self::#variant_ident(#(#field_names),*) }
                }
            }
        };

        // For enum variants containing a single-field struct, delegate to the inner Display
        let arm_body = match fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field_name = &field_names[0];
                quote! {
                    write!(f, "{}", #field_name)
                }
            }
            _ => {
                let default_str = variant.ident.to_string().to_lowercase().replace('_', ".");
                quote! {
                    write!(f, #default_str)
                }
            }
        };

        quote! {
            #match_pattern => {
                #arm_body
            }
        }
    });

    quote! {
        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #(#match_arms),*
                }
            }
        }
    }
}

/// Implement Display for structs
fn impl_struct_display(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[syn::Attribute],
) -> TokenStream {
    let asm_attr = attrs.iter().find(|attr| attr.path().is_ident("asm"));

    let asm_code_attr = attrs.iter().find(|attr| attr.path().is_ident("asm_code"));

    let fields = &data.fields;

    // Collect field names for destructuring
    let field_names: Vec<_> = match fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .map(|f| f.ident.as_ref().unwrap().clone())
            .collect(),
        Fields::Unit => vec![],
        Fields::Unnamed(fields) => (0..fields.unnamed.len())
            .map(|i| syn::Ident::new(&format!("field_{}", i), proc_macro2::Span::call_site()))
            .collect(),
    };

    let display_body = if let Some(attr) = asm_code_attr {
        match &attr.meta {
            syn::Meta::List(meta_list) => {
                let code_string = meta_list.tokens.to_string();
                let code_content = if code_string.starts_with('"') && code_string.ends_with('"') {
                    &code_string[1..code_string.len() - 1]
                } else {
                    &code_string
                };

                let processed_code = code_content
                    .replace("\\n", "\n")
                    .replace("\\{", "{")
                    .replace("\\}", "}")
                    .replace("\\\"", "\"");

                match processed_code.parse::<TokenStream>() {
                    Ok(code_tokens) => match fields {
                        Fields::Named(_) => {
                            quote! {
                                let Self { #(#field_names),* } = self;
                                write!(f, "{}", #code_tokens)
                            }
                        }
                        Fields::Unnamed(_) => {
                            quote! {
                                let Self(#(#field_names),*) = self;
                                write!(f, "{}", #code_tokens)
                            }
                        }
                        Fields::Unit => {
                            quote! {
                                write!(f, "{}", #code_tokens)
                            }
                        }
                    },
                    Err(_) => {
                        quote! {
                            write!(f, #processed_code)
                        }
                    }
                }
            }
            _ => {
                quote! {
                    compile_error!("malformed `asm_code` attribute. Expected `#[asm_code(...)]`")
                }
            }
        }
    } else if let Some(attr) = asm_attr {
        match attr.parse_args::<syn::LitStr>() {
            Ok(lit_str) => {
                let format_str = lit_str.value();

                if format_str.contains('{') {
                    match fields {
                        Fields::Named(_) => {
                            quote! {
                                let Self { #(#field_names),* } = self;
                                let result = format!(#format_str);
                                write!(f, "{}", result)
                            }
                        }
                        Fields::Unnamed(_) => {
                            quote! {
                                let Self(#(#field_names),*) = self;
                                let result = format!(#format_str);
                                write!(f, "{}", result)
                            }
                        }
                        Fields::Unit => {
                            quote! {
                                let result = format!(#format_str);
                                write!(f, "{}", result)
                            }
                        }
                    }
                } else {
                    quote! {
                        write!(f, #format_str)
                    }
                }
            }
            Err(_) => {
                quote! {
                    compile_error!("malformed `asm` attribute. Expected `#[asm(\"...\")]`")
                }
            }
        }
    } else {
        // Default formatting: lowercased name with dots
        let default_str = name.to_string().to_lowercase().replace('_', ".");
        quote! {
            write!(f, #default_str)
        }
    };

    quote! {
        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                #display_body
            }
        }
    }
}
