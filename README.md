# DRVFuzz: Data-Sensitive RISC-V CPU Fuzzing (USENIX Security'26) 

DRVFuzz is a RISC-V fuzzing and differential-testing toolkit. It generates
random or data-sensitive RISC-V programs, builds ELF artifacts, executes them
against multiple implementation wrappers, and localizes behavioral differences
with replayable reports. 

The current tool lives in `DRVFuzz/` and depends on the local
`riscv-instruction-crates/` crates.

## Repository Layout

- `DRVFuzz/` - Rust CLI crate and main implementation.
- `DRVFuzz/src/` - execution wrappers, testcase generation, diff analysis,
  SDModel data-sensitive generation, and transition-guided feedback.
- `DRVFuzz/configs/` - ready-to-run RV32/RV64 testcase, timeout, and run-option
  examples.
- `DRVFuzz/riscv_impls_bins/` - implementation wrapper binaries when present.
- `DRVFuzz/scripts/setup_riscv_env.sh` and `.fish` - helper scripts that export
  wrapper binary environment variables.
- `riscv-instruction-crates/` - local instruction description and encoding
  crates used by DRVFuzz.
- `release_assets/` - archive and checksum for prebuilt wrapper binaries.

## Requirements

- Linux environment for the bundled implementation wrappers.
- Rust stable with `cargo`.
- RISC-V GNU toolchain on `PATH`: `riscv64-unknown-elf-gcc`,
  `riscv64-unknown-elf-as`, `riscv64-unknown-elf-ld`, and
  `riscv64-unknown-elf-objdump`.
- Runtime libraries normally needed by the wrappers, such as `libc`,
  `libstdc++`, `libgcc_s`, `zlib`, `libzstd`, `libelf`, and `libsqlite3`.
- Bash/Zsh or Fish if you want to use the environment setup scripts.

## Wrapper Binaries

If `DRVFuzz/riscv_impls_bins/` is missing or incomplete, download
`riscv_impls_bins_binaries.tar.gz` from the GitHub Release assets into
`release_assets/`, then extract it from the repository root. The tarball is
intentionally ignored by Git because it is too large for normal repository
storage; the checksum file is kept in the repository.

```bash
sha256sum -c release_assets/riscv_impls_bins_binaries.tar.gz.sha256
tar -xzf release_assets/riscv_impls_bins_binaries.tar.gz -C DRVFuzz
chmod +x DRVFuzz/riscv_impls_bins/*
```

Load the wrapper paths before running tests:

```bash
cd DRVFuzz
source scripts/setup_riscv_env.sh
```

For Fish:

```fish
cd DRVFuzz
source scripts/setup_riscv_env.fish
```

The setup script exports paths for the bundled Spike, Rocket, BOOM v3/v4,
CVA6, PicoRV32, Kronos, and Srv32 binaries. The code also supports optional
XiangShan, Ibex, and Vex wrappers if their corresponding `RISCV_WRAPPER_*`
environment variables are set.

## Build

```bash
cd DRVFuzz
cargo build --release
```

The release binary is written to `DRVFuzz/target/release/DRVFuzz`. During
development, the examples below use `cargo run --release -- ...` from inside
`DRVFuzz/`.

## Quick Smoke Test

`exec` runs a bare instruction list on a single implementation. DRVFuzz wraps
the snippet with the implementation-specific prologue, trap handling, and exit
sequence.

```bash
cd DRVFuzz
mkdir -p temp
cat > temp/user_instructions.asm <<'EOF'
addi x1, x0, 1
addi x2, x0, 2
add  x3, x1, x2
EOF

cargo run --release -- exec \
  --asm temp/user_instructions.asm \
  --riscv-impl spike \
  --isa rv64 \
  --run-dir temp/exec \
  --timeout-secs 10 \
  --transition-report
```

Outputs are written under `temp/exec/Spike/`, including
`execution_output.json`, `execution_report.md`, `execution_timing.md`,
`user_instructions.asm`, and optional `transition_report.json/.md`.

## Differential Fuzzing

Run generated RV64 programs against Spike and Rocket:

