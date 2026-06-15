## Project Overview

This toolkit generates, executes, and compares RISC-V implementations, supporting fast fuzzing, differential testing, and ELF builds. Built-in wrappers include Spike, Rocket, Boom, PicoRV32, CVA6, XiangShan, Ibex, Vex, Kronos, and Srv32.

All CLI path arguments (for example `--asm`, `--testcase-config`, `--testcase`, `--run-dir`, `--impl-timeout`, `--run-options`, `--config`) are converted to absolute paths automatically so relative inputs stay valid even if the working directory later changes.

## Dependencies

- Rust stable (with `cargo`).  
- RISC-V cross toolchain: `riscv64-unknown-elf-{gcc,as,ld,objdump}` must be on `PATH`.  
- Runtime libraries: glibc runtime and loader (`libc`, `libm`, `libgcc_s`, `libstdc++`, `ld-linux`), compression/ELF libraries (`zlib`, `libzstd`, `libelf`), SQLite (`libsqlite3`).  
  - Ubuntu/Debian:  
    ```bash
    sudo apt-get install --no-install-recommends --no-upgrade --no-remove \
      libc6 libgcc-s1 libstdc++6 zlib1g libzstd1 libelf1t64 libsqlite3-0
    ```
  - Fedora/RHEL:  
    ```bash
    sudo dnf install glibc libgcc libstdc++ zlib libzstd elfutils-libelf sqlite-libs
    ```
- RISC-V implementation binaries: download with the scripts below or provide your own.  
- Shell: Bash/Zsh or Fish (for the helper scripts).

### Download prebuilt RISC-V implementation binaries

The repository ships one-click scripts that download commonly used prebuilt binaries from a fixed GitHub release into `riscv_impls_bins`. Files whose hashes already match are skipped.

- POSIX shells (Bash/Zsh): `scripts/download_bins_release.sh`
- Fish: `scripts/download_bins_release.fish`

### Configure environment variables

Use scripts to load binary paths into the current shell:

- POSIX shells (Bash/Zsh): `source scripts/setup_riscv_env.sh`
- Fish: `source scripts/setup_riscv_env.fish`

The scripts export the following paths for the binaries shipped in this repo (the system `spike` can still be used):
- `RISCV_WRAPPER_CVA6_RV32_BIN` / `RISCV_WRAPPER_CVA6_RV64_BIN`
- `RISCV_WRAPPER_PICORV32_BIN`
- `RISCV_WRAPPER_ROCKET_RV32_BIN` / `RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN` / `RISCV_WRAPPER_ROCKET_RV64_BIN`
- `RISCV_WRAPPER_BOOM_BIN` / `RISCV_WRAPPER_BOOM_V4_BIN`
- `RISCV_WRAPPER_SPIKE_BIN`
- `RISCV_WRAPPER_XS_EMU` / `RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW` / `RISCV_WRAPPER_XS_DIFF_SO`
- `RISCV_WRAPPER_OPENC910_RV64_BIN` / `RISCV_WRAPPER_OPENC910_RV64_SREC2VMEM`
- `RISCV_WRAPPER_IBEX_RV32_BIN`
- `RISCV_WRAPPER_VEX_RV32_BIN`
- `RISCV_WRAPPER_KRONOS_RV32_BIN`

The script prints the current configuration and highlights missing binaries. If you want custom locations, override the variables after sourcing (the Fish version also writes user-level global variables).

## Command Guide

All commands run via `cargo run --release -- <subcommand> ...`. Path arguments may be relative; they are converted to absolute paths automatically. Below are the main subcommands with key options and examples.

- `exec`: compile and run a user assembly snippet—best for single checks or minimal repros.  
  - Key options: `--asm` (input assembly), `--riscv-impl` (spike/rocket/pico-rv32/cva6/xiang-shan/ibex/vex/kronos), `--isa`, `--run-dir`, `--timeout-secs`, `--allow-unaligned`.  
  - Outputs: `--run-dir/<impl>/execution_output.json`, `execution_report.md`, `user_instructions.asm`, `execution_timing.md`.  
  - Example:  
    ```bash
    cargo run --release -- exec \
      --asm temp/user_instructions.asm \
      --riscv-impl spike \
      --isa rv64 \
      --run-dir temp/exec \
      --timeout-secs 10 \
      --allow-unaligned
    ```

- `diff`: differential fuzzing based on randomly generated testcases, supporting parallel multi-implementation runs.  
  - Key options: `--testcase-config` (TOML generator config), `--riscv-impl` list (comma-separated), `--impl-timeout`, `--run-options`, `--run-dir`, `--concurrency`, `--max-runs`.  
  - Behavior: each worker creates a timestamped subdirectory under `run-dir`, storing iteration artifacts and failure reports (`diff_analysis_failure.md`).  
  - Example:  
    ```bash
    cargo run --release -- diff \
      --testcase-config configs/rv64.toml \
      --riscv-impl spike,rocket \
      --run-dir temp/diff \
      --concurrency 2 \
      --impl-timeout configs/timeout.toml \
      --run-options configs/run_options.toml \
      --max-runs 50
    ```

- `diff-testcase`: replay an existing `testcase.json` to validate/reproduce differential results or to tweak timeouts/options.  
  - Key options: `--testcase`, `--run-dir`, `--impl-timeout`, `--run-options`.  
  - Example:  
    ```bash
    cargo run --release -- diff-testcase \
      --testcase artifacts/example/testcase.json \
      --run-dir temp/replay \
      --impl-timeout configs/timeout.toml \
      --run-options configs/run_options.toml
    ```

