use crate::{CsrConfig, Random, RandomConfig, RandomGenerationError};
use rand::Rng;
use rand::seq::IndexedRandom as _;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CsrDomain {
    Machine,
    Supervisor,
    Hypervisor,
    VirtualSupervisor,
    Rnmi,
    Floating,
    Vector,
    Debug,
}

impl CsrDomain {
    fn enabled(self, cfg: &CsrConfig) -> bool {
        match self {
            Self::Machine => cfg.enable_machine_csrs,
            Self::Supervisor => cfg.enable_supervisor_csrs,
            Self::Hypervisor => cfg.enable_hypervisor_csrs,
            Self::VirtualSupervisor => cfg.enable_virtual_supervisor_csrs,
            Self::Rnmi => cfg.enable_rnmi_csrs,
            Self::Floating => cfg.enable_floating_csrs,
            Self::Vector => cfg.enable_vector_csrs,
            Self::Debug => cfg.enable_debug_csrs,
        }
    }
}

/// Writable CSRs that the generator can legally target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WritableCsr {
    Mstatus,
    Misa,
    Mtvec,
    Medeleg,
    Mideleg,
    Mie,
    Mip,
    Mscratch,
    Mepc,
    Mcause,
    Mtval,
    Mcounteren,
    Menvcfg,
    Menvcfgh,
    Mtval2,
    Pmpcfg0,
    Pmpcfg1,
    Pmpcfg2,
    Pmpcfg3,
    Pmpaddr0,
    Pmpaddr1,
    Pmpaddr2,
    Pmpaddr3,
    Pmpaddr4,
    Pmpaddr5,
    Pmpaddr6,
    Pmpaddr7,
    Pmpaddr8,
    Pmpaddr9,
    Pmpaddr10,
    Pmpaddr11,
    Pmpaddr12,
    Pmpaddr13,
    Pmpaddr14,
    Pmpaddr15,
    Cycle,
    Time,
    Instret,
    Scounteren,
    Sstatus,
    Sie,
    Sip,
    Stvec,
    Sscratch,
    Sepc,
    Scause,
    Stval,
    Satp,
    Vtype,
    Vstart,
    Vxrm,
    Vxsat,
    Fflags,
    Frm,
    Fcsr,
    Dcsr,
    Dpc,
    Dscratch0,
    Dscratch1,
    Hstatus,
    Hedeleg,
    Hideleg,
    Hie,
    Hip,
    Hvip,
    Hgeie,
    Hgeip,
    Hvictl,
    Htval,
    Htinst,
    Hgatp,
    Henvcfg,
    Henvcfgh,
    Hcounteren,
    Htimedelta,
    Htimedeltah,
    Vsstatus,
    Vsie,
    Vsip,
    Vstvec,
    Vsscratch,
    Vsepc,
    Vscause,
    Vstval,
    Vsatp,
    Mnstatus,
    Mnscratch,
    Mnepc,
    Mncause,
}

