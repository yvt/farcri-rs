//! FarCri.rs: Criterion.rs on Remote Target
#![cfg_attr(feature = "role_target", no_std)]
#![cfg_attr(not(feature = "role_target"), crate_type = "dylib")]
#![deny(unsafe_op_in_unsafe_fn)]

// -------------------------------------------------------------------------
// Driver and Proxy modes

#[cfg(not(feature = "role_target"))]
mod cargo;
#[cfg(not(any(feature = "role_proxy", feature = "role_target")))]
mod driver;
pub mod macros;
#[cfg(feature = "role_proxy")]
mod proxy;

#[cfg(not(any(feature = "role_proxy", feature = "role_target")))]
pub use self::driver::main;
#[cfg(feature = "role_proxy")]
pub use self::proxy::main;

// -------------------------------------------------------------------------
// Target mode

#[cfg(feature = "cortex-m-rt")]
#[doc(hidden)]
pub extern crate cortex_m_rt;

mod target;

#[cfg(feature = "role_target")]
pub use self::target::main;

// -------------------------------------------------------------------------

mod bencher;
pub use self::bencher::{black_box, time, Bencher, BenchmarkGroup, Criterion, Throughput};

mod utils {
    mod fmt;
    mod strs;
    pub use self::fmt::*;
    pub use self::strs::*;

    #[cfg(not(feature = "role_target"))]
    mod stdserde;
    #[cfg(not(feature = "role_target"))]
    pub use self::stdserde::*;

    #[cfg(feature = "role_proxy")]
    mod futures;
    #[cfg(feature = "role_proxy")]
    pub use self::futures::*;
}