```bash
cargo run --release -- diff \
  --testcase-config configs/rv64.toml \
  --riscv-impl spike,rocket \
  --run-dir temp/diff_spike_rocket \
  --impl-timeout configs/timeout.toml \
  --run-options configs/run_options.toml \
  --concurrency 2 \
  --max-runs 50
```

Each worker creates timestamped run directories under `--run-dir`. Failure
diagnostics include `diff_analysis_failure.md`; successful diff-analysis runs
can include iteration summaries, initialization reports, write-difference
localization reports, timing reports, and replayable `testcase.json` files
depending on `configs/run_options*.toml`.

To compare Spike against selected targets one pair at a time:

```bash
cargo run --release -- diff-spike \
  --testcase-config configs/rv64.toml \
  --riscv-impl rocket,boom-v3,boom-v4,cva6 \
  --run-dir temp/diff_spike \
  --impl-timeout configs/timeout.toml \
  --run-options configs/run_options_emit.toml \
  --concurrency 2 \
  --max-runs 20
```

`diff-spike` writes pair-specific directories such as
`temp/diff_spike/spike_rocket_rv64/`.

## SDModel and Guided Feedback

DRVFuzz includes an SDModel-based data-sensitive generator. Enable it for
boundary values, exception-triggering operands, and floating-point corner cases:

```bash
cargo run --release -- diff \
  --testcase-config configs/rv64.toml \
  --riscv-impl spike,rocket \
  --run-dir temp/diff_sdmodel \
  --impl-timeout configs/timeout.toml \
  --run-options configs/run_options.toml \
  --sdmodel \
  --sdmodel-probability 0.5 \
  --max-runs 50
```

Transition-guided and mode-guided feedback are available for `diff` and
`diff-spike`. They are mutually exclusive:

```bash
cargo run --release -- diff \
  --testcase-config configs/rv64.toml \
  --riscv-impl spike,rocket \
  --run-dir temp/diff_transition_guided \
  --impl-timeout configs/timeout.toml \
  --run-options configs/run_options.toml \
  --sdmodel \
  --transition-guided \
  --transition-seed-pool-limit 64 \
  --transition-seed-window 16 \
  --max-runs 50
```

Guided runs write `transition_report.json` and `transition_report.md` in the
diff run directory when transition analysis is enabled.

## Replay a Saved Testcase

Use `diff-testcase` to reproduce or re-check a saved `testcase.json`:

```bash
cargo run --release -- diff-testcase \
  --testcase temp/diff_spike_rocket/<run>/testcase.json \
  --run-dir temp/replay \
  --impl-timeout configs/timeout.toml \
  --run-options configs/run_options.toml
```

Add `--transition-guided` if you want replay to emit transition-analysis
artifacts using the same run-options guidance path.

## Generate Inputs Without Running Cores

`generate` creates a testcase and ELF artifacts for an implementation without
executing it:

```bash
cargo run --release -- generate \
  --config configs/rv64.toml \
  --riscv-impl spike \
  --run-dir temp/generated \
  --minimal-artifacts
```

Full generation emits `testcase.json`, `user_instructions.asm`, `program.S`,
assembled output, ELF, and disassembly artifacts. `--minimal-artifacts` keeps
only the program and build products.

## Useful Configs

- `configs/rv32.toml` and `configs/rv64.toml` - baseline random testcase
  generation.
- `configs/rv32_data_sensitive.toml` - data-sensitive generation example.
- `configs/rv32_unligned.toml` and `configs/rv64_unligned.toml` - unaligned
  access test configurations.
- `configs/rv64_spike_boom.toml` and `configs/rv64_rocket_boom.toml` - pair or
  implementation-focused RV64 examples.
- `configs/timeout.toml` - per-implementation timeout table.
- `configs/run_options.toml`, `configs/run_options_emit.toml`, and
  `configs/run_options_single.toml` - diff-analysis reporting, cleanup,
  iteration, and guided-feedback options.

Run `cargo run --release -- --help` or
`cargo run --release -- <subcommand> --help` for the authoritative CLI option
list and accepted implementation names.
