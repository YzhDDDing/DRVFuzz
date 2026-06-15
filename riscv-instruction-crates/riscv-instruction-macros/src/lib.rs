extern crate proc_macro;

mod code_gen;
mod instruction_display;
mod random;
mod validated_value;

use std::{env, fs, path::PathBuf};

use code_gen::CodeGenerator;
use instruction_display::impl_instruction_display;
use random::impl_random_derive;
use riscv_instruction_parser::types::Instruction;
use validated_value::impl_validated_value;

use syn::parse_macro_input;

/// Implement Display for enum variants using `#[asm("...")]` or `#[asm_code(...)]`
#[proc_macro_derive(DeriveInstructionDisplay, attributes(asm, asm_code))]
pub fn instruction_display_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as syn::DeriveInput);
    let output = impl_instruction_display(&ast);
    proc_macro::TokenStream::from(output)
}

/// Implement the Random trait for enums and structs
#[proc_macro_derive(DeriveRandom)]
pub fn random_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input_ast = parse_macro_input!(input as syn::DeriveInput);
    let output_tokens = impl_random_derive(&input_ast);
    proc_macro::TokenStream::from(output_tokens)
}

/// Implement ValidatedValue for newtype structs (requires `#[validated(...)]`)
#[proc_macro_derive(DeriveValidatedValue, attributes(validated))]
pub fn derive_validated_value(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as syn::DeriveInput);
    let output_stream = impl_validated_value(&ast);
    proc_macro::TokenStream::from(output_stream)
}

/// Generate merged RISC-V instruction enums from JSON (shared instructions merged across RV32/RV64)
#[proc_macro]
pub fn generate_merged_riscv_instructions(
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input_str = input.to_string();
    let json_path = input_str.trim_matches('"');

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let mut path = PathBuf::from(manifest_dir);
    path.push(json_path);

    let file_content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    let instructions: Vec<Instruction> =
        serde_json::from_str(&file_content).expect("Failed to parse JSON");

    let code_generator = CodeGenerator::new(instructions);

    let generated_code = code_generator.generate_instruction_enum();
    proc_macro::TokenStream::from(generated_code)
}

/// Generate separated RISC-V instruction enums from JSON (fully split by extension and ISA base)
#[proc_macro]
pub fn generate_separated_riscv_instructions(
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input_str = input.to_string();
    let json_path = input_str.trim_matches('"');

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let mut path = PathBuf::from(manifest_dir);
    path.push(json_path);

    let file_content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    let instructions: Vec<Instruction> =
        serde_json::from_str(&file_content).expect("Failed to parse JSON");

    let code_generator = CodeGenerator::new(instructions);

    let generated_code = code_generator.generate_separated_instruction_enum();

    proc_macro::TokenStream::from(generated_code)
}
