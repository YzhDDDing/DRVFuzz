pub mod march_util;

pub mod merged_instructions {
    use crate::generate_assemble_march;
    riscv_instruction_macros::generate_merged_riscv_instructions!(
        "../assets/riscv_instructions_new.json"
    );
    generate_assemble_march!();
}

pub mod separated_instructions {
    use crate::generate_assemble_march;
    riscv_instruction_macros::generate_separated_riscv_instructions!(
        "../assets/riscv_instructions_new.json"
    );
    generate_assemble_march!();
}

pub use separated_instructions::{
    build_rv32_march, build_rv64_march, csr_config_from_rv32_extensions,
    csr_config_from_rv64_extensions,
};

#[cfg(test)]
mod test {
    use super::separated_instructions::*;
    use enum_iterator::all;
    use riscv_instruction_types::{MemConfig, MemoryAccessInstruction, RegisterConfig};
    use std::collections::VecDeque;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    fn test_random_config() -> RandomConfig {
        let mem_config = MemConfig::new()
            .with_register_number_range(8, 12)
            .with_immediate_ranges(0, 16);
        let register_config = RegisterConfig::new().with_integer_register_range(13, 20);

        RandomConfig::new()
            .with_mem_config(mem_config)
            .with_register_config(register_config)
    }

    fn generate_rv32_instructions(
        extension: RV32Extensions,
        count: usize,
    ) -> Result<Vec<String>, RandomGenerationError> {
        let mut rng = rand::rng();
        let config = test_random_config();

        (0..count)
            .map(|_| {
                extension
                    .random_sequence_with_rng(&mut rng, &config)
                    .map(|inst| inst.to_string())
            })
            .collect()
    }

    fn generate_rv64_instructions(
        extension: RV64Extensions,
        count: usize,
    ) -> Result<Vec<String>, RandomGenerationError> {
        let mut rng = rand::rng();
        let config = test_random_config();

        (0..count)
            .map(|_| {
                extension
                    .random_sequence_with_rng(&mut rng, &config)
                    .map(|inst| inst.to_string())
            })
            .collect()
    }

