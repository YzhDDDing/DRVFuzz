use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, LitBool, LitStr, Meta, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned as _,
};

#[derive(Default)]
pub struct ValidatedArgs {
    min: Option<LitStr>,
    max: Option<LitStr>,
    name: Option<LitStr>,
    display: Option<LitStr>,
    multiple_of: Option<LitStr>,
    skip_display: Option<LitBool>,
    forbidden: Option<LitStr>,
    odd_only: Option<LitBool>,
}

impl Parse for ValidatedArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = Self::default();
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;

        for meta in metas {
            match meta {
                Meta::NameValue(nv) => {
                    if nv.path.is_ident("min") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.min = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("max") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.max = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("name") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.name = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("display") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.display = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("multiple_of") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.multiple_of = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("forbidden") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.forbidden = Some(lit_str.clone());
                            }
                        }
                    } else if nv.path.is_ident("odd_only") {
                        if let syn::Expr::Lit(expr_lit) = &nv.value {
                            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                                args.odd_only =
                                    Some(LitBool::new(lit_str.value() == "true", lit_str.span()));
                            }
                        }
                    }
                }
                Meta::Path(path) => {
                    if path.is_ident("skip_display") {
                        args.skip_display = Some(LitBool::new(true, path.span()));
                    } else if path.is_ident("odd_only") {
                        args.odd_only = Some(LitBool::new(true, path.span()));
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        meta,
                        "Unsupported attribute format",
                    ));
                }
            }
        }

        Ok(args)
    }
}

/// Generate code for the ValidatedValue trait
pub fn impl_validated_value(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();

    let attr = ast
        .attrs
        .iter()
        .find(|a| a.path().is_ident("validated"))
        .expect("`validated` attribute is required for deriving `ValidatedValue`");
    let args: ValidatedArgs = attr
        .parse_args()
        .expect("Failed to parse `validated` attribute arguments");

    let inner_type = if let Data::Struct(s) = &ast.data {
        if let syn::Fields::Unnamed(f) = &s.fields {
            if f.unnamed.len() == 1 {
                f.unnamed.first().unwrap().ty.clone()
            } else {
                panic!("DeriveValidatedValue only supports newtype structs with one field");
            }
        } else {
            panic!("DeriveValidatedValue only supports newtype structs");
        }
    } else {
        panic!("DeriveValidatedValue can only be used on structs");
    };

    let min_expr: TokenStream = args
        .min
        .as_ref()
        .expect("`min` is required")
        .value()
        .parse()
        .unwrap();
    let max_expr: TokenStream = args
        .max
        .as_ref()
        .expect("`max` is required")
        .value()
        .parse()
        .unwrap();

    let name_in_msg = args
        .name
        .as_ref()
        .map(|n| n.value())
        .unwrap_or_else(|| name.to_string().to_snake_case().replace('_', " "));
    let error_msg = format!("{} {{}} exceeds range ({{}} to {{}})", name_in_msg);

    let multiple_check = if let Some(multiple_str) = &args.multiple_of {
        let multiple_expr: TokenStream = multiple_str.value().parse().unwrap();
        let err_msg = format!(
            "{} must be a multiple of {}",
            name_in_msg,
            multiple_str.value()
        );
        quote! {
            let multiple = #multiple_expr;
            if value % multiple != 0 {
                return Err(#err_msg.to_string());
            }
        }
    } else {
        quote! {}
    };

    let odd_only_check = if args.odd_only.as_ref().map_or(false, |b| b.value) {
        let err_msg = format!("{} must be odd", name_in_msg);
        quote! {
            if value % 2 == 0 {
                return Err(#err_msg.to_string());
            }
        }
    } else {
        quote! {}
    };

    let (forbidden_values, forbidden_check) = if let Some(forbidden_str) = &args.forbidden {
        let forbidden_str_value = forbidden_str.value();

        // Parse forbidden values according to the inner type
        let forbidden_values: Vec<TokenStream> = if forbidden_str_value.is_empty() {
            Vec::new()
        } else {
            forbidden_str_value
                .split(',')
                .filter_map(|s| {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        // Use the literal directly and let the compiler infer the type
                        let value_token: TokenStream = trimmed.parse().unwrap();
                        Some(value_token)
                    }
                })
                .collect()
        };

        let err_msg = format!(
            "{} value {{}} is forbidden for this instruction",
            name_in_msg
        );
        let forbidden_check = if !forbidden_values.is_empty() {
            quote! {
                if Self::FORBIDDEN.contains(&value) {
                    return Err(format!(#err_msg, value));
                }
            }
        } else {
            quote! {}
        };

        (forbidden_values, forbidden_check)
    } else {
        (Vec::new(), quote! {})
    };

    let skip_display = args.skip_display.as_ref().map_or(false, |b| b.value);

    let display_impl = if !skip_display {
        let display_format = args
            .display
            .as_ref()
            .map(|d| d.value())
            .unwrap_or("{}".to_string());

        quote! {
            impl #impl_generics std::fmt::Display for #name #ty_generics #where_clause {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, #display_format, self.0)
                }
            }
        }
    } else {
        quote! {}
    };

    let multiple_of_value = if let Some(multiple_str) = &args.multiple_of {
        let multiple_expr: TokenStream = multiple_str.value().parse().unwrap();
        quote! { Some(#multiple_expr) }
    } else {
        quote! { None }
    };

    let odd_only_value = if args.odd_only.as_ref().map_or(false, |b| b.value) {
        quote! { true }
    } else {
        quote! { false }
    };

    quote! {
        impl #impl_generics ValidatedValue<#inner_type> for #name #ty_generics #where_clause {
            const MIN: #inner_type = #min_expr;
            const MAX: #inner_type = #max_expr;
            const MULTIPLE_OF: Option<#inner_type> = #multiple_of_value;
            const FORBIDDEN: &'static [#inner_type] = &[#(#forbidden_values),*];
            const ODD_ONLY: bool = #odd_only_value;
            type Error = String;

            fn new(value: #inner_type) -> Result<Self, Self::Error> {
                #forbidden_check
                #multiple_check
                #odd_only_check
                if value >= Self::MIN && value <= Self::MAX {
                    Ok(Self(value))
                } else {
                    Err(format!(
                        #error_msg,
                        value,
                        Self::MIN,
                        Self::MAX
                    ))
                }
            }

            fn get(&self) -> #inner_type {
                self.0
            }

            fn set(&mut self, value: #inner_type) -> Result<(), Self::Error> {
                *self = Self::new(value)?;
                Ok(())
            }
        }

        #display_impl
    }
}
