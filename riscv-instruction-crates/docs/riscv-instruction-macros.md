# riscv-instruction-macros

`riscv-instruction-macros` is the procedural macro crate that generates the instruction enums, trait impls, and supporting code for the RISC-V instruction library.

## Table of Contents
- Overview
- Core Macros
  - Instruction-generation macros
  - Derive macros
- Code Generation Architecture
- Config Integration

## Overview
The crate automates:
- Instruction enum generation from JSON definitions.
- Trait implementations (`Display`, `Random`, `RandomInstructionSequence`, `ValidatedValue`, etc.).
- Constraint handling for operands.
- Smart code generation for memory-aware immediates and register constraints.

## Core Macros
### Instruction Generation
- Produces merged-model enums (`SharedInstruction`, `SpecificInstruction`) or separated RV32/RV64 enums based on instruction sharing.
- Builds structs, enums, and implementations for every instruction variant.

### Derive Macros
- `DeriveInstruction` (used internally) to emit instruction enums/structs.
- `DeriveRandom` to implement `Random` and `RandomInstructionSequence`, respecting `RegisterConfig`/`MemConfig` constraints and memory-aware immediates.
- `DeriveValidatedValue` to implement range/multiple/forbidden/odd-only checks with customizable display/name.
- `DeriveInstructionDisplay` to implement `Display` for assembly output with `#[asm("format")]` or custom `#[asm_code(...)]` blocks.

## Code Generation Architecture
Key modules:
- `analysis.rs` — analyzes instruction sharing and generates restricted type definitions.
- `enums.rs` — emits instruction enums (shared and main).
- `structs.rs` — emits instruction structs for merged/separated models.
- `types.rs` — generates restricted register/immediate types.
- `random.rs` — emits `Random`/`RandomInstructionSequence` impls with config-aware generation and memory-access analysis.
- `validated_value.rs` — emits `ValidatedValue` impls with `forbidden`, `odd_only`, `skip_display`, and custom names/displays.
- `instruction_display.rs` — handles assembly formatting placeholders.

## Config Integration
- Register detection: type names containing `IntegerRegister`, `FloatingPointRegister`, or `VectorRegister` automatically apply the corresponding ranges from `RegisterConfig`.
- Memory-aware immediates apply `MemConfig` immediate ranges.
- Constraint priority: type-intrinsic (`ValidatedValue`) > config (`RegisterConfig`/`MemConfig`) > extra attributes (`multiple_of`, `odd_only`, `forbidden`).

## Random Instruction Sequences
`DeriveRandom` also implements `RandomInstructionSequence`:
- Detects memory-access patterns via regex on assembly strings.
- Generates necessary pseudo-instructions (e.g., load-immediate for base registers) using `MemConfig` address ranges.
- Returns `InstructionSequence<T>` to retain type safety.
