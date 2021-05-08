//! Temporal quantifier for `std`
//!
//! In this port, `now` returns the current time in nanoseconds because that's
//! `std::time` gives.
use std::time::Instant;

// `Instant` doesn't let us *just* get the raw value
lazy_static::lazy_static! {
    static ref ORIGIN: Instant = Instant::now();
}

pub fn now() -> u64 {
    let origin = *ORIGIN;
    Instant::now().duration_since(origin).as_nanos() as u64
}
