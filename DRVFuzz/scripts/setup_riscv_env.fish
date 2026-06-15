#!/usr/bin/env fish

# RISC-V Wrapper Environment Setup Script for Fish Shell
# Source this script to set up required environment variables

# Get the absolute directory where this script is located
set -l SCRIPT_DIR (path dirname (path resolve (status --current-filename)))
# Keep helper binaries exported in current shell and persisted for future Fish sessions
function __set_riscv_wrapper --argument-names name value
    set -gx $name $value
    set -Ux $name $value
end

# Determine repository root (one directory above scripts)
set -l REPO_DIR (path dirname $SCRIPT_DIR)

# Set RISC-V wrapper binary paths - permanently in Fish config and export to subprocesses
__set_riscv_wrapper RISCV_WRAPPER_CVA6_RV32_BIN "$REPO_DIR/riscv_impls_bins/cva6_rv32"
__set_riscv_wrapper RISCV_WRAPPER_CVA6_RV64_BIN "$REPO_DIR/riscv_impls_bins/cva6_rv64"
__set_riscv_wrapper RISCV_WRAPPER_PICORV32_BIN "$REPO_DIR/riscv_impls_bins/picorv32"
__set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV32_BIN "$REPO_DIR/riscv_impls_bins/rocket_rv32"
__set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN "$REPO_DIR/riscv_impls_bins/rocket_rv32_no_d"
__set_riscv_wrapper RISCV_WRAPPER_ROCKET_RV64_BIN "$REPO_DIR/riscv_impls_bins/rocket_rv64"
__set_riscv_wrapper RISCV_WRAPPER_SPIKE_BIN "$REPO_DIR/riscv_impls_bins/spike"
__set_riscv_wrapper RISCV_WRAPPER_BOOM_BIN "$REPO_DIR/riscv_impls_bins/boom_v3_medium_rv64"
__set_riscv_wrapper RISCV_WRAPPER_BOOM_V4_BIN "$REPO_DIR/riscv_impls_bins/boom_v4_medium_rv64"
__set_riscv_wrapper RISCV_WRAPPER_XS_EMU "$REPO_DIR/riscv_impls_bins/xiangshan_rv64"
__set_riscv_wrapper RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW "$REPO_DIR/riscv_impls_bins/xiangshan_rv64_no_unaligned_rw"
__set_riscv_wrapper RISCV_WRAPPER_XS_DIFF_SO "$REPO_DIR/riscv_impls_bins/xiangshan_rv64_ref_spike_so"
__set_riscv_wrapper RISCV_WRAPPER_IBEX_RV32_BIN "$REPO_DIR/riscv_impls_bins/ibex_rv32"
__set_riscv_wrapper RISCV_WRAPPER_VEX_RV32_BIN "$REPO_DIR/riscv_impls_bins/vex_rv32"
__set_riscv_wrapper RISCV_WRAPPER_KRONOS_RV32_BIN "$REPO_DIR/riscv_impls_bins/kronos_rv32"
__set_riscv_wrapper RISCV_WRAPPER_SRV32_BIN "$REPO_DIR/riscv_impls_bins/srv32"

# Verify binaries exist (except spike which is expected to be in PATH)
echo "RISC-V environment variables set:"
echo "  RISCV_WRAPPER_CVA6_RV32_BIN=$RISCV_WRAPPER_CVA6_RV32_BIN"
echo "  RISCV_WRAPPER_CVA6_RV64_BIN=$RISCV_WRAPPER_CVA6_RV64_BIN"
echo "  RISCV_WRAPPER_PICORV32_BIN=$RISCV_WRAPPER_PICORV32_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV32_BIN=$RISCV_WRAPPER_ROCKET_RV32_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN=$RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN"
echo "  RISCV_WRAPPER_ROCKET_RV64_BIN=$RISCV_WRAPPER_ROCKET_RV64_BIN"
echo "  RISCV_WRAPPER_SPIKE_BIN=$RISCV_WRAPPER_SPIKE_BIN"
echo "  RISCV_WRAPPER_BOOM_BIN=$RISCV_WRAPPER_BOOM_BIN"
echo "  RISCV_WRAPPER_BOOM_V4_BIN=$RISCV_WRAPPER_BOOM_V4_BIN"
echo "  RISCV_WRAPPER_XS_EMU=$RISCV_WRAPPER_XS_EMU"
echo "  RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW=$RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW"
echo "  RISCV_WRAPPER_XS_DIFF_SO=$RISCV_WRAPPER_XS_DIFF_SO"
echo "  RISCV_WRAPPER_IBEX_RV32_BIN=$RISCV_WRAPPER_IBEX_RV32_BIN"
echo "  RISCV_WRAPPER_VEX_RV32_BIN=$RISCV_WRAPPER_VEX_RV32_BIN"
echo "  RISCV_WRAPPER_KRONOS_RV32_BIN=$RISCV_WRAPPER_KRONOS_RV32_BIN"
echo "  RISCV_WRAPPER_SRV32_BIN=$RISCV_WRAPPER_SRV32_BIN"

# Check if local binaries exist
for bin in $RISCV_WRAPPER_CVA6_RV32_BIN $RISCV_WRAPPER_CVA6_RV64_BIN $RISCV_WRAPPER_PICORV32_BIN $RISCV_WRAPPER_ROCKET_RV32_BIN $RISCV_WRAPPER_ROCKET_RV32_NO_D_BIN $RISCV_WRAPPER_ROCKET_RV64_BIN $RISCV_WRAPPER_SPIKE_BIN $RISCV_WRAPPER_BOOM_BIN $RISCV_WRAPPER_BOOM_V4_BIN $RISCV_WRAPPER_XS_EMU $RISCV_WRAPPER_XS_EMU_NO_UNALIGNED_RW $RISCV_WRAPPER_IBEX_RV32_BIN $RISCV_WRAPPER_VEX_RV32_BIN $RISCV_WRAPPER_KRONOS_RV32_BIN $RISCV_WRAPPER_SRV32_BIN
    if not test -f "$bin"
        echo "Warning: $bin does not exist"
    end
end

# Check if spike is available in PATH
if not command -v spike >/dev/null 2>&1
    echo "Warning: 'spike' command not found in PATH"
end

echo "Environment setup complete!"
