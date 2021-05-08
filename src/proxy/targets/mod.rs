use anyhow::Result;
use std::{ffi::OsString, fmt, future::Future, pin::Pin};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::cargo::CompiledExecutable;

// TODO: Some things in this module were copied from `r3_test_runner`.
//       Find a way to deduplicate

// mod demux;
mod ldscript;
mod probe_rs;

pub trait Target: Send + Sync + fmt::Debug {
    /// Get the target architecture.
    fn target_arch(&self) -> Arch;

    fn prepare_build(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn BuildSetup>>>>>;

    /// Get the additional Cargo features of `farcri` to enable when building
    /// the target in Target mode
    fn cargo_features(&self) -> &[&str];

    /// Connect to the target.
    fn connect(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn DebugProbe>>>>>;
}

/// Represents a temporary setup on the host computer for compilation, such as a
/// temporary directory containing a generated linker script.
pub trait BuildSetup: Send {
    /// Environment variables to set when building the target.
    fn build_envs(&self) -> Vec<(OsString, OsString)> {
        Vec::new()
    }

    /// Additional flags to pass to `rustc`.
    fn rustc_flags(&self) -> Vec<String> {
        Vec::new()
    }
}

impl BuildSetup for () {}

pub trait DebugProbe: Send {
    /// Program the specified ELF image and start its execution on the target.
    ///
    /// Returns the target's input and output stream. It's allowed for the
    /// stream to fail to deliver some of the first bytes sent or received.
    /// The stream is also allowed to return stray bytes from a previous run.
    /// However, the target must not read stray bytes from a previous run.
    ///
    /// All reads and write operations on the returned stream must not be
    /// cancelled, i.e., waking up the waker passed to `poll_read` or
    /// `poll_write` should cause the same polling method to be called again.
    /// When this is violated, the stream may stop working correctly. (Usually,
    /// this is violated when a `Read` (`Write`) future returned by
    /// `AsyncReadExt::read` (`AsyncWriteExt::write`) is dropped before
    /// finishing.)
    fn program_and_get_output(
        &mut self,
        exe: &CompiledExecutable,
    ) -> Pin<Box<dyn Future<Output = Result<DynAsyncReadWrite<'_>>> + '_>>;
}

type DynAsyncReadWrite<'a> = Pin<Box<dyn AsyncReadWrite + 'a>>;

pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite + ?Sized> AsyncReadWrite for T {}

pub static TARGETS: &[(&str, &dyn Target)] = &[("nucleo_f401re", &probe_rs::NucleoF401re)];

#[derive(Debug)]
struct OverrideTargetArch<T>(Arch, T);

impl<T: Target> Target for OverrideTargetArch<T> {
    fn target_arch(&self) -> Arch {
        self.0
    }

    fn prepare_build(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn BuildSetup>>>>> {
        self.1.prepare_build()
    }

    fn cargo_features(&self) -> &[&str] {
        self.1.cargo_features()
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn DebugProbe>>>>> {
        self.1.connect()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Arch {
    /// Armv7-A
    Armv7A,
    /// Arm M-Profile
    ArmM {
        /// Specifies the architecture version to use.
        version: ArmMVersion,
        /// The Floating-point extension.
        fpu: bool,
        /// The DSP extension.
        dsp: bool,
    },
    Riscv {
        /// XLEN
        xlen: Xlen,
        /// Use the reduced register set
        e: bool,
        /// The "M" extension (multiplication and division)
        m: bool,
        /// The "A" extension (atomics)
        a: bool,
        /// The "C" extension (compressed instructions)
        c: bool,
        /// The "F" extension (single-precision floating point numbers)
        f: bool,
        /// The "D" extension (double-precision floating point numbers)
        d: bool,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ArmMVersion {
    Armv6M,
    Armv7M,
    Armv8MBaseline,
    Armv8MMainline,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Xlen {
    _32 = 32,
    _64 = 64,
}

/// A set of build options passed to `rustc` to build an application for some
/// target specified by [`Arch`].
#[derive(Debug, Default)]
pub struct BuildOpt {
    pub target_triple: &'static str,
    pub target_features: String,
}

impl Arch {
    const NAMED_ARCHS: &'static [(&'static str, Self)] = &[
        ("cortex_a9", Self::CORTEX_A9),
        ("cortex_m0", Self::CORTEX_M0),
        ("cortex_m3", Self::CORTEX_M3),
        ("cortex_m4", Self::CORTEX_M4),
        ("cortex_m4f", Self::CORTEX_M4F),
        ("cortex_m23", Self::CORTEX_M23),
        ("cortex_m33", Self::CORTEX_M33),
        (
            "rv32i",
            Self::Riscv {
                xlen: Xlen::_32,
                e: false,
                m: false,
                a: false,
                c: false,
                f: false,
                d: false,
            },
        ),
        (
            "rv64i",
            Self::Riscv {
                xlen: Xlen::_64,
                e: false,
                m: false,
                a: false,
                c: false,
                f: false,
                d: false,
            },
        ),
        (
            "rv32e",
            Self::Riscv {
                xlen: Xlen::_32,
                e: true,
                m: false,
                a: false,
                c: false,
                f: false,
                d: false,
            },
        ),
    ];

    const CORTEX_A9: Self = Self::Armv7A;

    const CORTEX_M0: Self = Self::ArmM {
        version: ArmMVersion::Armv6M,
        fpu: false,
        dsp: false,
    };
    const CORTEX_M3: Self = Self::ArmM {
        version: ArmMVersion::Armv7M,
        fpu: false,
        dsp: false,
    };
    const CORTEX_M4: Self = Self::ArmM {
        version: ArmMVersion::Armv7M,
        fpu: false,
        dsp: true,
    };
    const CORTEX_M4F: Self = Self::ArmM {
        version: ArmMVersion::Armv7M,
        fpu: true,
        dsp: true,
    };
    const CORTEX_M23: Self = Self::ArmM {
        version: ArmMVersion::Armv8MBaseline,
        fpu: false,
        dsp: false,
    };
    const CORTEX_M33: Self = Self::ArmM {
        version: ArmMVersion::Armv8MMainline,
        fpu: false,
        dsp: false,
    };
    const CORTEX_M33_FPU: Self = Self::ArmM {
        version: ArmMVersion::Armv8MMainline,
        fpu: true,
        dsp: false,
    };

    const RV32IMAC: Self = Self::Riscv {
        xlen: Xlen::_32,
        e: false,
        m: true,
        a: true,
        c: true,
        f: false,
        d: false,
    };

    const RV64IMAC: Self = Self::Riscv {
        xlen: Xlen::_64,
        e: false,
        m: true,
        a: true,
        c: true,
        f: false,
        d: false,
    };

    const RV32GC: Self = Self::Riscv {
        xlen: Xlen::_32,
        e: false,
        m: true,
        a: true,
        c: true,
        f: true,
        d: true,
    };

    const RV64GC: Self = Self::Riscv {
        xlen: Xlen::_64,
        e: false,
        m: true,
        a: true,
        c: true,
        f: true,
        d: true,
    };

    pub fn build_opt(&self) -> Option<BuildOpt> {
        match self {
            // Arm A-Profile
            // -------------------------------------------------------------
            Self::Armv7A => Some(BuildOpt::from_target_triple("armv7a-none-eabi")),

            // Arm M-Profile
            // -------------------------------------------------------------
            Self::ArmM {
                version: ArmMVersion::Armv6M,
                fpu: false,
                dsp: false,
            } => Some(BuildOpt::from_target_triple("thumbv6m-none-eabi")),

            Self::ArmM {
                version: ArmMVersion::Armv6M,
                fpu: _,
                dsp: _,
            } => None,

            Self::ArmM {
                version: ArmMVersion::Armv7M,
                fpu: false,
                dsp: false,
            } => Some(BuildOpt::from_target_triple("thumbv7m-none-eabi")),

            Self::ArmM {
                version: ArmMVersion::Armv7M,
                fpu: false,
                dsp: true,
            } => Some(BuildOpt::from_target_triple("thumbv7em-none-eabi")),

            Self::ArmM {
                version: ArmMVersion::Armv7M,
                fpu: true,
                dsp: true,
            } => Some(BuildOpt::from_target_triple("thumbv7em-none-eabihf")),

            Self::ArmM {
                version: ArmMVersion::Armv7M,
                fpu: true,
                dsp: false,
            } => None,

            Self::ArmM {
                version: ArmMVersion::Armv8MBaseline,
                fpu: false,
                dsp: false,
            } => Some(BuildOpt::from_target_triple("thumbv8m.base-none-eabi")),

            Self::ArmM {
                version: ArmMVersion::Armv8MMainline,
                fpu: false,
                dsp: false,
            } => Some(BuildOpt::from_target_triple("thumbv8m.main-none-eabi")),

            Self::ArmM {
                version: ArmMVersion::Armv8MMainline,
                fpu: true,
                dsp: false,
            } => Some(BuildOpt::from_target_triple("thumbv8m.main-none-eabihf")),

            Self::ArmM {
                version: ArmMVersion::Armv8MBaseline | ArmMVersion::Armv8MMainline,
                fpu: _,
                dsp: _,
            } => None,

            // RISC-V
            // -------------------------------------------------------------
            Self::Riscv {
                xlen: Xlen::_32,
                e: false,
                m: false,
                a: false,
                c: false,
                f: false,
                d: false,
            } => Some(BuildOpt::from_target_triple("riscv32i-unknown-none-elf")),

            Self::Riscv {
                xlen: Xlen::_32,
                e: false,
                m: true,
                a: false,
                c: true,
                f: false,
                d: false,
            } => Some(BuildOpt::from_target_triple("riscv32imc-unknown-none-elf")),

            Self::Riscv {
                xlen: Xlen::_32,
                e: false,
                m: true,
                a: true,
                c: true,
                f: false,
                d: false,
            } => Some(BuildOpt::from_target_triple("riscv32imac-unknown-none-elf")),

            Self::Riscv {
                xlen: Xlen::_64,
                e: false,
                m: true,
                a: true,
                c: true,
                f: false,
                d: false,
            } => Some(BuildOpt::from_target_triple("riscv64imac-unknown-none-elf")),

            Self::Riscv {
                xlen: Xlen::_64,
                e: false,
                m: true,
                a: true,
                c: true,
                f: true,
                d: true,
            } => Some(BuildOpt::from_target_triple("riscv64gc-unknown-none-elf")),

            Self::Riscv {
                xlen,
                e,
                m,
                a,
                c,
                f,
                d,
            } => Some(
                BuildOpt::from_target_triple(match xlen {
                    Xlen::_32 => "riscv32imac-unknown-none-elf",
                    Xlen::_64 => "riscv64imac-unknown-none-elf",
                })
                .with_target_features(&[
                    if *e { Some("+e") } else { None },
                    if *m { None } else { Some("-m") },
                    if *a { None } else { Some("-a") },
                    if *c { None } else { Some("-c") },
                    if *f { Some("+f") } else { None },
                    if *d { Some("+d") } else { None },
                ]),
            ),
        }
    }

    fn with_feature_by_name(self, name: &str, enable: bool) -> Option<Self> {
        macro_rules! features {
            (
                Self::$variant:ident {
                    // Allow these features to be modified
                    $($feat:ident),*;
                    // These fields are left untouched
                    $($extra:ident),*
                }
            ) => {{
                $( let mut $feat = $feat; )*
                match name {
                    $(
                        stringify!($feat) => $feat = enable,
                    )*
                    _ => return None,
                }
                Some(Self::$variant { $($feat,)* $($extra,)* })
            }};
        }
        match self {
            Self::Armv7A => None,
            Self::ArmM { fpu, dsp, version } => features!(Self::ArmM { fpu, dsp; version }),
            Self::Riscv {
                e,
                m,
                a,
                c,
                f,
                d,
                xlen,
            } => features!(Self::Riscv { e, m, a, c, f, d; xlen }),
        }
    }
}

impl BuildOpt {
    fn from_target_triple(target_triple: &'static str) -> Self {
        Self {
            target_triple,
            ..Default::default()
        }
    }

    fn with_target_features(self, seq: &[Option<&'static str>]) -> Self {
        Self {
            target_features: crate::utils::CommaSeparatedNoSpace(seq.iter().filter_map(|x| *x))
                .to_string(),
            ..self
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, fm: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Armv7A => write!(fm, "cortex_a9"),
            Self::ArmM {
                mut fpu,
                mut dsp,
                version,
            } => {
                match (version, fpu, dsp) {
                    (ArmMVersion::Armv6M, _, _) => write!(fm, "cortex_m0")?,
                    (ArmMVersion::Armv7M, true, true) => {
                        write!(fm, "cortex_m4f")?;
                        fpu = false;
                        dsp = false;
                    }
                    (ArmMVersion::Armv7M, false, true) => {
                        write!(fm, "cortex_m4")?;
                        dsp = false;
                    }
                    (ArmMVersion::Armv7M, _, _) => write!(fm, "cortex_m3")?,
                    (ArmMVersion::Armv8MBaseline, _, _) => write!(fm, "cortex_m23")?,
                    (ArmMVersion::Armv8MMainline, _, _) => write!(fm, "cortex_m33")?,
                }
                if fpu {
                    write!(fm, "+fpu")?;
                }
                if dsp {
                    write!(fm, "+dsp")?;
                }
                Ok(())
            }
            Self::Riscv {
                e,
                m,
                a,
                c,
                f,
                d,
                xlen,
            } => {
                if *e {
                    write!(fm, "rv{}e", *xlen as u8)?;
                } else {
                    write!(fm, "rv{}i", *xlen as u8)?;
                }
                if *m {
                    write!(fm, "+m")?;
                }
                if *a {
                    write!(fm, "+a")?;
                }
                if *c {
                    write!(fm, "+c")?;
                }
                if *f {
                    write!(fm, "+f")?;
                }
                if *d {
                    write!(fm, "+d")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ArchParseError {
    #[error("Unknown base architecture: '{0}'")]
    UnknownBase(String),
    #[error("Unknown feature: '{0}'")]
    UnknownFeature(String),
}

impl std::str::FromStr for Arch {
    type Err = ArchParseError;

    /// Parse a target architecture string.
    ///
    /// A target architecture string should be specified in the following form:
    /// `base+feat1-feat2`
    ///
    ///  - `base` chooses a named architecture from `NAMED_ARCHS`.
    ///  - `+feat1` enables the feature `feat1`.
    ///  - `-feat2` disables the feature `feat2`.
    ///
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut i = s.find(&['-', '+'][..]).unwrap_or_else(|| s.len());
        let base = &s[0..i];
        let mut arch = Self::NAMED_ARCHS
            .iter()
            .find(|x| x.0 == base)
            .ok_or_else(|| ArchParseError::UnknownBase(base.to_owned()))?
            .1;

        while i < s.len() {
            let add = match s.as_bytes()[i] {
                b'+' => true,
                b'-' => false,
                _ => unreachable!(),
            };
            i += 1;

            // Find the next `-` or `+`
            let k = s[i..]
                .find(&['-', '+'][..])
                .map(|k| k + i)
                .unwrap_or_else(|| s.len());

            let feature = &s[i..k];

            arch = arch
                .with_feature_by_name(feature, add)
                .ok_or_else(|| ArchParseError::UnknownFeature(feature.to_owned()))?;

            i = k;
        }

        Ok(arch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_round_trip() {
        for (_, arch) in Arch::NAMED_ARCHS {
            let arch_str = arch.to_string();
            let arch2: Arch = arch_str.parse().unwrap();
            assert_eq!(*arch, arch2);
        }
    }
}
