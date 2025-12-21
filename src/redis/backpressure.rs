//! Backpressure controller for Redis stream processing
//!
//! Implements adaptive load shedding and rate limiting to prevent
//! overwhelming the system during high load situations.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Backpressure state levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureLevel {
    /// Normal operation - no backpressure applied
    Normal,
    /// Light pressure - slightly reduce throughput
    Light,
    /// Moderate pressure - significantly reduce throughput
    Moderate,
    /// Heavy pressure - minimal processing, shed load
    Heavy,
    /// Critical - pause processing entirely
    Critical,
}

impl BackpressureLevel {
    /// Get the delay multiplier for this level
    pub fn delay_multiplier(&self) -> u64 {
        match self {
            BackpressureLevel::Normal => 0,
            BackpressureLevel::Light => 1,
            BackpressureLevel::Moderate => 2,
            BackpressureLevel::Heavy => 5,
            BackpressureLevel::Critical => 10,
        }
    }

    /// Get the batch size reduction factor (1.0 = no reduction)
    pub fn batch_size_factor(&self) -> f64 {
        match self {
            BackpressureLevel::Normal => 1.0,
            BackpressureLevel::Light => 0.8,
            BackpressureLevel::Moderate => 0.5,
            BackpressureLevel::Heavy => 0.25,
            BackpressureLevel::Critical => 0.1,
        }
    }
}

/// Configuration for backpressure controller
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Pending messages threshold for Light pressure
    pub light_threshold: u64,
    /// Pending messages threshold for Moderate pressure
    pub moderate_threshold: u64,
    /// Pending messages threshold for Heavy pressure
    pub heavy_threshold: u64,
    /// Pending messages threshold for Critical pressure
    pub critical_threshold: u64,
    /// Base delay in milliseconds when under pressure
    pub base_delay_ms: u64,
    /// How often to evaluate backpressure (milliseconds)
    pub evaluation_interval_ms: u64,
    /// Window size for rate calculation (seconds)
    pub rate_window_secs: u64,
    /// Maximum processing rate (events/sec) before applying pressure
    pub max_processing_rate: Option<u64>,
    /// Enable adaptive rate limiting based on latency
    pub adaptive_rate_limit: bool,
    /// Target latency in milliseconds for adaptive rate limiting
    pub target_latency_ms: u64,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            light_threshold: 1000,
            moderate_threshold: 5000,
            heavy_threshold: 10000,
            critical_threshold: 50000,
            base_delay_ms: 50,
            evaluation_interval_ms: 1000,
            rate_window_secs: 60,
            max_processing_rate: None,
            adaptive_rate_limit: true,
            target_latency_ms: 100,
        }
    }
}

/// Internal state for rate tracking
struct RateTracker {
    processed_count: u64,
    window_start: Instant,
    current_rate: f64,
}

/// Backpressure controller
pub struct BackpressureController {
    config: BackpressureConfig,
    /// Current backpressure level
    level: RwLock<BackpressureLevel>,
    /// Atomic counters for lock-free metrics
    pending_count: AtomicU64,
    in_flight_count: AtomicU32,
    /// Rate tracking state
    rate_tracker: RwLock<RateTracker>,
    /// Current average latency in milliseconds
    avg_latency_ms: AtomicU64,
    /// Total pressure events
    pressure_events: AtomicU64,
}

