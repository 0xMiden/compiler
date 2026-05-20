use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use crate::HumanDuration;

const NOT_STARTED: u64 = u64::MAX;

/// This struct contains various statistics about a compilation session
pub struct Statistics {
    /// The time at which the compiler session started
    start_time: Instant,
    /// The elapsed time at which parsing started
    ///
    /// Parsing here refers to one of two things:
    ///
    /// 1. Loading of a Wasm module into memory and converting it to HIR
    /// 2. Parsing of an HIR module into memory
    parse_time: AtomicU64,
    /// The elapsed time at which optimizations/rewrites started
    opt_time: AtomicU64,
    /// The elapsed time at which codegen started
    codegen_time: AtomicU64,
}
impl fmt::Debug for Statistics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Statistics")
            .field("elapsed", &self.elapsed())
            .field("parsing", &self.parse_time())
            .field("optimization", &self.opt_time())
            .field("codegen", &self.codegen_time())
            .finish()
    }
}
impl Default for Statistics {
    fn default() -> Statistics {
        Self::new(Instant::now())
    }
}
impl Clone for Statistics {
    fn clone(&self) -> Self {
        Self {
            start_time: self.start_time,
            parse_time: AtomicU64::new(self.parse_time.load(Ordering::Relaxed)),
            opt_time: AtomicU64::new(self.opt_time.load(Ordering::Relaxed)),
            codegen_time: AtomicU64::new(self.codegen_time.load(Ordering::Relaxed)),
        }
    }
}
impl Statistics {
    pub fn new(start_time: Instant) -> Self {
        Self {
            start_time,
            parse_time: AtomicU64::new(NOT_STARTED),
            opt_time: AtomicU64::new(NOT_STARTED),
            codegen_time: AtomicU64::new(NOT_STARTED),
        }
    }

    /// Get the duration since the compiler session started
    pub fn elapsed(&self) -> HumanDuration {
        HumanDuration::since(self.start_time)
    }

    /// Get the time spent parsing/loading inputs, if applicable
    pub fn parse_time(&self) -> Option<HumanDuration> {
        load_duration(&self.parse_time)
    }

    /// Get the time spent optimizing the IR, if applicable
    pub fn opt_time(&self) -> Option<HumanDuration> {
        load_duration(&self.opt_time)
    }

    /// Get the time spent generating Miden Assembly, if applicable
    pub fn codegen_time(&self) -> Option<HumanDuration> {
        load_duration(&self.codegen_time)
    }

    /// Record that parsing/loading inputs has completed
    pub fn parsing_completed(&self) {
        store_duration(&self.parse_time, self.elapsed())
    }

    /// Record that optimization of the IR has completed
    pub fn optimization_completed(&self) {
        store_duration(&self.opt_time, self.elapsed())
    }

    /// Record that codegen of Miden Assembly has completed
    pub fn codegen_completed(&self) {
        store_duration(&self.codegen_time, self.elapsed())
    }
}

fn store_duration(raw_secs_f64: &AtomicU64, duration: HumanDuration) {
    let bits = duration.as_secs_f64().to_bits();
    raw_secs_f64.store(bits, Ordering::Relaxed)
}

fn load_duration(raw_secs_f64: &AtomicU64) -> Option<HumanDuration> {
    match raw_secs_f64.load(Ordering::Relaxed) {
        NOT_STARTED => None,
        bits => Some(Duration::from_secs_f64(f64::from_bits(bits)).into()),
    }
}
