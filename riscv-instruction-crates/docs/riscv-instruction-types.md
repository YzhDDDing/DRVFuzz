# riscv-instruction-types

`riscv-instruction-types` supplies the foundational types for the instruction crates: registers, immediates, configuration structs, validation traits, pseudo-instructions, and instruction sequences.

## Table of Contents
- Overview
- Core Type Hierarchy
- Register Types
- Immediate Types
- Configuration
- Special Types
- Trait System
- Pseudo-Instructions
- Instruction Sequences
- Data-Sensitive Rules

## Overview
The crate defines:
- Strongly typed registers (integer, floating-point, vector, saved/ compressed variants).
- Parameterized immediates with optional memory awareness.
- Configuration for constrained random generation.
- Validation traits for compile-time/ runtime checks.
- Pseudo-instruction support and instruction sequences.

## Core Type Hierarchy
```
Base types
├─ Registers
├─ Immediates (Immediate<BITS, MEM_AWARE>, UImmediate<BITS, MEM_AWARE>)
├─ Config types (RandomConfig, RegisterConfig, MemConfig, DataSensitiveConfig)
├─ Special types (CSRAddress, RoundingMode, FenceMode, FliConstant,
│                SavedRegListWithStackAdj<XLEN>)
└─ Pseudo-instructions and sequences (PseudoInstruction, InstructionSequence<T>)
```

## Register Types
- `IntegerRegister`, `FloatingPointRegister`, `VectorRegister`
- Saved registers (`SavedIntegerRegister`, compressed variants)
- Compressed registers (`CompressedIntegerRegister`, `CompressedFloatingPointRegister`)
- Saved register list with stack adjustment (`SavedRegListWithStackAdjRv32/64`)

## Immediate Types
- Signed: `Immediate<const BITS, const MEM_AWARE>`
- Unsigned: `UImmediate<const BITS, const MEM_AWARE>`
- Memory-aware immediates respect `MemConfig` during random generation.

## Configuration
- `RegisterConfig`: restrict register index ranges by class.
- `MemConfig`: limit base-register indices and immediate/offset ranges.
- `RandomConfig`: aggregates register/memory/CSR/data-sensitive settings and flags (e.g., capture fflags).

## Special Types
- `CSRAddress` — CSR address (0x000–0xFFF).
- `RoundingMode` — round-mode encoding.
- `FenceMode` — fence predicate/success fields.
- `FliConstant` — 32 hardware constants for the Zfa `fli` instruction.
- `SavedRegListWithStackAdj` — saved-register list plus stack adjustment, validated per XLEN.

## Trait System
- `Random` — constraint-aware random generation.
- `ValidatedValue` — construction and mutation with range/multiple/forbidden-value checks.
- `MemoryAware` — marks types that need memory constraints during generation.

## Pseudo-Instructions
`PseudoInstruction` expands convenient forms (e.g., `la`, `li`, `lla`) into real instructions; display formatting matches standard assembly.

## Instruction Sequences
`InstructionSequence<T>` holds zero or more pseudo-instructions followed by a main instruction. Helpers allow editing immediates and iterating over pseudo-ops.

## Data-Sensitive Rules
The `data_sensitive` module keeps “instruction → operand-class rules” for biasing floating-point values and round modes. Rules can be enumerated, queried by mnemonic, and extended via `CUSTOM_FLOAT_RULES`. Each `OperandClass` can provide sample bits or indicate `Preserve` to keep existing register values.
