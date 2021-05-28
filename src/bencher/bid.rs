use core::fmt;

#[derive(Clone, Copy)]
pub struct BenchmarkId<'a> {
    pub(crate) function_name: Option<&'a dyn fmt::Display>,
    pub(crate) parameter: Option<&'a dyn fmt::Display>,
}

impl<'a> BenchmarkId<'a> {
    /// Construct a new benchmark ID from a string function name and a parameter value.
    #[inline]
    pub fn new(function_name: &'a dyn fmt::Display, parameter: &'a dyn fmt::Display) -> Self {
        BenchmarkId {
            function_name: Some(function_name),
            parameter: Some(parameter),
        }
    }

    pub(crate) fn no_function() -> Self {
        Self {
            function_name: None,
            parameter: None,
        }
    }

    /// Construct a new benchmark ID from just a parameter value. Use this when benchmarking a
    /// single function with a variety of different inputs.
    #[inline]
    pub fn from_parameter(parameter: &'a dyn fmt::Display) -> BenchmarkId {
        BenchmarkId {
            function_name: None,
            parameter: Some(parameter),
        }
    }
}

impl fmt::Debug for BenchmarkId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct DisplayToDebug<'a>(&'a dyn fmt::Display);
        impl fmt::Debug for DisplayToDebug<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        f.debug_struct("BenchmarkId")
            .field("function_name", &self.function_name.map(DisplayToDebug))
            .field("parameter", &self.parameter.map(DisplayToDebug))
            .finish()
    }
}

/// Sealed trait which allows users to automatically convert strings to benchmark IDs.
pub trait AsBenchmarkId: private::Sealed {
    fn as_benchmark_id(&self) -> BenchmarkId<'_>;
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::BenchmarkId<'_> {}
    impl<S: core::fmt::Display> Sealed for S {}
}

impl AsBenchmarkId for BenchmarkId<'_> {
    fn as_benchmark_id(&self) -> BenchmarkId<'_> {
        *self
    }
}

impl<S: fmt::Display> AsBenchmarkId for S {
    fn as_benchmark_id(&self) -> BenchmarkId<'_> {
        BenchmarkId {
            function_name: Some(self),
            parameter: None,
        }
    }
}
