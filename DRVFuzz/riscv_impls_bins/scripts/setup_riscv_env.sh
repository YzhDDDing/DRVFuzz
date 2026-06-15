#!/usr/bin/env bash

# RISC-V Wrapper Environment Setup Script for Bash
# Source this script to set up required environment variables
# Usage: source setup_riscv_env.sh

# Get the absolute directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Determine repository root (one directory above scripts)
REPO_DIR="$(dirname "$SCRIPT_DIR")"

# Set RISC-V wrapper binary paths - export to current shell and subprocesses
export RISCV_WRAPPER_CVA6_RV32_BIN="$REPO_DIR/riscv_impls_bins/cva6_rv32"
export RISCV_WRAPPER_CVA6_RV64_BIN="$REPO_DIR/riscv_impls_bins/cva6_rv64"
export RISCV_WRAPPER_PICORV32_BIN="$REPO_DIR/riscv_impls_bins/picorv32"
export RISCV_WRAPPER_ROCKET_RV32_BIN="$REPO_DIR/riscv_impls_bins/rocket_rv32"
export RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN="$REPO_DIR/riscv_impls_bins/rocket_rv32_no_d"
export RISCV_WRAPPER_ROCKET_RV64_BIN="$REPO_DIR/riscv_impls_bins/rocket_rv64"
export RISCV_WRAPPER_SPIKE_BIN="$REPO_DIR/riscv_impls_bins/spike"
export RISCV_WRAPPER_XS_EMU="$REPO_DIR/riscv_impls_bins/xiangshan_rv64"
export RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW="$REPO_DIR/riscv_impls_bins/xiangshan_rv64_no_unaligned_rw"
export RISCV_WRAPPER_XS_DIFF_SO="$REPO_DIR/riscv_impls_bins/xiangshan_rv64_ref_spike_so"
export RISCV_WRAPPER_IBEX_RV32_BIN="$REPO_DIR/riscv_impls_bins/ibex_rv32"
export RISCV_WRAPPER_VEX_RV32_BIN="$REPO_DIR/riscv_impls_bins/vex_rv32"
export RISCV_WRAPPER_KRONOS_RV32_BIN="$REPO_DIR/riscv_impls_bins/kronos_rv32"

# Verify binaries exist (except spike which is expected to be in PATH)
echo "RISC-V environment variables set:"
echo "  RISCV_WRAPPER_CVA6_RV32_BIN=$RISCV_WRAPPER_CVA6_RV32_BIN"
echo "  RISCV_WRAPPER_CVA6_RV64_BIN=$RISCV_WRAPPER_CVA6_RV64_BIN"
echo "  RISCV_WRAPPER_PICORV32_BIN=$RISCV_WRAPPER_PICORV32_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV32_BIN=$RISCV_WRAPPER_ROCKET_RV32_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN=$RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV64_BIN=$RISCV_WRAPPER_ROCKET_RV64_BIN"
echo "  RISCV_WRAPPER_SPIKE_BIN=$RISCV_WRAPPER_SPIKE_BIN"
echo "  RISCV_WRAPPER_XS_EMU=$RISCV_WRAPPER_XS_EMU"
echo "  RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW=$RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW"
echo "  RISCV_WRAPPER_XS_DIFF_SO=$RISCV_WRAPPER_XS_DIFF_SO"
echo "  RISCV_WRAPPER_IBEX_RV32_BIN=$RISCV_WRAPPER_IBEX_RV32_BIN"
echo "  RISCV_WRAPPER_VEX_RV32_BIN=$RISCV_WRAPPER_VEX_RV32_BIN"
echo "  RISCV_WRAPPER_KRONOS_RV32_BIN=$RISCV_WRAPPER_KRONOS_RV32_BIN"

# Check if local binaries exist
for bin in "$RISCV_WRAPPER_CVA6_RV32_BIN" "$RISCV_WRAPPER_CVA6_RV64_BIN" "$RISCV_WRAPPER_PICORV32_BIN" "$RISCV_WRAPPER_ROCKET_RV32_BIN" "$RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN" "$RISCV_WRAPPER_ROCKET_RV64_BIN" "$RISCV_WRAPPER_SPIKE_BIN" "$RISCV_WRAPPER_XS_EMU" "$RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW" "$RISCV_WRAPPER_XS_DIFF_SO" "$RISCV_WRAPPER_IBEX_RV32_BIN" "$RISCV_WRAPPER_VEX_RV32_BIN" "$RISCV_WRAPPER_KRONOS_RV32_BIN"; do
    if [[ ! -f "$bin" ]]; then
        echo "Warning: $bin does not exist"
    fi
done

# Check if spike is available in PATH
if ! command -v spike >/dev/null 2>&1; then
    echo "Warning: 'spike' command not found in PATH"
fi

echo "Environment setup complete!"