const WRITABLE_CSRS: &[WritableCsr] = &[
    WritableCsr::Mstatus,
    WritableCsr::Misa,
    WritableCsr::Mtvec,
    WritableCsr::Medeleg,
    WritableCsr::Mideleg,
    WritableCsr::Mie,
    WritableCsr::Mip,
    WritableCsr::Mscratch,
    WritableCsr::Mepc,
    WritableCsr::Mcause,
    WritableCsr::Mtval,
    WritableCsr::Mcounteren,
    WritableCsr::Menvcfg,
    WritableCsr::Menvcfgh,
    WritableCsr::Mtval2,
    WritableCsr::Pmpcfg0,
    WritableCsr::Pmpcfg1,
    WritableCsr::Pmpcfg2,
    WritableCsr::Pmpcfg3,
    WritableCsr::Pmpaddr0,
    WritableCsr::Pmpaddr1,
    WritableCsr::Pmpaddr2,
    WritableCsr::Pmpaddr3,
    WritableCsr::Pmpaddr4,
    WritableCsr::Pmpaddr5,
    WritableCsr::Pmpaddr6,
    WritableCsr::Pmpaddr7,
    WritableCsr::Pmpaddr8,
    WritableCsr::Pmpaddr9,
    WritableCsr::Pmpaddr10,
    WritableCsr::Pmpaddr11,
    WritableCsr::Pmpaddr12,
    WritableCsr::Pmpaddr13,
    WritableCsr::Pmpaddr14,
    WritableCsr::Pmpaddr15,
    WritableCsr::Cycle,
    WritableCsr::Time,
    WritableCsr::Instret,
    WritableCsr::Scounteren,
    WritableCsr::Sstatus,
    WritableCsr::Sie,
    WritableCsr::Sip,
    WritableCsr::Stvec,
    WritableCsr::Sscratch,
    WritableCsr::Sepc,
    WritableCsr::Scause,
    WritableCsr::Stval,
    WritableCsr::Satp,
    WritableCsr::Vtype,
    WritableCsr::Vstart,
    WritableCsr::Vxrm,
    WritableCsr::Vxsat,
    WritableCsr::Fflags,
    WritableCsr::Frm,
    WritableCsr::Fcsr,
    WritableCsr::Dcsr,
    WritableCsr::Dpc,
    WritableCsr::Dscratch0,
    WritableCsr::Dscratch1,
    WritableCsr::Hstatus,
    WritableCsr::Hedeleg,
    WritableCsr::Hideleg,
    WritableCsr::Hie,
    WritableCsr::Hip,
    WritableCsr::Hvip,
    WritableCsr::Hgeie,
    WritableCsr::Hgeip,
    WritableCsr::Hvictl,
    WritableCsr::Htval,
    WritableCsr::Htinst,
    WritableCsr::Hgatp,
    WritableCsr::Henvcfg,
    WritableCsr::Henvcfgh,
    WritableCsr::Hcounteren,
    WritableCsr::Htimedelta,
    WritableCsr::Htimedeltah,
    WritableCsr::Vsstatus,
    WritableCsr::Vsie,
    WritableCsr::Vsip,
    WritableCsr::Vstvec,
    WritableCsr::Vsscratch,
    WritableCsr::Vsepc,
    WritableCsr::Vscause,
    WritableCsr::Vstval,
    WritableCsr::Vsatp,
    WritableCsr::Mnstatus,
    WritableCsr::Mnscratch,
    WritableCsr::Mnepc,
    WritableCsr::Mncause,
];

impl WritableCsr {
    fn domain(self) -> CsrDomain {
        match self {
            Self::Mstatus
            | Self::Misa
            | Self::Mtvec
            | Self::Mie
            | Self::Mip
            | Self::Mscratch
            | Self::Mepc
            | Self::Mcause
            | Self::Mtval
            | Self::Mcounteren
            | Self::Menvcfg
            | Self::Menvcfgh
            | Self::Mtval2
            | Self::Pmpcfg0
            | Self::Pmpcfg1
            | Self::Pmpcfg2
            | Self::Pmpcfg3
            | Self::Pmpaddr0
            | Self::Pmpaddr1
            | Self::Pmpaddr2
            | Self::Pmpaddr3
            | Self::Pmpaddr4
            | Self::Pmpaddr5
            | Self::Pmpaddr6
            | Self::Pmpaddr7
            | Self::Pmpaddr8
            | Self::Pmpaddr9
            | Self::Pmpaddr10
            | Self::Pmpaddr11
            | Self::Pmpaddr12
            | Self::Pmpaddr13
            | Self::Pmpaddr14
            | Self::Pmpaddr15
            | Self::Cycle
            | Self::Time
            | Self::Instret => CsrDomain::Machine,
            Self::Medeleg
            | Self::Mideleg
            | Self::Scounteren
            | Self::Sstatus
            | Self::Sie
            | Self::Sip
            | Self::Stvec
            | Self::Sscratch
            | Self::Sepc
            | Self::Scause
            | Self::Stval
            | Self::Satp => CsrDomain::Supervisor,
            Self::Hstatus
            | Self::Hedeleg
            | Self::Hideleg
            | Self::Hie
            | Self::Hip
            | Self::Hvip
            | Self::Hgeie
            | Self::Hgeip
            | Self::Hvictl
            | Self::Htval
            | Self::Htinst
            | Self::Hgatp
            | Self::Henvcfg
            | Self::Henvcfgh
            | Self::Hcounteren
            | Self::Htimedelta
            | Self::Htimedeltah => CsrDomain::Hypervisor,
            Self::Vsstatus
            | Self::Vsie
            | Self::Vsip
            | Self::Vstvec
            | Self::Vsscratch
            | Self::Vsepc
            | Self::Vscause
            | Self::Vstval
            | Self::Vsatp => CsrDomain::VirtualSupervisor,
            Self::Mnstatus | Self::Mnscratch | Self::Mnepc | Self::Mncause => CsrDomain::Rnmi,
            Self::Vtype | Self::Vstart | Self::Vxrm | Self::Vxsat => CsrDomain::Vector,
            Self::Fflags | Self::Frm | Self::Fcsr => CsrDomain::Floating,
            Self::Dcsr | Self::Dpc | Self::Dscratch0 | Self::Dscratch1 => CsrDomain::Debug,
        }
    }

