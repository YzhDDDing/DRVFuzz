#[macro_export]
macro_rules! generate_assemble_march {
    () => {
        use riscv_instruction_types::CsrConfig as TypesCsrConfig;
        use std::collections::BTreeSet;

        #[derive(Default)]
        struct CsrDomainFlags {
            supervisor: bool,
            hypervisor: bool,
            virtual_supervisor: bool,
            rnmi: bool,
            floating: bool,
            vector: bool,
            debug: bool,
        }

        impl CsrDomainFlags {
            fn finalize(mut self) -> TypesCsrConfig {
                if self.hypervisor {
                    self.supervisor = true;
                    self.virtual_supervisor = true;
                }
                TypesCsrConfig {
                    enable_machine_csrs: true,
                    enable_supervisor_csrs: self.supervisor,
                    enable_hypervisor_csrs: self.hypervisor,
                    enable_virtual_supervisor_csrs: self.virtual_supervisor,
                    enable_rnmi_csrs: self.rnmi,
                    enable_floating_csrs: self.floating,
                    enable_vector_csrs: self.vector,
                    enable_debug_csrs: self.debug,
                }
            }
        }

        /// Assemble collected extension components into a final march string.
        fn assemble_march(
            base: &str,
            mut std_exts: BTreeSet<char>,
            other_exts: BTreeSet<String>,
        ) -> String {
            // If no extensions are provided, return only the base integer ISA ('i').
            if std_exts.is_empty() && other_exts.is_empty() {
                return format!("{}i", base);
            }

            // If any extension is present, the base 'i' extension is required.
            std_exts.insert('i');

            // Build the standard extension section in canonical order (IMAFDQCV).
            let mut std_str = String::new();
            let canonical_order = "imafdqcv";

            for ext_char in canonical_order.chars() {
                if std_exts.remove(&ext_char) {
                    std_str.push(ext_char);
                }
            }
            // Append any remaining standard extensions alphabetically.
            for ext_char in std_exts {
                std_str.push(ext_char);
            }

            let mut result = format!("{}{}", base, std_str);

            // Append all other extensions separated by underscores (BTreeSet keeps them sorted).
            if !other_exts.is_empty() {
                let other_str = other_exts.into_iter().collect::<Vec<String>>().join("_");
                result.push('_');
                result.push_str(&other_str);
            }

            result
        }

        /// Build a RISC-V march string from a list of RV32Extensions.
        pub fn build_rv32_march(extensions: &[RV32Extensions]) -> String {
            if extensions.is_empty() {
                return "rv32i".to_string();
            }
            let mut std_exts = BTreeSet::new();
            let mut other_exts = BTreeSet::new();

            for ext in extensions {
                match ext {
                    // Standard extensions
                    RV32Extensions::I => {
                        std_exts.insert('i');
                    }
                    RV32Extensions::M => {
                        std_exts.insert('m');
                    }
                    RV32Extensions::F => {
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                    }
                    RV32Extensions::D => {
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                    }
                    RV32Extensions::Q => {
                        std_exts.insert('q');
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                        other_exts.insert("zfhmin".to_string());
                    }
                    RV32Extensions::C => {
                        std_exts.insert('c');
                    }
                    RV32Extensions::V => {
                        std_exts.insert('v');
                    }
                    RV32Extensions::H => {
                        other_exts.insert("h".to_string());
                    }

                    // 'B' bundles the Zba/Zbb/Zbc/Zbs extensions
                    RV32Extensions::B => {
                        other_exts.insert("zba".to_string());
                        other_exts.insert("zbb".to_string());
                        other_exts.insert("zbc".to_string());
                        other_exts.insert("zbs".to_string());
                    }

                    // 'A' (atomic) is implied by Za* extensions
                    RV32Extensions::Zaamo => {
                        std_exts.insert('a');
                        other_exts.insert("zaamo".to_string());
                    }
                    RV32Extensions::Zalrsc => {
                        std_exts.insert('a');
                    } // Zalrsc is part of 'A'
                    RV32Extensions::Zacas => {
                        std_exts.insert('a');
                        other_exts.insert("zacas".to_string());
                    }
                    RV32Extensions::Zabha => {
                        std_exts.insert('a');
                        other_exts.insert("zabha".to_string());
                        other_exts.insert("zacas".to_string());
                    }

                    // Zc* extensions with additional dependencies
                    RV32Extensions::Zcb => {
                        std_exts.insert('c');
                        std_exts.insert('m');
                        other_exts.insert("zcb".to_string());
                        other_exts.insert("zbb".to_string());
                    }
                    RV32Extensions::Zcmp => {
                        std_exts.insert('c');
                        other_exts.insert("zcmp".to_string());
                    }
                    RV32Extensions::Zcmop => {
                        std_exts.insert('c');
                        std_exts.insert('a');
                        other_exts.insert("zcmop".to_string());
                        other_exts.insert("zacas".to_string());
                    }
                    RV32Extensions::Zcd => {
                        std_exts.insert('c');
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zcd".to_string());
                    }
                    RV32Extensions::Zcf => {
                        std_exts.insert('c');
                        std_exts.insert('f');
                        other_exts.insert("zcf".to_string());
                    }

                    // Vector extensions; imply 'V'
                    RV32Extensions::Zvbb
                    | RV32Extensions::Zvbc
                    | RV32Extensions::Zvkg
                    | RV32Extensions::Zvks
                    | RV32Extensions::Zvkned
                    | RV32Extensions::Zvknha => {
                        std_exts.insert('v');
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }
                    RV32Extensions::Zvfbfmin | RV32Extensions::Zvfbfwma => {
                        std_exts.insert('v');
                        std_exts.insert('f');
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }

                    // These extensions do not modify the march string in this logic
                    RV32Extensions::S => {}
                    RV32Extensions::Zalasr => {
                        std_exts.insert('a');
                        other_exts.insert("zalasr".to_string());
                    }
                    RV32Extensions::Zilsd => {
                        other_exts.insert("zilsd".to_string());
                    }
                    RV32Extensions::Smrnmi => {
                        other_exts.insert("smrnmi".to_string());
                    }
                    RV32Extensions::Sdext => {}

                    // Other 'Z' extensions
                    RV32Extensions::Zfh => {
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfh".to_string());
                        other_exts.insert("zfa".to_string());
                    }
                    RV32Extensions::Zfbfmin => {
                        std_exts.insert('f');
                        other_exts.insert("zfbfmin".to_string());
                    }
                    RV32Extensions::Svinval => {
                        other_exts.insert("svinval".to_string());
                    }
                    RV32Extensions::Smdbltrp => {
                        other_exts.insert("smdbltrp".to_string());
                        other_exts.insert("smctr".to_string());
                    }

                    // All remaining extensions add their lowercase name directly
                    _ => {
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }
                }
            }
            assemble_march("rv32", std_exts, other_exts)
        }

        /// Derive the CSR domain configuration from an RV32 extension set.
        pub fn csr_config_from_rv32_extensions(extensions: &[RV32Extensions]) -> TypesCsrConfig {
            let mut flags = CsrDomainFlags::default();

            for ext in extensions {
                match ext {
                    RV32Extensions::S | RV32Extensions::Svinval | RV32Extensions::Smdbltrp => {
                        flags.supervisor = true;
                    }
                    RV32Extensions::Sdext => {
                        flags.supervisor = true;
                        flags.debug = true;
                    }
                    RV32Extensions::H => {
                        flags.hypervisor = true;
                    }
                    RV32Extensions::Smrnmi => {
                        flags.rnmi = true;
                    }
                    RV32Extensions::F
                    | RV32Extensions::D
                    | RV32Extensions::Q
                    | RV32Extensions::Zfh
                    | RV32Extensions::Zfbfmin => {
                        flags.floating = true;
                    }
                    RV32Extensions::V
                    | RV32Extensions::Zvbb
                    | RV32Extensions::Zvbc
                    | RV32Extensions::Zvkg
                    | RV32Extensions::Zvks
                    | RV32Extensions::Zvkned
                    | RV32Extensions::Zvknha
                    | RV32Extensions::Zvfbfmin
                    | RV32Extensions::Zvfbfwma => {
                        flags.vector = true;
                    }
                    _ => {}
                }
            }

            flags.finalize()
        }

        /// Build a RISC-V march string from a list of RV64Extensions.
        pub fn build_rv64_march(extensions: &[RV64Extensions]) -> String {
            if extensions.is_empty() {
                return "rv64i".to_string();
            }
            let mut std_exts = BTreeSet::new();
            let mut other_exts = BTreeSet::new();

            for ext in extensions {
                match ext {
                    // Standard extensions
                    RV64Extensions::I => {
                        std_exts.insert('i');
                    }
                    RV64Extensions::M => {
                        std_exts.insert('m');
                    }
                    RV64Extensions::F => {
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                    }
                    RV64Extensions::D => {
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                    }
                    RV64Extensions::Q => {
                        std_exts.insert('q');
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfa".to_string());
                        other_exts.insert("zfhmin".to_string());
                    }
                    RV64Extensions::C => {
                        std_exts.insert('c');
                    }
                    RV64Extensions::V => {
                        std_exts.insert('v');
                    }
                    RV64Extensions::H => {
                        other_exts.insert("h".to_string());
                    }
                    RV64Extensions::B => {
                        other_exts.insert("zba".to_string());
                        other_exts.insert("zbb".to_string());
                        other_exts.insert("zbc".to_string());
                        other_exts.insert("zbs".to_string());
                    }

                    // 'A' (atomic) extension
                    RV64Extensions::Zaamo => {
                        std_exts.insert('a');
                        other_exts.insert("zaamo".to_string());
                    }
                    RV64Extensions::Zalrsc => {
                        std_exts.insert('a');
                    }
                    RV64Extensions::Zacas => {
                        std_exts.insert('a');
                        other_exts.insert("zacas".to_string());
                    }
                    RV64Extensions::Zabha => {
                        std_exts.insert('a');
                        other_exts.insert("zabha".to_string());
                        other_exts.insert("zacas".to_string());
                    }

                    // Zc* extensions
                    RV64Extensions::Zcb => {
                        std_exts.insert('c');
                        std_exts.insert('m');
                        other_exts.insert("zcb".to_string());
                        other_exts.insert("zbb".to_string());
                        other_exts.insert("zba".to_string());
                    }
                    RV64Extensions::Zcmp => {
                        std_exts.insert('c');
                        other_exts.insert("zcmp".to_string());
                    }
                    RV64Extensions::Zcmop => {
                        std_exts.insert('c');
                        other_exts.insert("zcmop".to_string());
                    }
                    RV64Extensions::Zcd => {
                        std_exts.insert('c');
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zcd".to_string());
                    }
                    // Zcf is not present in the RV64Extensions enum

                    // Vector extensions
                    RV64Extensions::Zvbb
                    | RV64Extensions::Zvbc
                    | RV64Extensions::Zvkg
                    | RV64Extensions::Zvks
                    | RV64Extensions::Zvkned
                    | RV64Extensions::Zvknha => {
                        std_exts.insert('v');
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }
                    RV64Extensions::Zvfbfmin | RV64Extensions::Zvfbfwma => {
                        std_exts.insert('v');
                        std_exts.insert('f');
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }

                    // Extensions ignored for march construction
                    RV64Extensions::S => {}
                    RV64Extensions::Zalasr => {
                        std_exts.insert('a');
                        other_exts.insert("zalasr".to_string());
                    }
                    RV64Extensions::Zilsd => {
                        other_exts.insert("zilsd".to_string());
                    }
                    RV64Extensions::Smrnmi => {
                        other_exts.insert("smrnmi".to_string());
                    }
                    RV64Extensions::Sdext => {}

                    // Other 'Z' extensions
                    RV64Extensions::Zfh => {
                        std_exts.insert('d');
                        std_exts.insert('f');
                        other_exts.insert("zfh".to_string());
                        other_exts.insert("zfa".to_string());
                    }
                    RV64Extensions::Zfbfmin => {
                        std_exts.insert('f');
                        other_exts.insert("zfbfmin".to_string());
                    }
                    RV64Extensions::Svinval => {
                        other_exts.insert("svinval".to_string());
                    }
                    RV64Extensions::Smdbltrp => {
                        other_exts.insert("smdbltrp".to_string());
                        other_exts.insert("smctr".to_string());
                    }
                    RV64Extensions::Zkn => {
                        other_exts.insert("zkn".to_string());
                    } // RV64-only extension

                    // All remaining extensions
                    _ => {
                        other_exts.insert(format!("{:?}", ext).to_lowercase());
                    }
                }
            }

            assemble_march("rv64", std_exts, other_exts)
        }

        /// Derive the CSR domain configuration from an RV64 extension set.
        pub fn csr_config_from_rv64_extensions(extensions: &[RV64Extensions]) -> TypesCsrConfig {
            let mut flags = CsrDomainFlags::default();

            for ext in extensions {
                match ext {
                    RV64Extensions::S | RV64Extensions::Svinval | RV64Extensions::Smdbltrp => {
                        flags.supervisor = true;
                    }
                    RV64Extensions::Sdext => {
                        flags.supervisor = true;
                        flags.debug = true;
                    }
                    RV64Extensions::H => {
                        flags.hypervisor = true;
                    }
                    RV64Extensions::Smrnmi => {
                        flags.rnmi = true;
                    }
                    RV64Extensions::F
                    | RV64Extensions::D
                    | RV64Extensions::Q
                    | RV64Extensions::Zfh
                    | RV64Extensions::Zfbfmin => {
                        flags.floating = true;
                    }
                    RV64Extensions::V
                    | RV64Extensions::Zvbb
                    | RV64Extensions::Zvbc
                    | RV64Extensions::Zvkg
                    | RV64Extensions::Zvks
                    | RV64Extensions::Zvkned
                    | RV64Extensions::Zvknha
                    | RV64Extensions::Zvfbfmin
                    | RV64Extensions::Zvfbfwma => {
                        flags.vector = true;
                    }
                    _ => {}
                }
            }

            flags.finalize()
        }
    };
}
