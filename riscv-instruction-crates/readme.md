# RISC-V Instruction Crates

A strongly typed toolkit for generating, validating, and rendering RISC-V instructions. The workspace provides:
- `riscv-instruction`: user-facing library exposing merged and separated instruction enums plus helpers for random generation and sequence building.
- `riscv-instruction-types`: core types for registers, immediates, constraints, memory access metadata, and random/validation traits.
- `riscv-instruction-macros`: procedural macros that generate instruction enums, display impls, random generators, and validators from JSON.
- `riscv-instruction-parser`: tools that parse the upstream `riscv-unified-db` YAML definitions, normalize/patch them, emit JSON, and generate Markdown reports.

## Highlights
- **Type safety**: Dedicated types for integer, floating, vector registers; validated immediates with range/multiple/forbidden constraints.
- **Wide ISA coverage**: RV32 and RV64, standard extensions (I, M, F, D, Q, C, B, V, H, S) plus many Z* and S* variants. The active instruction set is driven entirely by `assets/riscv_instructions_new.json`.
- **Random generation**: Constraint-aware random instruction and operand generation with reproducible seeds, register/memory configuration, and full sequence builders.
- **Assembly output**: All instruction types implement `Display` for assembler-ready text; tested against `riscv64-unknown-elf-as`.

## Supported ISA (summary)
Most standardized and common draft extensions are implemented. GNU assembler compatibility is validated for the majority; a few extensions such as `Zalasr`, `Zilsd`, and `Smrnmi` may be unsupported by current toolchains even though the instructions are present in the library.

## Quick Start

Add to your `Cargo.toml`:

```toml
riscv-instruction = { path = "riscv-instruction" }
```

Basic usage patterns:
- **Merged view**: `riscv_instruction::merged_instructions` exposes shared instructions plus architecture-specific variants under a single enum.
- **Separated view**: `riscv_instruction::separated_instructions` exposes independent RV32 and RV64 enums.
- **Random sequences**: Use `RandomInstructionSequence` with `RandomConfig`, `RegisterConfig`, and `MemConfig` to generate constrained sequences.

See the examples in `riscv-instruction/examples` for end-to-end code (merged/separated usage, random sequences, register constraints, memory configuration).

## Regenerating assets
`assets/riscv_instructions_new.json`, `assets/riscv_detailed_extension_report.md`, and `assets/memory_addressing_summary.md` are produced by the parser tool.

1. Clone the upstream database (project root):
   ```bash
   git clone --depth 1 https://github.com/riscv-software-src/riscv-unified-db assets/riscv-unified-db
   ```
2. Run the generator:
   ```bash
   cargo run --package riscv-instruction-parser
   ```

## Project Structure
```
├── riscv-instruction/          # main library and tests
├── riscv-instruction-types/    # core typed operands, constraints, configs
├── riscv-instruction-macros/   # proc-macros for codegen, display, random, validation
└── riscv-instruction-parser/   # YAML parser → JSON/Markdown assets
```

## Examples
- `merged_usage.rs` – merged instruction view basics  
- `separated_usage.rs` – RV32/RV64 separated view  
- `random_merged_example.rs`, `random_separated_example.rs` – random generation  
- `random_sequence_example.rs` – sequences with memory/register setup  
- `test_register_config.rs`, `test_restricted_registers.rs`, `test_special_registers.rs` – constraint-focused demos

Run an example (from workspace root):
```bash
cargo run --package riscv-instruction --example merged_usage
```

## Testing
- Unit tests live across the crates.
- Assembler compatibility tests require `riscv64-unknown-elf-as` on your `PATH`. Run from `riscv-instruction`:
  ```bash
  cargo test -- --ignored
  ```
  (Long-running; generates random instruction batches, assembles, and reports failures.)

## Documentation
Detailed crate docs live in `docs/`:
- `docs/riscv-instruction.md`
- `docs/riscv-instruction-types.md`
- `docs/riscv-instruction-macros.md`
- `docs/riscv-instruction-parser.md`