    /// Parse a CSR name (lowercase assembly mnemonic) back into a writable CSR variant.
    pub fn from_name(name: &str) -> Option<Self> {
        if name.is_empty() {
            return None;
        }
        let needle = name.trim().to_ascii_lowercase();
        WRITABLE_CSRS
            .iter()
            .copied()
            .find(|csr| csr.to_string() == needle)
    }

    fn is_enabled(&self, cfg: &CsrConfig) -> bool {
        self.domain().enabled(cfg)
    }

    /// Randomly pick one writable CSR variant allowed by the provided extensions.
    pub fn random_enabled<R: Rng + ?Sized>(rng: &mut R, csr_config: &CsrConfig) -> Option<Self> {
        let choices: Vec<_> = WRITABLE_CSRS
            .iter()
            .copied()
            .filter(|csr| csr.is_enabled(csr_config))
            .collect();
        choices.choose(rng).copied()
    }

    /// Generate a random legal value for the current CSR variant.
    pub fn random_legal_value<R: Rng + ?Sized>(&self, rng: &mut R) -> u64 {
        match self {
            Self::Mstatus => random_mstatus(rng),
            Self::Misa => random_misa(rng),
            Self::Mtvec => random_mtvec(rng),
            Self::Medeleg => random_masked(rng, MEDELEG_MASK),
            Self::Mideleg => random_masked(rng, MIDELEG_MASK),
            Self::Mie | Self::Mip => random_masked(rng, INTERRUPT_MASK),
            Self::Mscratch => rng.random::<u64>(),
            Self::Mepc => random_epc(rng),
            Self::Mcause => random_cause(rng),
            Self::Mtval => rng.random::<u64>(),
            Self::Mcounteren => random_masked(rng, HPM_MASK),
            Self::Menvcfg | Self::Menvcfgh => random_menvcfg(rng),
            Self::Mtval2 => rng.random::<u64>(),
            Self::Pmpcfg0 | Self::Pmpcfg1 | Self::Pmpcfg2 | Self::Pmpcfg3 => random_pmpcfg(rng),
            Self::Pmpaddr0
            | Self::Pmpaddr1
            | Self::Pmpaddr2
            | Self::Pmpaddr3
            | Self::Pmpaddr4
            | Self::Pmpaddr5
            | Self::Pmpaddr6
            | Self::Pmpaddr7
            | Self::Pmpaddr8
            | Self::Pmpaddr9
            | Self::Pmpaddr10
            | Self::Pmpaddr11
            | Self::Pmpaddr12
            | Self::Pmpaddr13
            | Self::Pmpaddr14
            | Self::Pmpaddr15 => random_pmpaddr(rng),
            Self::Cycle | Self::Time | Self::Instret => random_counter(rng),
            Self::Scounteren => random_masked(rng, HPM_MASK),
            Self::Sstatus => random_sstatus(rng),
            Self::Sie | Self::Sip => random_masked(rng, SUPERVISOR_INTERRUPT_MASK),
            Self::Stvec => random_mtvec(rng),
            Self::Sscratch => rng.random::<u64>(),
            Self::Sepc => random_epc(rng),
            Self::Scause => random_cause(rng),
            Self::Stval => rng.random::<u64>(),
            Self::Satp => random_satp(rng),
            Self::Vtype => random_vtype(rng),
            Self::Vstart => random_vstart(rng),
            Self::Vxrm => random_vxrm(rng),
            Self::Vxsat => random_vxsat(rng),
            Self::Fflags => random_fflags(rng),
            Self::Frm => random_frm(rng),
            Self::Fcsr => random_fcsr(rng),
            Self::Dcsr => random_dcsr(rng),
            Self::Dpc => random_epc(rng),
            Self::Dscratch0 | Self::Dscratch1 => rng.random::<u64>(),
            Self::Hstatus => random_hstatus(rng),
            Self::Hedeleg => random_masked(rng, HEDELEG_MASK),
            Self::Hideleg => random_masked(rng, HIDELEG_MASK),
            Self::Hie | Self::Hip | Self::Hvip | Self::Hgeie | Self::Hgeip => {
                random_masked(rng, HYPERVISOR_INTERRUPT_MASK)
            }
            Self::Hvictl => random_hvictl(rng),
            Self::Htval | Self::Htinst => rng.random::<u64>(),
            Self::Hgatp => random_hgatp(rng),
            Self::Henvcfg | Self::Henvcfgh => random_henvcfg(rng),
            Self::Hcounteren => random_masked(rng, HPM_MASK),
            Self::Htimedelta | Self::Htimedeltah => rng.random::<u64>(),
            Self::Vsstatus => random_sstatus(rng),
            Self::Vsie | Self::Vsip => random_masked(rng, SUPERVISOR_INTERRUPT_MASK),
            Self::Vstvec => random_mtvec(rng),
            Self::Vsscratch => rng.random::<u64>(),
            Self::Vsepc => random_epc(rng),
            Self::Vscause => random_cause(rng),
            Self::Vstval => rng.random::<u64>(),
            Self::Vsatp => random_satp(rng),
            Self::Mnstatus => random_mnstatus(rng),
            Self::Mnscratch => rng.random::<u64>(),
            Self::Mnepc => random_epc(rng),
            Self::Mncause => random_cause(rng),
        }
    }
}

