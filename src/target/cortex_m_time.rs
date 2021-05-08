//! Temporal quantifier for Cortex-M devices
//!
//! This port uses SysTick for measurement. Unfortunately it's only a 24-bit
//! timer, so there will be some measurement errors roughly proportional to
//! the measured durations.
use core::sync::atomic::{AtomicUsize, Ordering};
use cortex_m::peripheral::{syst, SYST};

static CYCLE: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn init(mut syst: SYST) {
    syst.enable_interrupt();
    syst.set_reload(0xffffff);
    syst.set_clock_source(syst::SystClkSource::Core);
    syst.clear_current();
    syst.enable_counter();
}

#[cortex_m_rt::exception]
fn SysTick() {
    // note: Armv6-M doesn't support `fetch_add`
    CYCLE.store(
        CYCLE.load(Ordering::Relaxed).wrapping_add(1),
        Ordering::Relaxed,
    );
}

#[inline]
pub fn now() -> u64 {
    // Can't handle wrap-arounds with interrupts disabled
    // (There are other things that can disable interrupts, so
    // checking PRIMASK is insufficient, though.)
    debug_assert!(cortex_m::register::primask::read().is_inactive());

    loop {
        // `SYST::has_wrapped` takes `&mut self` for some mysterious reason, so
        // we are not using that
        let cycle = CYCLE.load(Ordering::Relaxed);
        cortex_m::asm::dmb(); // force ordering
        let value = SYST::get_current();
        cortex_m::asm::isb(); // force ordering and interrupt evaluation
        let cycle2 = CYCLE.load(Ordering::Relaxed);

        if cycle != cycle2 {
            // A wrap-around occurred - we can't tell if `value` belongs to
            // `cycle` or `cycle2`.
            continue;
        }

        return (value as u64 ^ 0xffffff) | ((cycle as u64) << 24);
    }
}
