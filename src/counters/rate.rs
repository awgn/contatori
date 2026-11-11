//! Rate counter with sharded atomic storage and continuous-time EWMA smoothing.
//!
//! [`Rate`] tracks the rate of change (units per second) over time while keeping
//! hot-path increments completely lock-free and sharded (`Ordering::Relaxed`).
//!
//! On the cold read path ([`Rate::rate()`] / [`Observable::value()`]), it computes
//! an Exponentially Weighted Moving Average (EWMA) protected by a `const`-initialized
//! `Mutex` with a minimum sampling step so multiple concurrent observers can read the
//! rate idempotently without resetting each other's window.

use crossbeam_utils::CachePadded;
use std::fmt::Debug;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::counters::{
    CounterValue, DEFAULT_SHARDS, MetricKind, Observable, sealed, thread_slot_index,
};

/// Default EWMA smoothing time constant (`5 seconds`).
pub const DEFAULT_RATE_WINDOW: Duration = Duration::from_secs(5);

/// Minimum elapsed time before updating the EWMA sample (`10 milliseconds`).
///
/// Calls within this interval return the cached moving average so concurrent
/// observers reading back-to-back receive consistent, idempotent values.
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(10);

/// Sub-threshold rates when `delta == 0` snap cleanly to `0.0`.
const ZERO_THRESHOLD: f64 = 1e-6;

#[derive(Debug)]
struct RateState {
    last_instant: Option<Instant>,
    last_value: u64,
    ewma_rate: f64,
    has_sample: bool,
}

impl RateState {
    const fn new() -> Self {
        Self {
            last_instant: None,
            last_value: 0,
            ewma_rate: 0.0,
            has_sample: false,
        }
    }
}

/// A high-performance rate counter (units/second) using sharded atomic storage
/// and continuous-time EWMA smoothing.
///
/// # Const Initialization
///
/// ```rust
/// use contatori::Rate;
/// use std::time::Duration;
///
/// static REQUESTS_PER_SEC: Rate = Rate::new()
///     .with_name("requests_per_sec")
///     .with_window(Duration::from_secs(5));
/// ```
pub struct Rate<const SHARDS: usize = DEFAULT_SHARDS> {
    name: &'static str,
    window: Duration,
    components: [CachePadded<AtomicU64>; SHARDS],
    state: Mutex<RateState>,
}

impl Rate<DEFAULT_SHARDS> {
    /// Creates a new rate counter with `64` shards and a `5s` EWMA window.
    #[must_use]
    pub const fn new() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Rate<SHARDS> {
    /// Creates a new rate counter with `SHARDS` shards and a `5s` EWMA window.
    ///
    /// # Panics
    ///
    /// Fails to compile if `SHARDS` is zero or not a power of two.
    #[must_use]
    pub const fn with_shards() -> Self {
        const {
            assert!(
                SHARDS > 0 && SHARDS.is_power_of_two(),
                "SHARDS must be a non-zero power of two"
            );
        }
        Self {
            name: "",
            window: DEFAULT_RATE_WINDOW,
            components: [const { CachePadded::new(AtomicU64::new(0)) }; SHARDS],
            state: Mutex::new(RateState::new()),
        }
    }

    /// Sets the metric name of this counter, returning `self` for method chaining.
    #[must_use]
    pub const fn with_name(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }

    /// Sets the EWMA smoothing time constant (`window`), returning `self`.
    ///
    /// Pass `Duration::ZERO` to disable exponential smoothing and report pure
    /// interval rates (still protected by the `10ms` minimum sample interval).
    #[must_use]
    pub const fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Returns the configured EWMA smoothing window.
    #[must_use]
    #[inline]
    pub const fn window(&self) -> Duration {
        self.window
    }

    #[inline]
    fn local_slot(&self) -> &AtomicU64 {
        &self.components[thread_slot_index::<SHARDS>()]
    }

    /// Increments the counter by `1`.
    #[inline]
    pub fn incr(&self) {
        self.add(1);
    }

    /// Adds `value` to the counter.
    #[inline]
    pub fn add(&self, value: u64) {
        self.local_slot().fetch_add(value, Ordering::Relaxed);
    }

    /// Returns the value of the current thread's shard.
    #[must_use]
    #[inline]
    pub fn local_value(&self) -> u64 {
        self.local_slot().load(Ordering::Relaxed)
    }

    /// Creates a local batch accumulator for this rate counter.
    ///
    /// Increments on the returned [`Local`](crate::adapters::Local) proxy are buffered
    /// in non-atomic memory and flushed to this counter on [`Local::flush()`](crate::adapters::Local::flush)
    /// or when dropped. Call [`flush()`](crate::adapters::Local::flush) at the end of each batch
    /// so rate samples stay timely.
    #[must_use]
    #[inline]
    pub const fn local(&self) -> crate::adapters::Local<'_, Self> {
        crate::adapters::Local::__from_parts(self, 0)
    }

