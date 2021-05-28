//! Implements a Criterion-like API and the benchmark runner.
use arrayvec::{ArrayString, ArrayVec};
use cryo::{cryo, LocalLock};
use measurement::Measurement;
use tokenlock::TokenLock;

use crate::utils::utf8_str_prev;

mod analysis;
mod bencher;
mod bid;
mod func;
pub mod measurement;
pub(crate) mod protocol;
mod proxylink;
pub mod time;

pub use self::{
    bencher::{black_box, Bencher},
    bid::*,
};

type GroupNameBuf = ArrayString<128>;
type FunctionNameBuf = ArrayString<128>;
type ParameterDescriptionBuf = ArrayString<128>;
type ValueBuf = ArrayVec<u64, 128>;

struct WorkingArea {
    link_buffer: [u8; 1024],
    group_name: GroupNameBuf,
    function_name: FunctionNameBuf,
    parameter_description: ParameterDescriptionBuf,
    value_buf: ValueBuf,
}

struct WorkingAreaTag;
type WorkingAreaAccessToken = tokenlock::SingletonToken<WorkingAreaTag>;
type WorkingAreaAccessTokenId = tokenlock::SingletonTokenId<WorkingAreaTag>;

static WORKING_AREA: TokenLock<WorkingArea, WorkingAreaAccessTokenId> = TokenLock::new(
    WorkingAreaAccessTokenId::new(),
    WorkingArea {
        link_buffer: [0; 1024],
        group_name: ArrayString::new_const(),
        function_name: ArrayString::new_const(),
        parameter_description: ArrayString::new_const(),
        value_buf: ValueBuf::new_const(),
    },
);

/// Target-independent entry point to be called by [`crate::target::main`].
///
/// # Safety
///
/// This method must not be called more than once.
pub(crate) unsafe fn main(groups: impl FnOnce(&mut Criterion), io: &mut crate::target::BencherIo) {
    // Safety: This method is called only once, so we can have full ownership
    //         of the `WorkingArea`.
    let token = unsafe { &mut WorkingAreaAccessToken::new_unchecked() };
    let work = WORKING_AREA.write(token);

    let mut link = proxylink::ProxyLink::new(io, &mut work.link_buffer);

    let mode = match link.recv() {
        protocol::DownstreamMessage::Greeting { mode, _unused } => mode,
        other => {
            panic!("unexpected downstream message: {:?}", other);
        }
    };

    let mut cri = Criterion {
        link,
        mode,
        group_name: &mut work.group_name,
        function_name: &mut work.function_name,
        parameter_description: &mut work.parameter_description,
        value_buf: &mut work.value_buf,
    };

    // `groups` will call `Criterion::benchmark_group`
    groups(&mut cri);

    cri.link.send(&protocol::UpstreamMessage::End);
}

/// The benchmark manager
///
/// In FarCri.rs, `Criterion` is always provided by the benchmark harness and
/// cannot be created by external code. The lifetime parameter, which is
/// specific to the FarCri.rs version of the `Criterion` type, represents the
/// reference to the benchmark harness' data structures.
pub struct Criterion<'link> {
    link: proxylink::ProxyLink<'link>,
    mode: protocol::Mode,
    group_name: &'link mut GroupNameBuf,
    function_name: &'link mut FunctionNameBuf,
    parameter_description: &'link mut ParameterDescriptionBuf,
    value_buf: &'link mut ValueBuf,
}

impl<'link> Criterion<'link> {
    pub fn benchmark_group(&mut self, group_name: &str) -> BenchmarkGroup<'link, '_> {
        // Copy `group_name` to `self.group_name`. If it doesn't fit, copy
        // as many Unicode scalars as possible. (Ideally grapheme boundaries
        // should be used, but that's probably too much to handle for MCUs)
        // TODO: the broad objective overlaps with `fill_array_string_with_display`
        self.group_name.clear();
        let group_name = if group_name.len() > self.group_name.capacity() {
            let new_len = utf8_str_prev(group_name.as_bytes(), self.group_name.capacity());
            &group_name[..new_len]
        } else {
            group_name
        };
        self.group_name.push_str(group_name);

        self.link
            .send(&protocol::UpstreamMessage::BeginningBenchmarkGroup {
                group: self.group_name,
            });

        BenchmarkGroup {
            cri: self,
            throughput: None,
        }
    }

    pub fn bench_function(&mut self, id: &str, f: impl FnMut(&mut Bencher<'_>)) -> &mut Self {
        self.benchmark_group(id)
            .bench_function(BenchmarkId::no_function(), f);
        self
    }
}

/// Enum representing different ways of measuring the throughput of benchmarked code.
/// If the throughput setting is configured for a benchmark then the estimated throughput will
/// be reported as well as the time per iteration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Throughput {
    /// Measure throughput in terms of bytes/second. The value should be the number of bytes
    /// processed by one iteration of the benchmarked code. Typically, this would be the length of
    /// an input string or `&[u8]`.
    Bytes(u64),