impl Random for WritableCsr {
    type Output = Self;

    fn random_with_rng<R: Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        Self::random_enabled(rng, &config.csr_config).ok_or_else(|| {
            RandomGenerationError::new(
                stringify!(WritableCsr),
                "no writable CSR enabled for current extension filter",
            )
        })
    }
}

impl fmt::Display for WritableCsr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_csr_lowercase(self, f)
    }
}

/// CSRs that must exist per the privileged specification and are always readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadableCsr {
    Mstatus,
    Misa,
    Mtvec,
    Medeleg,
    Mideleg,
    Mie,
    Mip,
    Mscratch,
    Mepc,
    Mcause,
    Mtval,
    Mcounteren,
    Menvcfg,
    Menvcfgh,
    Mtval2,
    Pmpcfg0,
    Pmpcfg1,
    Pmpcfg2,
    Pmpcfg3,
    Pmpaddr0,
    Pmpaddr1,
    Pmpaddr2,
    Pmpaddr3,
    Pmpaddr4,
    Pmpaddr5,
    Pmpaddr6,
    Pmpaddr7,
    Pmpaddr8,
    Pmpaddr9,
    Pmpaddr10,
    Pmpaddr11,
    Pmpaddr12,
    Pmpaddr13,
    Pmpaddr14,
    Pmpaddr15,
    Scounteren,
    Satp,
    Vtype,
    Vstart,
    Vxrm,
    Vxsat,
    Fflags,
    Frm,
    Fcsr,
    Dcsr,
    Dpc,
    Dscratch0,
    Dscratch1,
    Cycle,
    Instret,
    Time,
    Mhartid,
    Mvendorid,
    Marchid,
    Mimpid,
    Sstatus,
    Sie,
    Sip,
    Stvec,
    Sscratch,
    Sepc,
    Scause,
    Stval,
    Hstatus,
    Hedeleg,
    Hideleg,
    Hie,
    Hip,
    Hvip,
    Hgeie,
    Hgeip,
    Hvictl,
    Htval,
    Htinst,
    Hgatp,
    Henvcfg,
    Henvcfgh,
    Hcounteren,
    Htimedelta,
    Htimedeltah,
    Vsstatus,
    Vsie,
    Vsip,
    Vstvec,
    Vsscratch,
    Vsepc,
    Vscause,
    Vstval,
    Vsatp,
    Mnstatus,
    Mnscratch,
    Mnepc,
    Mncause,
    Vl,
    Vlenb,
}