- `diff-spike`: pair Spike with selected or all other implementations for differential runs, automatically skipping ISA-incompatible targets or those lacking required alignment support.  
  - Key options: `--testcase-config`, `--run-dir`, `--concurrency`, `--max-runs`, `--riscv-impl` (optional target list), `--impl-timeout`, `--run-options`.  
  - Output layout: each pairing lives under `run-dir/spike_<impl>_<isa>/`.  
  - Example:  
    ```bash
    cargo run --release -- diff-spike \
      --testcase-config configs/rv64.toml \
      --run-dir temp/spike \
      --concurrency 2 \
      --riscv-impl rocket,cva6 \
      --impl-timeout configs/timeout.toml \
      --run-options configs/run_options_emit.toml \
      --max-runs 20
    ```

- `generate`: generate random testcases and ELF artifacts only; execution is not performed. Useful for pre-generating inputs or using an external runner.  
  - Key options: `--config` (random generation config), `--riscv-impl`, `--run-dir`; optional `--isa` override and `--minimal-artifacts` (emit only `.S/.s/.elf`).  
  - Default outputs: `testcase.json`, `user_instructions.asm`, `program.S/.s`, `.elf`, `.dump`.  
  - Example:  
    ```bash
    # Generate an rv64 suite and emit minimal artifacts
    cargo run --release -- generate \
      --config configs/rv64.toml \
      --riscv-impl spike \
      --run-dir temp/generate \
      --minimal-artifacts

    # Override ISA to rv32 and emit full artifacts
    cargo run --release -- generate \
      --config configs/rv64.toml \
      --riscv-impl spike \
      --isa rv32 \
      --run-dir temp/generate_rv32
    ```

## Markdown Reports

- `exec` single run (`run-dir/<impl>/`): `execution_report.md` includes “Basic Info” (implementation/ISA/instructions/exceptions/user memory range), “Exception Summary” (per-instruction context + exception table with type counts), “Execution Trace Summary” (per-instruction register/memory changes or exceptions), and “Statistics Summary” (write/exception/normal ratios). `execution_timing.md` lists implementation runtimes.
- `diff` / `diff-testcase` / `diff-spike` iteration and replay directories (`iter_xxx`, `write_replay_xxx`, `write_history_min_xxx`, etc.):
  - Each multi-implementation run produces `execution_timings.md` (per-implementation runtimes); if `run_options.emit_execution_report_md = true`, each `<impl>/execution_report.md` mirrors the `exec` structure.
  - `report_00_iter_summary.md`: counts of test instructions plus per-implementation exception counts and instructions with writes.
  - Initialization reports: `report_{nn}_initial.md` records the number of initialization instructions and confirms register/memory writes match; `report_{nn}_initial_failure.md` lists the failing instruction index, reason, and per-implementation instructions with register/memory writes.
  - Exception-related: `report_{nn}_exception_removal.md` summarizes trimming driven by exception differences (original/removed/remaining counts) and lists instructions with register/memory context and per-implementation exceptions; `report_{nn}_exception_cause_diff.md` uses the same tables for differing exception causes.
  - Write-difference localization: `report_{nn}_write_removal.md` records instruction counts before/after trimming, difference classifications/tags/reasons, and per-implementation instruction/context/register/memory writes. Supporting reports include `write_replay_***/report_01_write_replay.md` (success/failure details with context), `write_history_min_***/report_00_history_min_error.md` (reasons a history minimization failed plus write columns), `write_history_min_***/candidate_***/report_01_history_candidate.md` (different starting candidates with differences/exceptions and context tables or a no-repro note), and `write_history_min_***/report_history_minimization.md` (best starting point, history length, and target difference table or warning).
  - Summary/failure: `final_report.md` records initial/final test instruction counts and the number of exception/write-difference removal rounds; any worker failure produces `diff_analysis_failure.md` with worker ID, error summary, and debug prints.

## Configuration Examples

- `configs/rv32.toml` / `configs/rv64.toml`: baseline random instruction generation configs for `--testcase-config` (`diff` / `diff-spike`) or `--config` (`generate`).  
- `configs/rv32_unligned.toml` / `configs/rv64_unligned.toml`: random configs that require unaligned memory access; used the same way.  
- `configs/timeout.toml`: per-implementation timeout table for `--impl-timeout` (`diff` / `diff-testcase` / `diff-spike`).  
- `configs/run_options.toml` / `configs/run_options_emit.toml`: diff run options (iteration count, whether to emit reports, whether to clean successful artifacts, etc.) for `--run-options` (`diff` / `diff-testcase` / `diff-spike`).

## Publishing/Syncing RISC-V implementation binaries

- `scripts/upload_bins_release.sh` / `scripts/upload_bins_release.fish`: upload binaries in `riscv_impls_bins` to a fixed GitHub release.  
  - Uploads everything by default; pass file names (relative to `riscv_impls_bins`) to update only selected files.  
  - Skips files whose hashes match the release and refreshes `riscv_impls_bins_manifest.json`.
- `scripts/download_bins_release.sh` / `scripts/download_bins_release.fish`: download binaries from the same release.  
  - Reads the manifest to fetch only missing or hash-mismatched files; if there is no manifest on first run, it downloads everything.
- Environment variables to customize behavior:  
  - `DEV_RELEASE_TAG`: fixed release tag (default `dev-release`).  
  - `DEV_RELEASE_MANIFEST`: manifest file name (default `riscv_impls_bins_manifest.json`).  
  - `PYTHON_BIN`: Python interpreter path; if unset, the scripts look for `python3`/`python`.
- Before uploading, run `gh auth login` and ensure the CLI token has release permissions.