impl BackpressureController {
    /// Create a new backpressure controller
    pub fn new(config: BackpressureConfig) -> Self {
        Self {
            config,
            level: RwLock::new(BackpressureLevel::Normal),
            pending_count: AtomicU64::new(0),
            in_flight_count: AtomicU32::new(0),
            rate_tracker: RwLock::new(RateTracker {
                processed_count: 0,
                window_start: Instant::now(),
                current_rate: 0.0,
            }),
            avg_latency_ms: AtomicU64::new(0),
            pressure_events: AtomicU64::new(0),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(BackpressureConfig::default())
    }

    /// Update the pending messages count
    pub fn set_pending_count(&self, count: u64) {
        self.pending_count.store(count, Ordering::SeqCst);
    }

    /// Increment in-flight count when starting to process
    pub fn start_processing(&self) -> u32 {
        self.in_flight_count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Decrement in-flight count when done processing
    pub fn finish_processing(&self, latency_ms: u64) {
        self.in_flight_count.fetch_sub(1, Ordering::SeqCst);
        self.update_latency(latency_ms);
    }

    /// Update the average latency (exponential moving average)
    fn update_latency(&self, latency_ms: u64) {
        let current = self.avg_latency_ms.load(Ordering::Relaxed);
        if current == 0 {
            self.avg_latency_ms.store(latency_ms, Ordering::Relaxed);
        } else {
            // EMA with alpha = 0.1
            let new_avg = ((current as f64 * 0.9) + (latency_ms as f64 * 0.1)) as u64;
            self.avg_latency_ms.store(new_avg, Ordering::Relaxed);
        }
    }

    /// Record processed events for rate calculation
    pub async fn record_processed(&self, count: u64) {
        let mut tracker = self.rate_tracker.write().await;
        tracker.processed_count += count;

        // Update rate if window has elapsed
        let elapsed = tracker.window_start.elapsed();
        if elapsed >= Duration::from_secs(self.config.rate_window_secs) {
            let secs = elapsed.as_secs_f64();
            tracker.current_rate = tracker.processed_count as f64 / secs;
            tracker.processed_count = 0;
            tracker.window_start = Instant::now();
        }
    }

    /// Get current processing rate (events/second)
    pub async fn get_processing_rate(&self) -> f64 {
        self.rate_tracker.read().await.current_rate
    }

    /// Evaluate and update backpressure level
    pub async fn evaluate(&self) -> BackpressureLevel {
        let pending = self.pending_count.load(Ordering::SeqCst);
        let latency = self.avg_latency_ms.load(Ordering::Relaxed);
        let in_flight = self.in_flight_count.load(Ordering::Relaxed);

        let mut level = self.level.write().await;
        let old_level = *level;

        // Determine level based on pending count
        let pending_level = if pending >= self.config.critical_threshold {
            BackpressureLevel::Critical
        } else if pending >= self.config.heavy_threshold {
            BackpressureLevel::Heavy
        } else if pending >= self.config.moderate_threshold {
            BackpressureLevel::Moderate
        } else if pending >= self.config.light_threshold {
            BackpressureLevel::Light
        } else {
            BackpressureLevel::Normal
        };

        // Consider latency for adaptive rate limiting
        let latency_level = if self.config.adaptive_rate_limit {
            let target = self.config.target_latency_ms;
            if latency > target * 10 {
                BackpressureLevel::Critical
            } else if latency > target * 5 {
                BackpressureLevel::Heavy
            } else if latency > target * 2 {
                BackpressureLevel::Moderate
            } else if latency > target {
                BackpressureLevel::Light
            } else {
                BackpressureLevel::Normal
            }
        } else {
            BackpressureLevel::Normal
        };

        // Use the more severe level
        let new_level =
            if pending_level as u8 > latency_level as u8 { pending_level } else { latency_level };

        // Apply hysteresis - don't oscillate rapidly between levels
        if new_level != old_level {
            match (old_level, new_level) {
                // Allow immediate escalation
                (_, new) if new as u8 > old_level as u8 => {
                    *level = new_level;
                    self.pressure_events.fetch_add(1, Ordering::Relaxed);
                    warn!(
                        "Backpressure escalated: {:?} -> {:?} (pending={}, latency={}ms, in_flight={})",
                        old_level, new_level, pending, latency, in_flight
                    );
                },
                // Require sustained improvement to de-escalate
                (_, _) => {
                    // Only step down one level at a time
                    let one_step_down = match old_level {
                        BackpressureLevel::Critical => BackpressureLevel::Heavy,
                        BackpressureLevel::Heavy => BackpressureLevel::Moderate,
                        BackpressureLevel::Moderate => BackpressureLevel::Light,
                        BackpressureLevel::Light => BackpressureLevel::Normal,
                        BackpressureLevel::Normal => BackpressureLevel::Normal,
                    };
                    *level = one_step_down;
                    if one_step_down != old_level {
                        info!(
                            "Backpressure de-escalated: {:?} -> {:?} (pending={}, latency={}ms)",
                            old_level, one_step_down, pending, latency
                        );
                    }
                },
            }
        }

        *level
    }

    /// Get current backpressure level without re-evaluating
    pub async fn get_level(&self) -> BackpressureLevel {
        *self.level.read().await
    }

    /// Check if should pause processing
    pub async fn should_pause(&self) -> bool {
        matches!(self.get_level().await, BackpressureLevel::Critical)
    }

    /// Get recommended delay based on current backpressure
    pub async fn get_delay(&self) -> Duration {
        let level = self.get_level().await;
        let multiplier = level.delay_multiplier();
        Duration::from_millis(self.config.base_delay_ms * multiplier)
    }

    /// Get recommended batch size based on current backpressure
    pub async fn get_adjusted_batch_size(&self, base_size: u64) -> u64 {
        let level = self.get_level().await;
        let factor = level.batch_size_factor();
        ((base_size as f64 * factor) as u64).max(1)
    }

    /// Wait with backpressure delay if needed
    pub async fn wait_if_needed(&self) {
        let delay = self.get_delay().await;
        if !delay.is_zero() {
            debug!("Backpressure delay: {:?}", delay);
            tokio::time::sleep(delay).await;
        }
    }

    /// Get backpressure metrics
    pub fn get_metrics(&self) -> BackpressureMetrics {
        BackpressureMetrics {
            pending_count: self.pending_count.load(Ordering::Relaxed),
            in_flight_count: self.in_flight_count.load(Ordering::Relaxed),
            avg_latency_ms: self.avg_latency_ms.load(Ordering::Relaxed),
            pressure_events: self.pressure_events.load(Ordering::Relaxed),
        }
    }

    /// Force a specific backpressure level (for testing)
    pub async fn force_level(&self, level: BackpressureLevel) {
        let mut current = self.level.write().await;
        *current = level;
    }
}

/// Metrics from the backpressure controller
#[derive(Debug, Clone, Default)]
pub struct BackpressureMetrics {
    pub pending_count: u64,
    pub in_flight_count: u32,
    pub avg_latency_ms: u64,
    pub pressure_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backpressure_levels() {
        let config = BackpressureConfig {
            light_threshold: 10,
            moderate_threshold: 50,
            heavy_threshold: 100,
            critical_threshold: 500,
            ..Default::default()
        };
        let controller = BackpressureController::new(config);

        // Normal state
        controller.set_pending_count(5);
        assert_eq!(controller.evaluate().await, BackpressureLevel::Normal);

        // Light pressure
        controller.set_pending_count(15);
        assert_eq!(controller.evaluate().await, BackpressureLevel::Light);

        // Moderate pressure
        controller.set_pending_count(75);
        assert_eq!(controller.evaluate().await, BackpressureLevel::Moderate);

        // Heavy pressure
        controller.set_pending_count(200);
        assert_eq!(controller.evaluate().await, BackpressureLevel::Heavy);

        // Critical pressure
        controller.set_pending_count(600);
        assert_eq!(controller.evaluate().await, BackpressureLevel::Critical);
    }

    #[tokio::test]
    async fn test_batch_size_adjustment() {
        let controller = BackpressureController::with_defaults();

        // Normal - no reduction
        controller.force_level(BackpressureLevel::Normal).await;
        assert_eq!(controller.get_adjusted_batch_size(100).await, 100);

        // Light - 80%
        controller.force_level(BackpressureLevel::Light).await;
        assert_eq!(controller.get_adjusted_batch_size(100).await, 80);

        // Moderate - 50%
        controller.force_level(BackpressureLevel::Moderate).await;
        assert_eq!(controller.get_adjusted_batch_size(100).await, 50);

        // Heavy - 25%
        controller.force_level(BackpressureLevel::Heavy).await;
        assert_eq!(controller.get_adjusted_batch_size(100).await, 25);

        // Critical - 10%
        controller.force_level(BackpressureLevel::Critical).await;
        assert_eq!(controller.get_adjusted_batch_size(100).await, 10);
    }

    #[tokio::test]
    async fn test_delay_calculation() {
        let config = BackpressureConfig { base_delay_ms: 100, ..Default::default() };
        let controller = BackpressureController::new(config);

        controller.force_level(BackpressureLevel::Normal).await;
        assert_eq!(controller.get_delay().await, Duration::from_millis(0));

        controller.force_level(BackpressureLevel::Light).await;
        assert_eq!(controller.get_delay().await, Duration::from_millis(100));

        controller.force_level(BackpressureLevel::Moderate).await;
        assert_eq!(controller.get_delay().await, Duration::from_millis(200));

        controller.force_level(BackpressureLevel::Heavy).await;
        assert_eq!(controller.get_delay().await, Duration::from_millis(500));

        controller.force_level(BackpressureLevel::Critical).await;
        assert_eq!(controller.get_delay().await, Duration::from_millis(1000));
    }

    #[tokio::test]
    async fn test_in_flight_tracking() {
        let controller = BackpressureController::with_defaults();

        assert_eq!(controller.get_metrics().in_flight_count, 0);

        controller.start_processing();
        assert_eq!(controller.get_metrics().in_flight_count, 1);

        controller.start_processing();
        assert_eq!(controller.get_metrics().in_flight_count, 2);

        controller.finish_processing(50);
        assert_eq!(controller.get_metrics().in_flight_count, 1);

        controller.finish_processing(100);
        assert_eq!(controller.get_metrics().in_flight_count, 0);
    }

    #[tokio::test]
    async fn test_latency_tracking() {
        let controller = BackpressureController::with_defaults();

        // First latency sets the value directly
        controller.finish_processing(100);
        assert_eq!(controller.get_metrics().avg_latency_ms, 100);

        // Subsequent values use EMA
        controller.finish_processing(200);
        // 0.9 * 100 + 0.1 * 200 = 110
        assert_eq!(controller.get_metrics().avg_latency_ms, 110);
    }

    #[tokio::test]
    async fn test_should_pause() {
        let controller = BackpressureController::with_defaults();

        controller.force_level(BackpressureLevel::Heavy).await;
        assert!(!controller.should_pause().await);

        controller.force_level(BackpressureLevel::Critical).await;
        assert!(controller.should_pause().await);
    }

    #[tokio::test]
    async fn test_de_escalation_is_gradual() {
        let config = BackpressureConfig {
            light_threshold: 10,
            moderate_threshold: 50,
            heavy_threshold: 100,
            critical_threshold: 500,
            ..Default::default()
        };
        let controller = BackpressureController::new(config);

        // Escalate to critical
        controller.set_pending_count(600);
        controller.evaluate().await;
        assert_eq!(controller.get_level().await, BackpressureLevel::Critical);

        // Even with low pending, should only step down one level at a time
        controller.set_pending_count(0);
        controller.evaluate().await;
        assert_eq!(controller.get_level().await, BackpressureLevel::Heavy);

        controller.evaluate().await;
        assert_eq!(controller.get_level().await, BackpressureLevel::Moderate);

        controller.evaluate().await;
        assert_eq!(controller.get_level().await, BackpressureLevel::Light);

        controller.evaluate().await;
        assert_eq!(controller.get_level().await, BackpressureLevel::Normal);
    }
}