const READABLE_CSRS: &[ReadableCsr] = &[
    ReadableCsr::Mstatus,
    ReadableCsr::Misa,
    ReadableCsr::Mtvec,
    ReadableCsr::Medeleg,
    ReadableCsr::Mideleg,
    ReadableCsr::Mie,
    ReadableCsr::Mip,
    ReadableCsr::Mscratch,
    ReadableCsr::Mepc,
    ReadableCsr::Mcause,
    ReadableCsr::Mtval,
    ReadableCsr::Mcounteren,
    ReadableCsr::Menvcfg,
    ReadableCsr::Menvcfgh,
    ReadableCsr::Mtval2,
    ReadableCsr::Pmpcfg0,
    ReadableCsr::Pmpcfg1,
    ReadableCsr::Pmpcfg2,
    ReadableCsr::Pmpcfg3,
    ReadableCsr::Pmpaddr0,
    ReadableCsr::Pmpaddr1,
    ReadableCsr::Pmpaddr2,
    ReadableCsr::Pmpaddr3,
    ReadableCsr::Pmpaddr4,
    ReadableCsr::Pmpaddr5,
    ReadableCsr::Pmpaddr6,
    ReadableCsr::Pmpaddr7,
    ReadableCsr::Pmpaddr8,
    ReadableCsr::Pmpaddr9,
    ReadableCsr::Pmpaddr10,
    ReadableCsr::Pmpaddr11,
    ReadableCsr::Pmpaddr12,
    ReadableCsr::Pmpaddr13,
    ReadableCsr::Pmpaddr14,
    ReadableCsr::Pmpaddr15,
    ReadableCsr::Scounteren,
    ReadableCsr::Satp,
    ReadableCsr::Vtype,
    ReadableCsr::Vstart,
    ReadableCsr::Vxrm,
    ReadableCsr::Vxsat,
    ReadableCsr::Fflags,
    ReadableCsr::Frm,
    ReadableCsr::Fcsr,
    ReadableCsr::Dcsr,
    ReadableCsr::Dpc,
    ReadableCsr::Dscratch0,
    ReadableCsr::Dscratch1,
    ReadableCsr::Cycle,
    ReadableCsr::Instret,
    ReadableCsr::Time,
    ReadableCsr::Mhartid,
    ReadableCsr::Mvendorid,
    ReadableCsr::Marchid,
    ReadableCsr::Mimpid,
    ReadableCsr::Sstatus,
    ReadableCsr::Sie,
    ReadableCsr::Sip,
    ReadableCsr::Stvec,
    ReadableCsr::Sscratch,
    ReadableCsr::Sepc,
    ReadableCsr::Scause,
    ReadableCsr::Stval,
    ReadableCsr::Hstatus,
    ReadableCsr::Hedeleg,
    ReadableCsr::Hideleg,
    ReadableCsr::Hie,
    ReadableCsr::Hip,
    ReadableCsr::Hvip,
    ReadableCsr::Hgeie,
    ReadableCsr::Hgeip,
    ReadableCsr::Hvictl,
    ReadableCsr::Htval,
    ReadableCsr::Htinst,
    ReadableCsr::Hgatp,
    ReadableCsr::Henvcfg,
    ReadableCsr::Henvcfgh,
    ReadableCsr::Hcounteren,
    ReadableCsr::Htimedelta,
    ReadableCsr::Htimedeltah,
    ReadableCsr::Vsstatus,
    ReadableCsr::Vsie,
    ReadableCsr::Vsip,
    ReadableCsr::Vstvec,
    ReadableCsr::Vsscratch,
    ReadableCsr::Vsepc,
    ReadableCsr::Vscause,
    ReadableCsr::Vstval,
    ReadableCsr::Vsatp,
    ReadableCsr::Mnstatus,
    ReadableCsr::Mnscratch,
    ReadableCsr::Mnepc,
    ReadableCsr::Mncause,
    ReadableCsr::Vl,
    ReadableCsr::Vlenb,
];

