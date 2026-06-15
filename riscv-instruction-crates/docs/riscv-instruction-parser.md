# riscv-instruction-parser

`riscv-instruction-parser` is a Rust library that reads instruction definitions from the RISC-V Unified Database YAML files, normalizes them, and produces structured JSON plus human-friendly reports. It also extracts operand information, analyzes memory-access patterns, and offers detailed per-extension summaries.

## Table of Contents
- Overview
- Core Features
  - YAML parser (`parser.rs`)
  - Instruction fixer (`instruction_fixer.rs`)
  - Operand extractor (`operand_extractor.rs`)
  - Memory access analysis
  - Report generator (`report_generator.rs`)
- Module Layout
- Processing Workflow
- Data Structures
  - Instruction, Operand, OperandType, OperandRestriction, MemoryAddressInfo, AssemblySyntax
  - ISAExtension, ISABase
- Key Components
  - Parser
  - Fixer
  - Operand extractor
  - Report generator
- Instruction Fixer Highlights
- Report Generator Highlights

## Overview
The crate parses upstream YAML definitions, fixes known inconsistencies, infers operand metadata, and outputs both JSON and Markdown reports for downstream tooling.

## Core Features
### 1. YAML Parser (`parser.rs`)
- Scans `arch/inst` in the RISC-V Unified Database.
- Parses each YAML file into `Instruction` structs (name, long name, description, operands, encoding, ISA bases).

### 2. Instruction Fixer (`instruction_fixer.rs`)
- Reconciles operand names between YAML and assembly syntax.
- Normalizes assembly formats, adds missing operands, and patches special cases.
- Infers operand types and constraints.

### 3. Operand Extractor (`operand_extractor.rs`)
- Extracts operands from assembly strings using brace parsing, regexes, and fallbacks.
- Understands formats such as `{rd}, {rs1}, {imm}`, `imm(rs1)`, and indexed operands like `rs1+1`.

### 4. Memory Access Analysis
- Detects memory-access instructions and records base register and offset operands.

### 5. Report Generator (`report_generator.rs`)
- Builds per-extension tables, ISA compatibility stats, operand usage summaries, and length/constraint distributions.

## Module Layout
```
├── lib.rs                 # crate entry; memory-access analysis test
├── main.rs                # executable entry point
├── types.rs               # core data structures
├── parser.rs              # YAML parser
├── instruction_fixer.rs   # instruction normalization
├── operand_extractor.rs   # operand extraction helpers
└── report_generator.rs    # Markdown report writer
```

## Processing Workflow
1. Scan extension folders under `arch/inst`.
2. Parse YAML files into `Instruction` structs.
3. Extract operands and encoding info.
4. Infer operand types and constraints.
5. Fix known data issues and normalize assembly syntax.
6. Serialize results to JSON.
7. Generate analysis reports.

## Data Structures
### Key Types
- `Instruction`: name, `ISAExtension`, `Vec<ISABase>`, operands, `AssemblySyntax`, optional `MemoryAddressInfo`.
- `Operand`: name, optional `OperandType`, bit lengths per ISA base, optional `OperandRestriction`.
- `OperandType`: integer register, saved integer register, floating register, vector register, signed/unsigned immediates, CSR address, round mode, fence mode, FLI constant, saved reg list with stack adjustment, not-equal compressed saved register pair.
- `OperandRestriction`: multiple-of, min/max range, forbidden values, odd-only flag.
- `MemoryAddressInfo`: optional base operand, optional fixed base (e.g., `sp`), optional offset operand.
- `AssemblySyntax`: `Format(String)` or `RustCode(String)`.

### ISA Types
- `ISAExtension`: all standard RISC-V extensions (I, M, F, D, Q, C, V, B, Z* series, etc.).
- `ISABase`: `RV32`, `RV64`.

## Key Components
### Parser
Reads YAML definitions, collects operands/encodings, and determines supported ISA bases.

### Fixer
Patches operand names, assembly syntax, missing operands, and special-case rules.

### Operand Extractor
Uses brace parsing, regexes, and heuristics to find operand identifiers in assembly strings.

### Report Generator
Produces Markdown covering per-extension counts, ISA compatibility, operand usage, length distributions, and constraint statistics.

## Instruction Fixer Highlights
- Maps YAML operand names to assembly syntax.
- Normalizes special instructions (e.g., compressed stack ops, aq/rl atomics).
- Repairs operand types and ranges (uimm vs imm, round-mode removal, etc.).

## Report Generator Highlights
- Extension overview tables (standard vs. compressed counts).
- ISA base compatibility breakdown.
- Operand usage and constraint summaries.
- Operand length distributions for RV32 and RV64.