    fn create_assembly_file(instructions: &[String], filename: &str) -> std::io::Result<()> {
        // Ensure any existing file with the same name is removed first
        if std::path::Path::new(filename).exists() {
            std::fs::remove_file(filename)?;
        }

        // Build the full assembly file content in memory
        let mut content = String::new();

        content.push_str(".section .text\n");
        content.push_str(".global _start\n");
        content.push_str("_start:\n");

        for inst in instructions {
            content.push_str(&format!("    {}\n", inst));
        }

        content.push_str("    # Exit program\n");
        content.push_str("    li a0, 0\n");
        content.push_str("    li a7, 93\n");
        content.push_str("    ecall\n");

        // Attempt to write the file multiple times
        for attempt in 1..=5 {
            match std::fs::write(filename, &content) {
                Ok(_) => {
                    // After a successful write, wait briefly to let the filesystem sync
                    std::thread::sleep(std::time::Duration::from_millis(10));

                    // Verify the file exists and is readable
                    if std::path::Path::new(filename).exists() {
                        match std::fs::read_to_string(filename) {
                            Ok(read_content) => {
                                if read_content.len() == content.len() {
                                    return Ok(());
                                } else {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!(
                                            "File content length mismatch: expected {}, actual {}",
                                            content.len(),
                                            read_content.len()
                                        ),
                                    ));
                                }
                            }
                            Err(e) => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::PermissionDenied,
                                    format!("Failed to read newly created file: {}", e),
                                ));
                            }
                        }
                    } else {
                        if attempt < 5 {
                            println!("    Attempt {}: file missing after write, retrying...", attempt);
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            continue;
                        } else {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::NotFound,
                                format!("File missing after creation: {} (attempt {})", filename, attempt),
                            ));
                        }
                    }
                }
                Err(e) => {
                    if attempt < 5 {
                        println!("    Attempt {}: write failed {}, retrying...", attempt, e);
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        continue;
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("File write failed (attempt {}): {}", attempt, e),
                        ));
                    }
                }
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "All file creation attempts failed",
        ))
    }

    fn test_assembly(filename: &str, march: &str, output_path: &str) -> (bool, String) {
        // Before testing, verify the input file still exists
        for check_attempt in 1..=3 {
            if std::path::Path::new(filename).exists() {
                break;
            } else {
                if check_attempt < 3 {
                    println!("    Check {}: file missing, waiting...", check_attempt);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                } else {
                    return (false, format!("Input file still missing after all checks: {}", filename));
                }
            }
        }

        // Try reading the file to ensure permissions and content are correct
        match std::fs::File::open(filename) {
            Ok(_) => {
                // File can be opened; continue to validate content
                match std::fs::read_to_string(filename) {
                    Ok(content) => {
                        if content.is_empty() {
                            return (false, format!("Input file {} is empty", filename));
                        }
                        if !content.contains(".section .text") {
                            return (false, format!("Input file {} has an invalid format", filename));
                        }
                    }
                    Err(e) => {
                        return (false, format!("Failed to read input file {}: {}", filename, e));
                    }
                }
            }
            Err(e) => {
                return (false, format!("Failed to open input file {}: {}", filename, e));
            }
        }

        // Get the absolute path of the file
        let abs_path = match std::fs::canonicalize(filename) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(_) => filename.to_string(),
        };

        // Remove any existing output file
        let _ = std::fs::remove_file(output_path);

        let output = Command::new("riscv64-unknown-elf-as")
            .arg(format!("-march={}", march))
            .arg("-o")
            .arg(output_path)
            .arg(&abs_path)
            .output();

        match output {
            Ok(result) => {
                let success = result.status.success();
                let stderr = String::from_utf8_lossy(&result.stderr);
                let stdout = String::from_utf8_lossy(&result.stdout);
                let has_error = stderr.to_lowercase().contains("error");

                let error_info = if !success || has_error {
                    format!(
                        "Exit code: {}\nCommand: riscv64-unknown-elf-as -march={} -o {} {}\nSTDOUT:\n{}\nSTDERR:\n{}",
                        result.status.code().unwrap_or(-1),
                        march,
                        output_path,
                        abs_path,
                        stdout,
                        stderr
                    )
                } else {
                    String::new()
                };

                (success && !has_error, error_info)
            }
            Err(e) => (false, format!("Command execution failed: {}", e)),
        }
    }

    #[derive(Debug, Clone)]
    enum TestOutcome {
        Success,
        Failure { details: String },
    }

    #[derive(Debug, Clone)]
    struct TestResult {
        name: String,
        outcome: TestOutcome,
    }

    impl TestResult {
        fn success(name: String) -> Self {
            Self {
                name,
                outcome: TestOutcome::Success,
            }
        }

        fn failure(name: String, details: impl Into<String>) -> Self {
            Self {
                name,
                outcome: TestOutcome::Failure {
                    details: details.into(),
                },
            }
        }
    }

    const PARALLEL_ENV_VAR: &str = "RISCV_TEST_THREADS";

    fn resolve_parallelism() -> usize {
        if let Ok(value) = std::env::var(PARALLEL_ENV_VAR) {
            match value.parse::<usize>() {
                Ok(parsed) if parsed > 0 => return parsed,
                _ => println!(
                    "Warning: environment variable {} has an invalid value ({}); using default thread count",
                    PARALLEL_ENV_VAR, value
                ),
            }
        }

        std::thread::available_parallelism()
            .map(|count| count.get().saturating_sub(5).max(1))
            .unwrap_or(1)
    }

    fn run_in_parallel<E>(
        items: Vec<E>,
        worker: fn(E) -> TestResult,
        threads: usize,
    ) -> Vec<TestResult>
    where
        E: Send + 'static,
    {
        if items.is_empty() {
            return Vec::new();
        }

        let total_items = items.len();
        let max_threads = threads.max(1).min(total_items);
        let queue = Arc::new(Mutex::new({
            let mut deque = VecDeque::with_capacity(items.len());
            for (idx, item) in items.into_iter().enumerate() {
                deque.push_back((idx, item));
            }
            deque
        }));
        let results = Arc::new(Mutex::new(vec![None; total_items]));

        let mut handles = Vec::with_capacity(max_threads);
        for _ in 0..max_threads {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            handles.push(std::thread::spawn(move || {
                loop {
                    let job = {
                        let mut queue_lock = queue.lock().unwrap();
                        queue_lock.pop_front()
                    };
                    match job {
                        Some((idx, item)) => {
                            let result = worker(item);
                            let mut results_lock = results.lock().unwrap();
                            results_lock[idx] = Some(result);
                        }
                        None => break,
                    }
                }
            }));
        }

        for handle in handles {
            let _ = handle.join();
        }

        let results = Arc::try_unwrap(results)
            .expect("Failed to reclaim result ownership")
            .into_inner()
            .expect("Failed to obtain results");

        results
            .into_iter()
            .map(|entry| entry.expect("Missing test result"))
            .collect()
    }

    fn run_rv32_extension(extension: RV32Extensions) -> TestResult {
        let extension_name = format!("RV32_{:?}", extension);

        let instructions = match generate_rv32_instructions(extension, 1000) {
            Ok(instrs) => instrs,
            Err(err) => {
                return TestResult::failure(extension_name, format!("Random instruction generation failed: {}", err));
            }
        };

        let filename = format!("test_{}.S", extension_name);
        let _ = std::fs::remove_file(&filename);

        if let Err(e) = create_assembly_file(&instructions, &filename) {
            return TestResult::failure(
                extension_name,
                format!("Failed to create assembly file {}: {}", filename, e),
            );
        }

        if !std::path::Path::new(&filename).exists() {
            return TestResult::failure(
                extension_name,
                format!("Assembly file {} does not exist after creation", filename),
            );
        }

        let march = build_rv32_march(&vec![extension]);
        let output_path = format!("output_{}.o", extension_name);
        let (success, error_info) = test_assembly(&filename, &march, &output_path);

        let result = if success {
            TestResult::success(extension_name)
        } else {
            let mut details = if error_info.is_empty() {
                "Unknown error".to_string()
            } else {
                error_info
            };

            match create_error_log(&extension_name, &instructions, &march, &details) {
                Ok((log_file, asm_file)) => {
                    details.push_str(&format!("; error log: {}; assembly file: {}", log_file, asm_file));
                }
                Err(e) => details.push_str(&format!("; failed to create error log: {}", e)),
            }

            TestResult::failure(extension_name, details)
        };

        let _ = std::fs::remove_file(&filename);
        let _ = std::fs::remove_file(&output_path);

        result
    }

    fn run_rv64_extension(extension: RV64Extensions) -> TestResult {
        let extension_name = format!("RV64_{:?}", extension);

        let instructions = match generate_rv64_instructions(extension, 1000) {
            Ok(instrs) => instrs,
            Err(err) => {
                return TestResult::failure(extension_name, format!("Random instruction generation failed: {}", err));
            }
        };

        let filename = format!("test_{}.S", extension_name);
        let _ = std::fs::remove_file(&filename);

        if let Err(e) = create_assembly_file(&instructions, &filename) {
            return TestResult::failure(
                extension_name,
                format!("Failed to create assembly file {}: {}", filename, e),
            );
        }

        if !std::path::Path::new(&filename).exists() {
            return TestResult::failure(
                extension_name,
                format!("Assembly file {} does not exist after creation", filename),
            );
        }

        let march = build_rv64_march(&vec![extension]);
        let output_path = format!("output_{}.o", extension_name);
        let (success, error_info) = test_assembly(&filename, &march, &output_path);

        let result = if success {
            TestResult::success(extension_name)
        } else {
            let mut details = if error_info.is_empty() {
                "Unknown error".to_string()
            } else {
                error_info
            };

            match create_error_log(&extension_name, &instructions, &march, &details) {
                Ok((log_file, asm_file)) => {
                    details.push_str(&format!("; error log: {}; assembly file: {}", log_file, asm_file));
                }
                Err(e) => details.push_str(&format!("; failed to create error log: {}", e)),
            }

            TestResult::failure(extension_name, details)
        };

        let _ = std::fs::remove_file(&filename);
        let _ = std::fs::remove_file(&output_path);

        result
    }

    fn collect_rv32_extensions() -> Vec<RV32Extensions> {
        all::<RV32Extensions>().collect()
    }

    fn collect_rv64_extensions() -> Vec<RV64Extensions> {
        all::<RV64Extensions>().collect()
    }

    fn create_error_log(
        extension_name: &str,
        instructions: &[String],
        march: &str,
        error_info: &str,
    ) -> std::io::Result<(String, String)> {
        // Create the error log directory
        std::fs::create_dir_all("error_logs")?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let log_filename = format!("error_logs/{}_{}_errors.log", extension_name, timestamp);

        // Build the complete log content in memory
        let mut log_content = String::new();

        log_content.push_str("=== Error Log ===\n");
        log_content.push_str(&format!("Instruction set extension: {}\n", extension_name));
        log_content.push_str(&format!("MARCH: {}\n", march));
        log_content.push_str(&format!("Timestamp: {}\n", timestamp));
        log_content.push_str(&format!("Instruction count: {}\n", instructions.len()));
        log_content.push_str("\n");

        log_content.push_str("=== Error Details ===\n");
        log_content.push_str(error_info);
        log_content.push_str("\n\n");

        log_content.push_str("=== Generated Instructions ===\n");
        for (i, inst) in instructions.iter().enumerate() {
            log_content.push_str(&format!("{:4}: {}\n", i + 1, inst));
        }

        // Write the complete log content in one go
        std::fs::write(&log_filename, log_content)?;

        // Also save the assembly file in the error log directory
        let asm_filename = format!("error_logs/{}_{}.S", extension_name, timestamp);
        create_assembly_file(instructions, &asm_filename)?;

        Ok((log_filename, asm_filename))
    }

    #[test]
    fn merged_instruction_offset_access() -> Result<(), Box<dyn std::error::Error>> {
        use super::merged_instructions as merged;

        let xd = merged::IntegerRegister::new(1)?;
        let base = riscv_instruction_types::BaseAddressRegister::new(8)?;
        let imm = riscv_instruction_types::Immediate::<12, true>::new(32)?;
        let lw_struct = merged::I_Shared_LW { imm, xs1: base, xd };

        assert_eq!(lw_struct.offset_operand_value(), Some(32));

        let lw_instruction = merged::RiscvInstruction::Shared(merged::SharedInstruction::I(
            merged::ISharedInstructions::LW(lw_struct.clone()),
        ));
        assert_eq!(lw_instruction.offset_operand_value(), Some(32));

        Ok(())
    }

    #[test]
    fn separated_instruction_offset_access() -> Result<(), Box<dyn std::error::Error>> {
        let xd = IntegerRegister::new(5)?;
        let base = riscv_instruction_types::BaseAddressRegister::new(9)?;
        let imm = riscv_instruction_types::Immediate::<12, true>::new(16)?;
        let lw_struct = RV32_I_LW { imm, xs1: base, xd };

        assert_eq!(lw_struct.offset_operand_value(), Some(16));

        let lw_instruction =
            RiscvInstruction::RV32(RV32Instruction::I(RV32IInstructions::LW(lw_struct.clone())));
        assert_eq!(lw_instruction.offset_operand_value(), Some(16));

        Ok(())
    }

    #[test]
    fn test_all_separated_instructions() {
        let _ = std::fs::remove_file("output.o");
        let _ = std::fs::remove_dir_all("error_logs");

        fn is_zalasr_extension(name: &str) -> bool {
            name.contains("Zalasr")
        }

        let parallelism = resolve_parallelism();

        let rv32_extensions = collect_rv32_extensions();
        let mut results = run_in_parallel(rv32_extensions, run_rv32_extension, parallelism);

        let rv64_extensions = collect_rv64_extensions();
        results.extend(run_in_parallel(
            rv64_extensions,
            run_rv64_extension,
            parallelism,
        ));

        let mut failed_cases = Vec::new();
        let mut ignored_failures = Vec::new();

        for result in results {
            if let TestOutcome::Failure { .. } = &result.outcome {
                if is_zalasr_extension(&result.name) {
                    ignored_failures.push(result);
                } else {
                    failed_cases.push(result);
                }
            }
        }

        if !ignored_failures.is_empty() {
            println!("Ignored failures (GNU toolchain does not yet support Zalasr):");
            for case in &ignored_failures {
                if let TestOutcome::Failure { details, .. } = &case.outcome {
                    println!("- {}: {}", case.name, details);
                }
            }
        }

        if failed_cases.is_empty() {
            println!("No failing cases");
        } else {
            println!("Failing tests:");
            for case in &failed_cases {
                if let TestOutcome::Failure { details, .. } = &case.outcome {
                    println!("- {}: {}", case.name, details);
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.starts_with("test_") && filename.ends_with(".S") {
                        let _ = std::fs::remove_file(filename);
                    }
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(".") {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.starts_with("output_") && filename.ends_with(".o") {
                        let _ = std::fs::remove_file(filename);
                    }
                }
            }
        }

        if failed_cases.is_empty() {
            return;
        }

        panic!("Tests failed! {} test cases failed", failed_cases.len());
    }
}