impl ReadableCsr {
    fn domain(self) -> CsrDomain {
        match self {
            Self::Mstatus
            | Self::Misa
            | Self::Mtvec
            | Self::Mie
            | Self::Mip
            | Self::Mscratch
            | Self::Mepc
            | Self::Mcause
            | Self::Mtval
            | Self::Mcounteren
            | Self::Menvcfg
            | Self::Menvcfgh
            | Self::Mtval2
            | Self::Pmpcfg0
            | Self::Pmpcfg1
            | Self::Pmpcfg2
            | Self::Pmpcfg3
            | Self::Pmpaddr0
            | Self::Pmpaddr1
            | Self::Pmpaddr2
            | Self::Pmpaddr3
            | Self::Pmpaddr4
            | Self::Pmpaddr5
            | Self::Pmpaddr6
            | Self::Pmpaddr7
            | Self::Pmpaddr8
            | Self::Pmpaddr9
            | Self::Pmpaddr10
            | Self::Pmpaddr11
            | Self::Pmpaddr12
            | Self::Pmpaddr13
            | Self::Pmpaddr14
            | Self::Pmpaddr15
            | Self::Cycle
            | Self::Instret
            | Self::Time
            | Self::Mhartid
            | Self::Mvendorid
            | Self::Marchid
            | Self::Mimpid => CsrDomain::Machine,
            Self::Medeleg
            | Self::Mideleg
            | Self::Scounteren
            | Self::Satp
            | Self::Sstatus
            | Self::Sie
            | Self::Sip
            | Self::Stvec
            | Self::Sscratch
            | Self::Sepc
            | Self::Scause
            | Self::Stval => CsrDomain::Supervisor,
            Self::Hstatus
            | Self::Hedeleg
            | Self::Hideleg
            | Self::Hie
            | Self::Hip
            | Self::Hvip
            | Self::Hgeie
            | Self::Hgeip
            | Self::Hvictl
            | Self::Htval
            | Self::Htinst
            | Self::Hgatp
            | Self::Henvcfg
            | Self::Henvcfgh
            | Self::Hcounteren
            | Self::Htimedelta
            | Self::Htimedeltah => CsrDomain::Hypervisor,
            Self::Vsstatus
            | Self::Vsie
            | Self::Vsip
            | Self::Vstvec
            | Self::Vsscratch
            | Self::Vsepc
            | Self::Vscause
            | Self::Vstval
            | Self::Vsatp => CsrDomain::VirtualSupervisor,
            Self::Mnstatus | Self::Mnscratch | Self::Mnepc | Self::Mncause => CsrDomain::Rnmi,
            Self::Vtype | Self::Vstart | Self::Vxrm | Self::Vxsat | Self::Vl | Self::Vlenb => {
                CsrDomain::Vector
            }
            Self::Fflags | Self::Frm | Self::Fcsr => CsrDomain::Floating,
            Self::Dcsr | Self::Dpc | Self::Dscratch0 | Self::Dscratch1 => CsrDomain::Debug,
        }
    }

    fn is_enabled(&self, cfg: &CsrConfig) -> bool {
        self.domain().enabled(cfg)
    }

    /// Randomly pick one readable CSR variant allowed by the provided configuration.
    pub fn random_enabled<R: Rng + ?Sized>(rng: &mut R, csr_config: &CsrConfig) -> Option<Self> {
        let choices: Vec<_> = READABLE_CSRS
            .iter()
            .copied()
            .filter(|csr| csr.is_enabled(csr_config))
            .collect();
        choices.choose(rng).copied()
    }
}

impl Random for ReadableCsr {
    type Output = Self;

    fn random_with_rng<R: Rng>(
        rng: &mut R,
        config: &RandomConfig,
    ) -> Result<Self::Output, RandomGenerationError> {
        ReadableCsr::random_enabled(rng, &config.csr_config).ok_or_else(|| {
            RandomGenerationError::new(
                stringify!(ReadableCsr),
                "no readable CSR enabled for current extension filter",
            )
        })
    }
}

impl fmt::Display for ReadableCsr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_csr_lowercase(self, f)
    }
}

const SIMPLE_MSTATUS_MASK: u64 = (1 << 0)
    | (1 << 1)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 7)
    | (1 << 17)
    | (1 << 18)
    | (1 << 19)
    | (1 << 20)
    | (1 << 21)
    | (1 << 22);