    /// Computes the cumulative count across all shards.
    #[must_use]
    #[inline]
    pub fn total_value(&self) -> u64 {
        self.components
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .fold(0u64, u64::wrapping_add)
    }

    /// Computes and returns the smoothed rate of change in units per second.
    ///
    /// - On the very first call, establishes the baseline `(t0, count0)` and returns `0.0`.
    /// - Calls separated by less than `10ms` return the cached moving average without
    ///   resetting the sampling baseline, making concurrent reads from multiple observers safe.
    /// - Subsequent calls update the continuous-time EWMA using $\alpha = 1 - e^{-\Delta t / \tau}$.
    pub fn rate(&self) -> f64 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let current_value = self.total_value();

        let Some(last_time) = state.last_instant else {
            state.last_instant = Some(now);
            state.last_value = current_value;
            return 0.0;
        };

        let elapsed = now.saturating_duration_since(last_time);
        if elapsed < MIN_SAMPLE_INTERVAL {
            return state.ewma_rate;
        }

        let elapsed_secs = elapsed.as_secs_f64();
        let delta = current_value.wrapping_sub(state.last_value);
        let instant_rate = delta as f64 / elapsed_secs;

        state.last_instant = Some(now);
        state.last_value = current_value;

        if !state.has_sample {
            state.ewma_rate = instant_rate;
            state.has_sample = true;
        } else {
            let tau = self.window.as_secs_f64();
            let alpha = if tau <= 0.0 {
                1.0
            } else {
                1.0 - (-elapsed_secs / tau).exp()
            };
            let next = alpha * instant_rate + (1.0 - alpha) * state.ewma_rate;
            state.ewma_rate = if delta == 0 && next < ZERO_THRESHOLD {
                0.0
            } else {
                next
            };
        }

        state.ewma_rate
    }
}

impl<const SHARDS: usize> Observable for Rate<SHARDS> {
    #[inline]
    fn value(&self) -> CounterValue {
        CounterValue::Float(self.rate())
    }

    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn metric_kind(&self) -> MetricKind {
        MetricKind::Gauge
    }
}

impl<const SHARDS: usize> sealed::Resettable for Rate<SHARDS> {
    #[inline]
    fn value_and_reset(&self) -> CounterValue {
        CounterValue::Float(self.rate())
    }
}

impl<const SHARDS: usize> Default for Rate<SHARDS> {
    fn default() -> Self {
        Self::with_shards()
    }
}

impl<const SHARDS: usize> Debug for Rate<SHARDS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, counter) in self.components.iter().enumerate() {
            let val = counter.load(Ordering::Relaxed);
            if val != 0 {
                write!(f, " [{i}]:{val}")?;
            }
        }
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        write!(
            f,
            " | last_value:{}, ewma:{:.2} }}",
            state.last_value, state.ewma_rate
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_const_static_init() {
        static COUNTER: Rate = Rate::new()
            .with_name("req_rate")
            .with_window(Duration::from_secs(2));
        COUNTER.incr();
        assert_eq!(COUNTER.name(), "req_rate");
        assert_eq!(COUNTER.window(), Duration::from_secs(2));
        assert!(COUNTER.total_value() >= 1);
    }

    #[test]
    fn test_first_call_baseline_and_rate_calculation() {
        let counter = Rate::new();
        counter.add(100);
        // First call establishes baseline
        assert_eq!(counter.rate(), 0.0);

        counter.add(1000);
        thread::sleep(Duration::from_millis(50));

        let r1 = counter.rate();
        assert!(r1 > 5_000.0 && r1 < 50_000.0, "unexpected rate {r1}");

        // Immediate second read from a second observer returns the exact same cached EWMA!
        let r2 = counter.rate();
        assert!((r1 - r2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rate_with_no_change_from_start() {
        let counter = Rate::new();
        counter.add(100);
        let _ = counter.rate();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(counter.rate(), 0.0);
    }

    #[test]
    fn test_ewma_smoothing_and_decay() {
        let counter = Rate::new().with_window(Duration::from_millis(50));
        let _ = counter.rate();
        counter.add(500);
        thread::sleep(Duration::from_millis(50));
        let initial = counter.rate();
        assert!(initial > 0.0);

        // No more traffic: rate decays smoothly rather than jumping wildly on sub-window reads
        thread::sleep(Duration::from_millis(30));
        let decayed = counter.rate();
        assert!(decayed < initial && decayed > 0.0);
    }

    #[test]
    fn test_concurrent_readers_and_writers_never_panic() {
        let counter = Arc::new(Rate::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let c = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                for _ in 0..1_000 {
                    c.incr();
                    let _ = c.rate();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(counter.total_value(), 4_000);
    }
}
