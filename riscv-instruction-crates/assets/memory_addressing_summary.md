# Memory Addressing Placeholder (English)

This file is normally generated automatically by `riscv-instruction-parser` and lists all instructions that use base+offset addressing, along with the placeholder names (`{xs1}`, `{rs1}`, `{imm}`, `{uimm}`, `{offset}`, `sp`) and whether the instruction actually performs a memory access.

The previous Chinese summary has been replaced with this English note to keep the repository language-unified.

To regenerate the full analysis in English:
1. Ensure `assets/riscv-unified-db` contains the upstream YAML.
2. Run `cargo run --package riscv-instruction-parser`.

The generated report will include:
- Standard integer load/store addressing forms
- Compressed load/store variants
- Vector load/store patterns
- Pseudo or non-memory instructions that still use base+offset syntax (e.g., `jalr`)
- Observations on operand naming consistency