const MEDELEG_MASK: u64 = 0xFFFF;
const MIDELEG_MASK: u64 = (1 << 9) | (1 << 8) | (1 << 5) | (1 << 4) | (1 << 1) | (1 << 0);
const INTERRUPT_MASK: u64 = (1 << 11)
    | (1 << 9)
    | (1 << 7)
    | (1 << 5)
    | (1 << 3)
    | (1 << 1)
    | (1 << 8)
    | (1 << 4)
    | (1 << 0);
const HPM_MASK: u64 = 0xFFFF_FFFF;
const SUPERVISOR_INTERRUPT_MASK: u64 = (1 << 9) | (1 << 5) | (1 << 1);
const SSTATUS_VISIBLE_MASK: u64 =
    (1 << 1) | (1 << 5) | (1 << 6) | (1 << 8) | (0b11 << 13) | (0b11 << 15) | (0b11 << 32);
const MENV_CFG_MASK: u64 = 0xFFFF;
const HENVCFG_MASK: u64 = 0xFFFF;
const HEDELEG_MASK: u64 = MEDELEG_MASK;
const HIDELEG_MASK: u64 = MIDELEG_MASK;
const HYPERVISOR_INTERRUPT_MASK: u64 = INTERRUPT_MASK;
const HSTATUS_VISIBLE_MASK: u64 = (0b11 << 32) | (1 << 9) | (1 << 8) | (1 << 6) | (1 << 5);
const PMPADDR_MASK: u64 = (1u64 << 54) - 1;

fn random_masked<R: Rng + ?Sized>(rng: &mut R, mask: u64) -> u64 {
    rng.random::<u64>() & mask
}

fn pick<'a, R: Rng + ?Sized, T: Copy>(rng: &mut R, values: &'a [T]) -> T {
    *values.choose(rng).expect("non-empty value list")
}

fn random_mstatus<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let mut value = rng.random::<u64>() & SIMPLE_MSTATUS_MASK;
    // SPP (bit 8)
    value |= (rng.random::<bool>() as u64) << 8;
    // MPP (bits 12-11)
    let mpp = match rng.random_range(0..=2) {
        0 => 0b00,
        1 => 0b01,
        _ => 0b11,
    } as u64;
    value |= mpp << 11;
    // FS bits 14-13
    let fs = pick(rng, &[0b00u64, 0b01, 0b10, 0b11]);
    value |= fs << 13;
    // XS bits 16-15
    let xs = pick(rng, &[0b00u64, 0b01, 0b10, 0b11]);
    value |= xs << 15;
    // UXL bits 33-32
    let uxl = pick(rng, &[0b01u64, 0b10]);
    value |= uxl << 32;
    // SXL bits 35-34
    let sxl = pick(rng, &[0b01u64, 0b10]);
    value |= sxl << 34;
    value
}

fn random_misa<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    // Force RV64 base ISA (MXL = 2) and always include I.
    let mut value = 2u64 << 62;
    let isa_bit = |letter: char| -> u64 { 1u64 << (letter as u32 - 'A' as u32) };
    value |= isa_bit('I');
    // Optional extensions
    for &ext in &['M', 'A', 'F', 'D', 'C', 'V', 'S', 'U'] {
        if rng.random::<bool>() {
            value |= isa_bit(ext);
        }
    }
    value
}

fn random_mtvec<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let mode = rng.random_range(0..=1) as u64;
    let base = (rng.random_range(0..(1 << 18)) as u64) << 2; // 4-byte aligned
    base | mode
}

fn random_satp<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    const MODES: &[u64] = &[0, 8, 9, 10];
    let mode = pick(rng, MODES);
    let asid = rng.random::<u16>() as u64;
    let ppn = rng.random::<u64>() & ((1u64 << 44) - 1);
    (mode << 60) | (asid << 44) | ppn
}

fn random_vtype<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let vlmul = pick(rng, &[0b000u64, 0b001, 0b010, 0b011, 0b101, 0b110, 0b111]);
    let sew_log2 = pick(rng, &[0u64, 1, 2, 3, 4, 5, 6]);
    let vta = rng.random::<bool>() as u64;
    let vma = rng.random::<bool>() as u64;
    vlmul | (sew_log2 << 3) | (vta << 6) | (vma << 7)
}

fn random_vstart<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    const MAX_VSTART: u64 = 1 << 11; // assumes VLMAX up to 2048 elements
    rng.random_range(0..MAX_VSTART)
}

