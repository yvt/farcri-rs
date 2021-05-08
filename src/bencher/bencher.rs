use super::measurement;

/// Timer struct used to iterate a benchmarked function and measure the runtime.
///
/// This struct provides different timing loops as methods. Each timing loop provides a different
/// way to time a routine and each has advantages and disadvantages.
///
/// * If you want to do the iteration and measurement yourself (eg. passing the iteration count
///   to a separate process), use `iter_custom`.
/// * Otherwise, use `iter`.
pub struct Bencher<'link> {
    /// Have we iterated this benchmark?
    pub(super) iterated: bool,
    /// Number of times to iterate this benchmark
    pub(super) iters: u64,
    /// The measured value
    pub(super) value: u64,
    /// Reference to the measurement object
    pub(super) measurement: measurement::Measurement<'link>,
    /// How much time did it take to perform the iteration? Used for the warmup period.
    pub(super) elapsed_time: measurement::Duration,
    /// Specifies whether `elapsed_time` should be set.
    pub(super) wants_elapsed_time: bool,
}

impl Bencher<'_> {
    /// Times a `routine` by executing it many times and timing the total elapsed time.
    ///
    /// Prefer this timing loop when `routine` returns a value that doesn't have a destructor.
    ///
    /// # Timing model
    ///
    /// Note that the `Bencher` also times the time required to destroy the output of `routine()`.
    /// Therefore prefer this timing loop when the runtime of `mem::drop(O)` is negligible compared
    /// to the runtime of the `routine`.
    ///
    /// ```text
    /// elapsed = Instant::now + iters * (routine + mem::drop(O) + Range::next)
    /// ```
    ///
    /// # Example
    ///
    /// ```rust
    /// #[macro_use] extern crate criterion;
    ///
    /// use criterion::*;
    ///
    /// // The function to benchmark
    /// fn foo() {
    ///     // ...
    /// }
    ///
    /// fn bench(c: &mut Criterion) {
    ///     c.bench_function("iter", move |b| {
    ///         b.iter(|| foo())
    ///     });
    /// }
    ///
    /// criterion_group!(benches, bench);
    /// criterion_main!(benches);
    /// ```
    ///
    #[inline(never)]
    pub fn iter<O, R>(&mut self, mut routine: R)
    where
        R: FnMut() -> O,
    {
        self.iterated = true;
        let time_start = self.wants_elapsed_time.then(|| self.measurement.now());
        let start = self.measurement.value();
        for _ in 0..self.iters {
            black_box(routine());
        }
        self.value = self.measurement.value().wrapping_sub(start);
        if let Some(time_start) = time_start {
            self.elapsed_time = self.measurement.now() - time_start;
        }
    }

    /// Times a `routine` by executing it many times and relying on `routine` to measure its own execution time.
    ///
    /// Prefer this timing loop in cases where `routine` has to do its own measurements to
    /// get accurate timing information (for example in multi-threaded scenarios where you spawn
    /// and coordinate with multiple threads).
    ///
    /// # Timing model
    /// Custom, the timing model is whatever is returned as the Duration from `routine`.
    ///
    /// # Example
    /// ```rust
    /// #[macro_use] extern crate criterion;
    /// use criterion::*;
    /// use criterion::black_box;
    /// use std::time::Instant;
    ///
    /// fn foo() {
    ///     // ...
    /// }
    ///
    /// fn bench(c: &mut Criterion) {
    ///     c.bench_function("iter", move |b| {
    ///         b.iter_custom(|iters| {
    ///             let start = Instant::now();
    ///             for _i in 0..iters {
    ///                 black_box(foo());
    ///             }
    ///             start.elapsed()
    ///         })
    ///     });
    /// }
    ///
    /// criterion_group!(benches, bench);
    /// criterion_main!(benches);
    /// ```
    ///
    #[inline(never)]
    pub fn iter_custom<R>(&mut self, mut routine: R)
    where
        R: FnMut(u64) -> u64,
    {
        self.iterated = true;
        let time_start = self.measurement.now();
        self.value = routine(self.iters);
        self.elapsed_time = self.measurement.now() - time_start;
    }

    // Benchmarks must actually call one of the iter methods. This causes benchmarks to fail loudly
    // if they don't.
    pub(crate) fn assert_iterated(&mut self) {
        if !self.iterated {
            panic!("Benchmark function must call Bencher::iter or related method.");
        }
        self.iterated = false;
    }
}

pub fn black_box<T>(dummy: T) -> T {
    unsafe {
        let ret = core::ptr::read_volatile(&dummy);
        core::mem::forget(dummy);
        ret
    }
}