    /// Measure throughput in terms of elements/second. The value should be the number of elements
    /// processed by one iteration of the benchmarked code. Typically, this would be the size of a
    /// collection, but could also be the number of lines of input text or the number of values to
    /// parse.
    Elements(u64),
}

pub struct BenchmarkGroup<'link, 'cri> {
    cri: &'cri mut Criterion<'link>,
    throughput: Option<Throughput>,
}

impl BenchmarkGroup<'_, '_> {
    /// Set the input size for this benchmark group. Used for reporting the
    /// throughput.
    pub fn throughput(&mut self, throughput: Throughput) -> &mut Self {
        self.throughput = Some(throughput);
        self
    }

    /// Benchmark the given parameterless function inside this benchmark group.
    pub fn bench_function(
        &mut self,
        id: impl AsBenchmarkId,
        mut f: impl FnMut(&mut Bencher<'_>),
    ) -> &mut Self {
        self.bench_function_inner(id.as_benchmark_id(), &mut f)
    }

    /// Benchmark the given parameterized function inside this benchmark group.
    pub fn bench_with_input<I: ?Sized>(
        &mut self,
        id: impl AsBenchmarkId,
        input: &I,
        mut f: impl FnMut(&mut Bencher<'_>, &I),
    ) -> &mut Self {
        self.bench_function(id, move |b| f(b, input))
    }

    fn bench_function_inner(
        &mut self,
        id: BenchmarkId<'_>,
        f: &mut dyn FnMut(&mut Bencher<'_>),
    ) -> &mut Self {
        let id = protocol::RawBenchmarkId {
            group_id: self.cri.group_name.as_str(),
            function_id: if let Some(x) = &id.function_name {
                fill_array_string_with_display(&mut self.cri.function_name, Some(x));
                Some(self.cri.function_name.as_str())
            } else {
                None
            },
            value_str: if let Some(x) = &id.parameter {
                fill_array_string_with_display(&mut self.cri.parameter_description, Some(x));
                Some(self.cri.parameter_description.as_str())
            } else {
                None
            },
            throughput: self.throughput.map(Into::into),
        };

        let mut func = func::Function::new(f);

        match self.cri.mode {
            protocol::Mode::Benchmark => {
                // TODO: send `SkippingBenchmark` if skipped
                self.cri
                    .link
                    .send(&protocol::UpstreamMessage::BeginningBenchmark { id });

                cryo!(let link: CryoMut<_, LocalLock> = &mut self.cri.link);
                analysis::common(
                    &id,
                    &mut func,
                    &protocol::BenchmarkConfig::default(),
                    &mut self.cri.value_buf,
                    Measurement::new(link.write()),
                );
            } // protocol::Mode::Benchmark

            protocol::Mode::Test => {
                cryo!(let link: CryoMut<_, LocalLock> = &mut self.cri.link);
                log::info!("Testing {}", id);
                func.bench(Measurement::new(link.write()), 1, &mut [Default::default()]);
                log::info!("... Success");
            } // protocol::Mode::Test
        } // match self.cri.mode

        // Wait for a `Continue` message
        log::debug!("Waiting for `Continue`...");
        match self.cri.link.recv() {
            protocol::DownstreamMessage::Continue => {}
            other => {
                panic!("unexpected downstream message: {:?}", other);
            }
        }

        self
    }

    /// Consume the benchmark group and generate the summary reports for the group.
    ///
    /// It is recommended to call this explicitly, but if you forget it will be called when the
    /// group is dropped.
    pub fn finish(self) {}
}

impl Drop for BenchmarkGroup<'_, '_> {
    fn drop(&mut self) {
        let cri = &mut *self.cri;
        cri.link
            .send(&protocol::UpstreamMessage::FinishedBenchmarkGroup);

        // Wait for a `Continue` message
        log::debug!("Waiting for `Continue`...");
        match cri.link.recv() {
            protocol::DownstreamMessage::Continue => {}
            other => {
                panic!("unexpected downstream message: {:?}", other);
            }
        }
    }
}

// TODO: Implement a better way to be dynamic over `N`. Const generics is nice
//       but doesn't support unsizing (yet?).
fn fill_array_string_with_display<const N: usize>(
    buf: &mut ArrayString<N>,
    display: Option<&dyn core::fmt::Display>,
) {
    buf.clear();
    if let Some(display) = display {
        // Should there be an error, it's probably a capacity error
        let _ = write!(buf as &mut dyn core::fmt::Write, "{}", display);
    }
}
