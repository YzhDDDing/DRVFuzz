use std::{fs, path::PathBuf, process::Command};

use crate::{
    error::BuildElfError, extension_map::ExtensionMap, isa_base::ISABase, riscv_impls::RiscVImpl,
};

/// ELF build result containing paths to all generated files.
#[derive(Debug, Clone)]
pub struct ElfBuildResult {
    /// Assembly file path actually fed to the toolchain (may be a preprocessed `.s` file).
    pub assembly_file: PathBuf,
    /// Object file path.
    pub object_file: PathBuf,
    /// Executable file path.
    pub executable_file: PathBuf,
    /// Disassembly output path (the dump file).
    pub disassembly_file: PathBuf,
}

/// One-stop ELF compilation that returns detailed build artifacts.
pub fn build_elf_with_extensions<P: AsRef<std::path::Path>>(
    assembly_file: P,
    extension_map: &ExtensionMap,
    isa_base: &ISABase,
    riscv_impl: &RiscVImpl,
) -> Result<ElfBuildResult, BuildElfError> {
    // Build the -march string
    let march = extension_map
        .build_march(isa_base)
        .map_err(|source| BuildElfError::MarchBuild { source })?;
    let mabi = extension_map
        .build_mabi(isa_base)
        .map_err(|source| BuildElfError::MabiBuild { source })?;

    let tool_prefix = "riscv64-unknown-elf";

    let assembly_file_path = assembly_file.as_ref().to_path_buf();
    let linker_script_path = assembly_file_path.with_extension("ld");
    fs::write(&linker_script_path, riscv_impl.linker_script_content()).map_err(|source| {
        BuildElfError::LinkerScriptWrite {
            path: linker_script_path.clone(),
            source,
        }
    })?;
    // Preprocess `.S` files
    let preprocessed_file: Result<PathBuf, BuildElfError> =
        if assembly_file_path.extension().map_or(false, |e| e == "S") {
            let preprocessed_path = assembly_file_path.with_extension("s");

            let gcc_cmd = format!("{}-gcc", tool_prefix);
            let mut cmd = Command::new(&gcc_cmd);
            cmd.arg(format!("-march={}", march));
            cmd.arg(format!("-mabi={}", mabi));
            cmd.arg("-E");
            cmd.arg(&assembly_file_path);
            cmd.arg("-o");
            cmd.arg(&preprocessed_path);

            execute_command(&mut cmd, &gcc_cmd, "preprocessing")?;
            Ok(preprocessed_path)
        } else {
            Ok(assembly_file_path.clone())
        };

    let input_for_assembler = preprocessed_file?;

    // Assemble the input into an object file
    let object_file = assembly_file_path.with_extension("o");
    let as_cmd = format!("{}-as", tool_prefix);
    let mut cmd = Command::new(&as_cmd);
    cmd.arg(format!("-mabi={}", mabi));
    cmd.arg("-g") // Emit debug info so objdump -S can correlate to source
        .arg("-o")
        .arg(&object_file)
        .arg(format!("-march={}", march))
        .arg(&input_for_assembler);

    execute_command(&mut cmd, &as_cmd, "assembling")?;

    // Link the object file into an executable
    let executable_file = assembly_file_path.with_extension("elf");
    let ld_cmd = format!("{}-ld", tool_prefix);
    let mut cmd = Command::new(&ld_cmd);
    // The linker normally infers -mabi from the object file; no need to pass it.
    cmd.arg("-o")
        .arg(&executable_file)
        .arg(&object_file)
        .arg("-T")
        .arg(linker_script_path);

    // Choose linker emulation based on ISA base to avoid ABI mismatches
    match isa_base {
        ISABase::Rv32 => cmd.arg("-m").arg("elf32lriscv"),
        ISABase::Rv64 => cmd.arg("-m").arg("elf64lriscv"),
    };

    execute_command(&mut cmd, &ld_cmd, "linking")?;

    // Produce the disassembly file
    let disassembly_file = assembly_file_path.with_extension("dump");
    let objdump_cmd = format!("{}-objdump", tool_prefix);
    let mut cmd = Command::new(&objdump_cmd);
    cmd.arg("-d") // Disassemble
        .arg("-S") // Intermix source code with disassembly
        .arg(&executable_file)
        .arg("--disassembler-options=no-aliases,numeric");

    let output = execute_command(&mut cmd, &objdump_cmd, "disassembling")?;

    fs::write(&disassembly_file, output.stdout).map_err(|source| {
        BuildElfError::DisassemblyWrite {
            path: disassembly_file.clone(),
            source,
        }
    })?;

    // Bundle the build results
    let result = ElfBuildResult {
        assembly_file: input_for_assembler,
        object_file,
        executable_file,
        disassembly_file,
    };

    Ok(result)
}

fn execute_command(
    cmd: &mut Command,
    command: &str,
    stage: &'static str,
) -> Result<std::process::Output, BuildElfError> {
    log::info!("Executing {} command: {:?}", stage, cmd);
    let output = cmd.output().map_err(|source| BuildElfError::CommandSpawn {
        stage,
        command: command.to_string(),
        source,
    })?;

    if output.status.success() {
        Ok(output)
    } else {
        let stderr = {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr_trimmed = stderr.trim();
            if stderr_trimmed.is_empty() {
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            } else {
                stderr_trimmed.to_owned()
            }
        };

        Err(BuildElfError::CommandFailure {
            stage,
            command: command.to_string(),
            stderr,
        })
    }
}

#[cfg(test)]
mod test {
    use crate::riscv_impls::RiscVImpl;

    #[test]
    fn test_build_elf() {
        let simple_asm = r#"    .section .text
    .globl _start
_start:
    li a0, 42
    li a7, 93
"#;
        let asm_path = std::env::temp_dir().join("test_simple.S");
        std::fs::write(&asm_path, simple_asm).unwrap();
        let isa_base = crate::isa_base::ISABase::Rv64;
        let riscv_impl = RiscVImpl::Spike;
        let extension_map = riscv_impl.extension_map();

        let result =
            super::build_elf_with_extensions(&asm_path, &extension_map, &isa_base, &riscv_impl);
        log::info!("Build result: {:?}", result);
        assert!(result.is_ok());
        let build_result = result.unwrap();
        log::info!("Assembly file: {:?}", build_result.assembly_file);
        log::info!("Object file: {:?}", build_result.object_file);
        log::info!("Executable file: {:?}", build_result.executable_file);
        log::info!("Disassembly file: {:?}", build_result.disassembly_file);

        // Clean up
        let _ = std::fs::remove_file(&asm_path);
        let _ = std::fs::remove_file(asm_path.with_extension("ld"));
        let _ = std::fs::remove_file(build_result.assembly_file);
        let _ = std::fs::remove_file(build_result.object_file);
        let _ = std::fs::remove_file(build_result.executable_file);
        let _ = std::fs::remove_file(build_result.disassembly_file);
    }
}
