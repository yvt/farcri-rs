/// The entry point for Target mode.
///
/// Actually, this module is built in all modes. This is because `crate::
/// bencher` depends on the hardware abstraction provided by this module, and
/// user benchmark crates need the API provided by `crate::bencher` to
/// successfully compile. Non-Target modes don't specfiy any target-specifying
/// Cargo features, so we must be prepared to handle such cases even when
/// this module isn't actually used at runtime.
///
/// > **Rationale:** It's possible to remove this redundant dependency by using
/// > `#[cfg(...)]` in `crate::bencher`. However, this approach would introduce
/// > a larger amount of noise to the code and hence more maintenance burdens.
///

// --------------------------------------------------------------------------

// Panic handler
// TODO: catch panics in the proxy
#[cfg(feature = "panic-rtt-target")]
use panic_rtt_target as _;

// -------------------------------------------------------------------------

// `cortex-m-rt` interrupt handlers
#[cfg(feature = "stm32f4xx-hal")]
use stm32f4xx_hal as _;

// --------------------------------------------------------------------------

#[cfg(feature = "rtt-target")]
mod logger_rtt;
#[cfg(feature = "rtt-target")]
use self::logger_rtt::Comm;

// --------------------------------------------------------------------------

// Temporal quantification
#[cfg(feature = "cortex-m-rt")]
mod cortex_m_time;

#[cfg(feature = "target_std")]
mod std_time;

// --------------------------------------------------------------------------

// Suppress the "dead code" warning in non-Target mode
#[cfg(not(feature = "role_target"))]
#[used]
static _UNUSED: fn() = || main(|_| {});

pub fn main(groups: impl FnOnce(&mut crate::bencher::Criterion)) -> ! {
    #[cfg(feature = "cortex-m-rt")]
    {
        let p = cortex_m::Peripherals::take().unwrap();
        cortex_m_time::init(p.SYST);
    }

    #[cfg(feature = "rtt-target")]
    let comm = Comm::new();

    // Safety: We call this function only once throught the program's lifetime
    unsafe {
        crate::bencher::main(
            groups,
            &mut BencherIo {
                #[cfg(feature = "rtt-target")]
                comm,
            },
        );
    }

    loop {
        core::hint::spin_loop();
    }
}

/// Stores state variables maintained by this module and provides methods to be
/// called by `crate::bencher`.
pub(crate) struct BencherIo {
    #[cfg(feature = "rtt-target")]
    comm: Comm,
}

impl BencherIo {
    pub fn write(&mut self, b: &[u8]) {
        let _ = b;
        match () {
            #[cfg(feature = "rtt-target")]
            () => self.comm.write(b),
            #[cfg(not(feature = "rtt-target"))]
            () => unimplemented!(),
        }
    }

    /// Read bytes from the host, blocking the execution until at least one byte
    /// is read.
    pub fn read(&mut self, b: &mut [u8]) -> usize {
        let _ = b;
        match () {
            #[cfg(feature = "rtt-target")]
            () => self.comm.read(b),
            #[cfg(not(feature = "rtt-target"))]
            () => unimplemented!(),
        }
    }

    #[inline(never)]
    pub fn now(&mut self) -> u64 {
        match () {
            #[cfg(feature = "cortex-m-rt")]
            () => cortex_m_time::now(),
            #[cfg(feature = "target_std")]
            () => std_time::now(),
            #[allow(unreachable_patterns)]
            _ => unimplemented!(),
        }
    }
}
