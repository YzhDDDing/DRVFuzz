#!/usr/bin/env bash

# RISC-V Wrapper Environment Setup Script for Bash
# Source this script to set up required environment variables
# Usage: source setup_riscv_env.sh

# Get the absolute directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Determine repository root (one directory above scripts)
REPO_DIR="$(dirname "$SCRIPT_DIR")"

BIN_DIR="$REPO_DIR/riscv_impls_bins"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) export PATH="$BIN_DIR:$PATH" ;;
esac

set_riscv_wrapper() {
    local name="$1"
    local file="$2"
    local required="${3:-required}"
    local path="$BIN_DIR/$file"

    if [[ -f "$path" ]]; then
        export "$name=$path"
        return 0
    fi

    unset "$name"
    if [[ "$required" == "required" ]]; then
        echo "Warning: $path does not exist"
    fi
    return 0
}

# Set RISC-V wrapper binary paths that are present in this repository.
set_riscv_wrapper RISCV_WRAPPER_CVA6_RV32_BIN cva6_rv32
set_riscv_wrapper RISCV_WRAPPER_CVA6_RV64_BIN cva6_rv64
set_riscv_wrapper RISCV_WRAPPER_PICORV32_BIN picorv32
set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV32_BIN rocket_rv32
set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN rocket_rv32_no_d
set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV64_BIN rocket_rv64
set_riscv_wrapper RISCV_WRAPPER_SPIKE_BIN spike
set_riscv_wrapper RISCV_WRAPPER_BOOM_V3_BIN boom_v3_medium_rv64
set_riscv_wrapper RISCV_WRAPPER_BOOM_V4_BIN boom_v4_medium_rv64
set_riscv_wrapper RISCV_WRAPPER_KRONOS_RV32_BIN kronos_rv32
set_riscv_wrapper RISCV_WRAPPER_SRV32_BIN srv32

# Backward-compatible alias used by older docs/scripts.
if [[ -n "${RISCV_WRAPPER_BOOM_V3_BIN:-}" ]]; then
    export RISCV_WRAPPER_BOOM_BIN="$RISCV_WRAPPER_BOOM_V3_BIN"
fi

# Optional implementations supported by the code but not bundled in this repo.
set_riscv_wrapper RISCV_WRAPPER_XS_EMU xiangshan_rv64 optional
set_riscv_wrapper RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW xiangshan_rv64_no_unaligned_rw optional
set_riscv_wrapper RISCV_WRAPPER_XS_DIFF_SO xiangshan_rv64_ref_spike_so optional
set_riscv_wrapper RISCV_WRAPPER_IBEX_RV32_BIN ibex_rv32 optional
set_riscv_wrapper RISCV_WRAPPER_VEX_RV32_BIN vex_rv32 optional

# Verify binaries exist (except spike which may also be on PATH)
echo "RISC-V environment variables set:"
for var in \
    RISCV_WRAPPER_CVA6_RV32_BIN \
    RISCV_WRAPPER_CVA6_RV64_BIN \
    RISCV_WRAPPER_PICORV32_BIN \
    RISCV_WRAPPER_ROCKET_RV32_BIN \
    RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN \
    RISCV_WRAPPER_ROCKET_RV64_BIN \
    RISCV_WRAPPER_SPIKE_BIN \
    RISCV_WRAPPER_BOOM_V3_BIN \
    RISCV_WRAPPER_BOOM_BIN \
    RISCV_WRAPPER_BOOM_V4_BIN \
    RISCV_WRAPPER_XS_EMU \
    RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW \
    RISCV_WRAPPER_XS_DIFF_SO \
    RISCV_WRAPPER_IBEX_RV32_BIN \
    RISCV_WRAPPER_VEX_RV32_BIN \
    RISCV_WRAPPER_KRONOS_RV32_BIN \
    RISCV_WRAPPER_SRV32_BIN
do
    if [[ -n "${!var:-}" ]]; then
        echo "  $var=${!var}"
    fi
done

# Check if local binaries exist
for bin in \
    "${RISCV_WRAPPER_CVA6_RV32_BIN:-}" \
    "${RISCV_WRAPPER_CVA6_RV64_BIN:-}" \
    "${RISCV_WRAPPER_PICORV32_BIN:-}" \
    "${RISCV_WRAPPER_ROCKET_RV32_BIN:-}" \
    "${RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN:-}" \
    "${RISCV_WRAPPER_ROCKET_RV64_BIN:-}" \
    "${RISCV_WRAPPER_SPIKE_BIN:-}" \
    "${RISCV_WRAPPER_BOOM_V3_BIN:-}" \
    "${RISCV_WRAPPER_BOOM_V4_BIN:-}" \
    "${RISCV_WRAPPER_XS_EMU:-}" \
    "${RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW:-}" \
    "${RISCV_WRAPPER_XS_DIFF_SO:-}" \
    "${RISCV_WRAPPER_IBEX_RV32_BIN:-}" \
    "${RISCV_WRAPPER_VEX_RV32_BIN:-}" \
    "${RISCV_WRAPPER_KRONOS_RV32_BIN:-}" \
    "${RISCV_WRAPPER_SRV32_BIN:-}"
do
    if [[ -z "$bin" ]]; then
        continue
    fi
    if [[ ! -f "$bin" ]]; then
        echo "Warning: $bin does not exist"
    fi
done

# Check if spike is available in PATH
if ! command -v spike >/dev/null 2>&1; then
    echo "Warning: 'spike' command not found in PATH"
fi

echo "Environment setup complete!"
