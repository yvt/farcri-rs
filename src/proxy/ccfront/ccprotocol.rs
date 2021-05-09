//! cargo-criterion protocol
use serde::{Deserialize, Serialize};
use std::mem::size_of;

pub(crate) const RUNNER_MAGIC_NUMBER: &str = "cargo-criterion";
pub(crate) const RUNNER_HELLO_SIZE: usize = 15 //RUNNER_MAGIC_NUMBER.len() // magic number
    + (size_of::<u8>() * 3); // version number

pub(crate) const BENCHMARK_MAGIC_NUMBER: &str = "Criterion";
pub(crate) const BENCHMARK_HELLO_SIZE: usize = 9 //BENCHMARK_MAGIC_NUMBER.len() // magic number
    + (size_of::<u8>() * 3) // version number
    + size_of::<u16>() // protocol version
    + size_of::<u16>(); // protocol format
pub(crate) const PROTOCOL_VERSION: u16 = 1;
pub(crate) const PROTOCOL_FORMAT: u16 = 1;

/// Enum defining the messages we can receive
#[derive(Debug, Deserialize)]
pub(crate) enum IncomingMessage {
    // Value formatter requests
    FormatValue {
        value: f64,
    },
    FormatThroughput {
        value: f64,
        throughput: Throughput,
    },
    ScaleValues {
        typical_value: f64,
        values: Vec<f64>,
    },
    ScaleThroughputs {
        typical_value: f64,
        values: Vec<f64>,
        throughput: Throughput,
    },
    ScaleForMachines {
        values: Vec<f64>,
    },
    Continue,

    __Other,
}

/// Enum defining the messages we can send
#[derive(Debug, Serialize)]
pub(crate) enum OutgoingMessage<'a> {
    BeginningBenchmarkGroup {
        group: &'a str,
    },
    FinishedBenchmarkGroup {
        group: &'a str,
    },
    BeginningBenchmark {
        id: RawBenchmarkId,
    },
    SkippingBenchmark {
        id: RawBenchmarkId,
    },
    Warmup {
        id: RawBenchmarkId,
        nanos: f64,
    },
    MeasurementStart {
        id: RawBenchmarkId,
        sample_count: u64,
        estimate_ns: f64,
        iter_count: u64,
    },
    MeasurementComplete {
        id: RawBenchmarkId,
        iters: &'a [f64],
        times: &'a [f64],
        plot_config: PlotConfiguration,
        sampling_method: SamplingMethod,
        benchmark_config: BenchmarkConfig,
    },
    // value formatter responses
    FormattedValue {
        value: String,
    },
    ScaledValues {
        scaled_values: Vec<f64>,
        unit: &'a str,
    },
}

// Also define serializable variants of certain things, either to avoid leaking
// serializability into the public interface or because the serialized form
// is a bit different from the regular one.

#[derive(Debug, Serialize, Clone)]
pub(crate) struct RawBenchmarkId {
    group_id: String,
    function_id: Option<String>,
    value_str: Option<String>,
    throughput: Vec<Throughput>,
}
impl<Str> From<&crate::bencher::protocol::RawBenchmarkId<Str>> for RawBenchmarkId
where
    for<'a> &'a Str: Into<String>,
{
    fn from(other: &crate::bencher::protocol::RawBenchmarkId<Str>) -> RawBenchmarkId {
        RawBenchmarkId {
            group_id: (&other.group_id).into(),
            function_id: other.function_id.as_ref().map(Into::into),
            value_str: other.value_str.as_ref().map(Into::into),
            throughput: other.throughput.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) enum AxisScale {
    Linear,
    Logarithmic,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlotConfiguration {
    pub summary_scale: AxisScale,
}

#[derive(Debug, Serialize)]
struct Duration {
    secs: u64,
    nanos: u32,
}
impl From<crate::bencher::time::Duration> for Duration {
    fn from(other: crate::bencher::time::Duration) -> Self {
        Duration {
            secs: other.as_nanos() / 1_000_000_000,
            nanos: (other.as_nanos() % 1_000_000_000) as u32,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct BenchmarkConfig {
    confidence_level: f64,
    measurement_time: Duration,
    noise_threshold: f64,
    nresamples: usize,
    sample_size: usize,
    significance_level: f64,
    warm_up_time: Duration,
}
impl From<&crate::bencher::protocol::BenchmarkConfig> for BenchmarkConfig {
    fn from(other: &crate::bencher::protocol::BenchmarkConfig) -> Self {
        BenchmarkConfig {
            confidence_level: 0.95,
            measurement_time: other.measurement_time.into(),
            noise_threshold: 0.01,
            nresamples: other.nresamples,
            sample_size: other.sample_size,
            significance_level: 0.05,
            warm_up_time: other.warm_up_time.into(),
        }
    }
}

/// Currently not used; defined for forwards compatibility with cargo-criterion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) enum SamplingMethod {
    Linear,
    Flat,
}

pub(crate) type Throughput = crate::bencher::protocol::Throughput;
