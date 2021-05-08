use core::borrow::Borrow;
use core::fmt;
use serde::{Deserialize, Serialize};

pub use super::time::{Duration, Instant};

pub(crate) const HANDSHAKE_MAGIC: &[u8] = b"\x01fluttershyyay";
pub(crate) const HANDSHAKE_NONCE_LEN: usize = 16;

pub(crate) const HANDSHAKE_END_MAGIC: &[u8] = b"\x02applejack";

/// A message sent from the Proxy program to the Target program.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "role_proxy", derive(Serialize))]
pub(crate) enum DownstreamMessage<Str> {
    Greeting {
        /// A dummy message to use the `Str` generic parameter
        _unused: Str,
        mode: Mode,
    },
    /// Terminate the Target program's listening loop and causes it to proceed
    /// to the next task.
    Continue,
    /// Response to [`UpstreamMessage::GetInstant`].
    Instant(Instant),
}

#[derive(Debug, Deserialize, Copy, Clone)]
#[cfg_attr(feature = "role_proxy", derive(Serialize))]
/// Enum representing the execution mode.
pub(crate) enum Mode {
    /// Run benchmarks normally.
    Benchmark,
    /// Run benchmarks once to verify that they work, but otherwise do not measure them.
    Test,
}

/// A message sent from the Target program to the Proxy program. This is sort
/// of a slimmed-down verison of `IncomingMessage` from `cargo-criteion`.
/// Backwards compatbiility is not important because we are sure that both sides
/// will use exactly the same vesrion of `farcri`.
///
/// `Str` can be `String` or `&str`.
///
/// `Values` can be `Vec<u64>` or `&[u64]`.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "role_proxy", derive(Deserialize))]
pub(crate) enum UpstreamMessage<Str, Values> {
    BeginningBenchmarkGroup {
        group: Str,
    },
    FinishedBenchmarkGroup,
    BeginningBenchmark {
        id: RawBenchmarkId<Str>,
    },
    SkippingBenchmark {
        id: RawBenchmarkId<Str>,
    },
    Warmup {
        warm_up_goal_duration: Duration,
    },
    MeasurementStart {
        warm_up_iter_count: u64,
        warm_up_duration: Duration,
        num_samples: usize,
        num_iters: u64,
    },
    MeasurementComplete {
        num_iters_per_sample: u64,
        values: Values,
        benchmark_config: BenchmarkConfig,
        // sampling_method: always `Flat`
    },

    /// Indicates there are no more benchmark tests remaining. Not in
    /// `IncomingMessage`.
    End,

    /// Queries the current time Not in `IncomingMessage`.
    GetInstant,
}

#[derive(Debug, Serialize, Copy, Clone)]
#[cfg_attr(feature = "role_proxy", derive(Deserialize))]
pub(crate) struct RawBenchmarkId<Str> {
    pub(super) group_id: Str,
    pub(super) function_id: Option<Str>,
    pub(super) value_str: Option<Str>,
    pub(super) throughput: Option<Throughput>,
}

impl<Str: Borrow<str>> fmt::Display for RawBenchmarkId<Str> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            Some(&self.group_id)
                .map(Str::borrow)
                .filter(|x| !x.is_empty()),
            self.function_id
                .as_ref()
                .map(Str::borrow)
                .filter(|x| !x.is_empty()),
            self.value_str
                .as_ref()
                .map(Str::borrow)
                .filter(|x| !x.is_empty()),
        ];

        for (i, part) in parts.iter().filter_map(|&x| x).enumerate() {
            if i != 0 {
                f.write_str("/")?;
            }
            f.write_str(part)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "role_proxy", derive(Deserialize))]
pub(crate) enum Throughput {
    Bytes(u64),
    Elements(u64),
}

impl From<super::Throughput> for Throughput {
    #[inline]
    fn from(x: super::Throughput) -> Self {
        match x {
            crate::Throughput::Bytes(x) => Self::Bytes(x),
            crate::Throughput::Elements(x) => Self::Elements(x),
        }
    }
}

#[derive(Debug, Serialize, Copy, Clone)]
#[cfg_attr(feature = "role_proxy", derive(Deserialize))]
pub struct BenchmarkConfig {
    // confidence_level: f64,
    pub measurement_time: Duration,
    // noise_threshold: f64,
    pub nresamples: usize,
    pub sample_size: usize,
    // significance_level: f64,
    pub warm_up_time: Duration,
}

impl Default for BenchmarkConfig {
    #[inline]
    fn default() -> Self {
        Self {
            measurement_time: Duration::from_nanos(5_000_000_000),
            nresamples: 100_000,
            sample_size: 50,
            warm_up_time: Duration::from_nanos(3_000_000_000),
        }
    }
}
