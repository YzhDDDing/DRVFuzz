# riscv-instruction

`riscv-instruction` is the primary user-facing crate that exposes a complete set of RISC-V instructions generated from the processed definition JSON. It provides typed instruction enums, operand types, random generation utilities, and assembly formatting.

## Table of Contents
- Overview
- Installation
- Core Concepts
  - Instruction type hierarchy
  - Operand types
- Instruction Organization Modes
  - Merged model (`merged_instructions`)
  - Separated model (`separated_instructions`)
- Basic Usage
  - Creating instructions (merged and separated)
  - Inspecting instruction metadata
- Random Instruction Generation
- Configuration (register and memory constraints)
- Instruction Sequence Generation
- Assembler Compatibility Tests
- Example Programs

## Overview
The crate is generated via procedural macros from the JSON produced by `riscv-instruction-parser`. It offers:
- Type safety for instructions and operands.
- Two organization models (merged or separated by ISA base).
- Built‑in random instruction generation.
- Standard RISC-V assembly output.

## Installation
Add to `Cargo.toml`:
```toml
riscv-instruction = { path = "../riscv-instruction" }
```

## Core Concepts
### Instruction Type Hierarchy
```
RiscvInstruction
├── SharedInstruction / SpecificInstruction
│   ├── Extension enum (I, M, F, D, C, V, …)
│   │   └── InstructionVariant (e.g., ADD, SUB, …)
│   │       └── Operand struct (registers, immediates)
```

### Operand Types
- Registers: `IntegerRegister`, `FloatingPointRegister`, `VectorRegister`, saved-register variants.
- Immediates: `Immediate<N>`, `UImmediate<N>`, `SignedImmediate<N>`.
- Special: CSR address, round mode, fence mode, FLI constant, saved reg list with stack adjustment, etc.

## Instruction Organization Modes
### Merged (`merged_instructions`)
Shared instructions across RV32/RV64 plus architecture-specific variants under a single enum. Useful for cross-ISA tooling and compatibility-aware generation.

### Separated (`separated_instructions`)
Distinct enums for RV32 and RV64, each containing its full extension set. Useful for ISA-specific optimizations, simulators, or assemblers.

## Basic Usage
- Construct operands and instructions using either merged or separated models.
- Format to assembly with `Display`.
- Inspect metadata (extension, ISA base, operand count, memory access info).

## Random Instruction Generation
- Generate random instructions or sequences for a chosen ISA base or extension.
- Optional deterministic seeds for reproducible streams.

## Configuration
- `RegisterConfig`: constrain integer, floating, and vector register ranges.
- `MemConfig`: constrain base-register indices and immediate/offset ranges for memory instructions.

## Instruction Sequence Generation
Creates executable sequences with automatic setup for base registers and immediates so that generated instructions are valid.

## Assembler Compatibility Tests
Large random corpora are emitted to assembly and assembled with `riscv64-unknown-elf-as` using appropriate `-march` strings; failures are logged with the offending instructions.

## Example Programs
- `examples/merged_usage.rs` — basic merged-model usage
- `examples/separated_usage.rs` — basic separated-model usage
- `examples/random_merged_example.rs` — random generation (merged)
- `examples/random_separated_example.rs` — random generation (separated)
- `examples/random_sequence_example.rs` — sequence generation
- `examples/test_register_config.rs` — register constraints
- `examples/test_special_registers.rs` — special register types
