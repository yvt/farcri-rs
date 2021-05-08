use core::{fmt, ops};
use serde::{Deserialize, Serialize};

/// Represents a point of time, measured in nanoseconds.
#[derive(Default, Copy, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Instant(u64);

impl fmt::Debug for Instant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

/// Represents a duration, measured in nanoseconds.
#[derive(Default, Copy, Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(transparent)]
pub struct Duration(u64);

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (value, suffix) = if self.0 < 5_000 {
            (self.0, "ns")
        } else if self.0 < 5_000_000 {
            (self.0 / 1_000, "μs")
        } else if self.0 < 5_000_000_000 {
            (self.0 / 1_000_000, "ms")
        } else {
            (self.0 / 1_000_000_000, "s")
        };
        write!(f, "{}{}", value, suffix)
    }
}

impl fmt::Debug for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

impl ops::Add for Duration {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl ops::AddAssign for Duration {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl ops::Sub for Instant {
    type Output = Duration;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Duration(self.0 - rhs.0)
    }
}

impl Instant {
    #[inline]
    pub fn from_nanos(x: u64) -> Self {
        Self(x)
    }

    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }
}

impl Duration {
    #[inline]
    pub const fn from_nanos(x: u64) -> Self {
        Self(x)
    }

    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }
}