fn random_vxrm<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    pick(rng, &[0u64, 1, 2, 3])
}

fn random_vxsat<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    rng.random::<bool>() as u64
}

fn random_counter<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    rng.random::<u64>()
}

fn random_fflags<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    (rng.random::<u8>() as u64) & 0x1F
}

fn random_frm<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    pick(rng, &[0u64, 1, 2, 3, 4])
}

fn random_fcsr<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let frm = random_frm(rng);
    let fflags = random_fflags(rng);
    (frm << 5) | fflags
}

fn random_menvcfg<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    random_masked(rng, MENV_CFG_MASK)
}

fn random_henvcfg<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    random_masked(rng, HENVCFG_MASK)
}

fn random_pmpcfg_entry<R: Rng + ?Sized>(rng: &mut R) -> u8 {
    let r = rng.random::<bool>() as u8;
    let w = rng.random::<bool>() as u8;
    let x = rng.random::<bool>() as u8;
    let a = pick(rng, &[0b00u8, 0b01, 0b10, 0b11]);
    let l = rng.random::<bool>() as u8;
    r | (w << 1) | (x << 2) | ((a & 0b11) << 3) | (l << 7)
}

fn random_pmpcfg<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let mut value = 0u64;
    for idx in 0..8 {
        value |= (random_pmpcfg_entry(rng) as u64) << (idx * 8);
    }
    value
}

fn random_pmpaddr<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    rng.random::<u64>() & PMPADDR_MASK
}

fn random_hstatus<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let vsxl = pick(rng, &[0b01u64, 0b10]) << 32;
    let base = random_masked(rng, HSTATUS_VISIBLE_MASK & !(0b11 << 32));
    vsxl | base
}

fn random_hgatp<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let mode = pick(rng, &[0u64, 8, 9, 10]);
    let vmid = rng.random::<u64>() & 0x3FFF; // 14-bit VMID per spec
    let ppn = rng.random::<u64>() & ((1u64 << 44) - 1);
    (mode << 60) | (vmid << 44) | ppn
}

fn random_hvictl<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    random_masked(rng, 0xFFFF)
}

fn random_mnstatus<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    random_mstatus(rng)
}

fn random_epc<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let value = rng.random::<u64>();
    value & !0b11
}

fn random_cause<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let interrupt = rng.random::<bool>() as u64;
    let code = rng.random_range(0..=31u8) as u64;
    (interrupt << 63) | code
}

fn random_sstatus<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    random_mstatus(rng) & SSTATUS_VISIBLE_MASK
}

fn random_dcsr<R: Rng + ?Sized>(rng: &mut R) -> u64 {
    let mut value = 0u64;
    // xdebugver = 1.0 (4)
    value |= 4u64 << 28;
    // cause field bits 8:6
    let cause = pick(rng, &[1u64, 2, 3, 4, 5, 7]);
    value |= cause << 6;
    // step/stop bits
    if rng.random::<bool>() {
        value |= 1 << 2; // step
    }
    if rng.random::<bool>() {
        value |= 1 << 3; // nmip latch (writable alias)
    }
    if rng.random::<bool>() {
        value |= 1 << 4; // mprven
    }
    if rng.random::<bool>() {
        value |= 1 << 9; // stoptime
    }
    if rng.random::<bool>() {
        value |= 1 << 10; // stopcount
    }
    if rng.random::<bool>() {
        value |= 1 << 11; // stepie
    }
    if rng.random::<bool>() {
        value |= 1 << 12; // ebreaku
    }
    if rng.random::<bool>() {
        value |= 1 << 13; // ebreaks
    }
    if rng.random::<bool>() {
        value |= 1 << 15; // ebreakm
    }
    if rng.random::<bool>() {
        value |= 1 << 16; // ebreakvu
    }
    if rng.random::<bool>() {
        value |= 1 << 17; // ebreakvs
    }
    if rng.random::<bool>() {
        value |= 1 << 19; // cetrig
    }
    // extcause bits (26:24)
    let extcause = rng.random_range(0..=4) as u64;
    value |= extcause << 24;
    value
}

fn fmt_csr_lowercase<T: fmt::Debug>(value: &T, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut name = format!("{:?}", value);
    name.make_ascii_lowercase();
    f.write_str(&name)
}
