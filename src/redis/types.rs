#[derive(Debug)]
pub struct PendingItem {
    pub id: String,
    pub idle_time: u64,
    pub delivery_count: u64,
}

#[derive(Debug, Clone)]
pub enum DeadLetterPolicy {
    /// Discard the message after max retries
    Discard,
    /// Move to dead letter queue after max retries
    MoveToDeadLetter { queue_name: String },
}

impl Default for DeadLetterPolicy {
    fn default() -> Self {
        Self::Discard
    }
}

/// Reason a message was sent to the dead letter queue
#[derive(Debug, Clone)]
pub enum DeadLetterReason {
    /// Maximum retries exceeded
    MaxRetriesExceeded,
    /// Message could not be decoded
    DecodeError,
    /// Processing timeout exceeded
    Timeout,
    /// Explicit rejection by processor
    Rejected,
}

impl std::fmt::Display for DeadLetterReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeadLetterReason::MaxRetriesExceeded => write!(f, "max_retries_exceeded"),
            DeadLetterReason::DecodeError => write!(f, "decode_error"),
            DeadLetterReason::Timeout => write!(f, "timeout"),
            DeadLetterReason::Rejected => write!(f, "rejected"),
        }
    }
}

/// Metadata for a dead letter queue entry
#[derive(Debug, Clone)]
pub struct DeadLetterMetadata {
    /// Original message ID from the source stream
    pub original_id: String,
    /// Source stream key
    pub source_stream: String,
    /// Consumer group that was processing
    pub group_name: String,
    /// Consumer that last attempted processing
    pub consumer_name: String,
    /// Number of delivery attempts before dead-lettering
    pub delivery_count: u64,
    /// Unix timestamp (milliseconds) when first delivered
    pub first_delivery_time: u64,
    /// Unix timestamp (milliseconds) when dead-lettered
    pub dead_letter_time: u64,
    /// Reason for dead-lettering
    pub reason: DeadLetterReason,
    /// Optional error message from last processing attempt
    pub error_message: Option<String>,
}

use std::sync::atomic::{AtomicU64, Ordering};

/// Processing metrics for a stream (atomic version for lock-free updates)
#[derive(Debug)]
pub struct AtomicStreamMetrics {
    processed_count: AtomicU64,
    error_count: AtomicU64,
    retry_count: AtomicU64,
    dead_letter_count: AtomicU64,
    /// Stored as bits for atomic operations
    processing_rate_bits: AtomicU64,
    average_latency_bits: AtomicU64,
    /// Used for rate calculation
    last_rate_update_count: AtomicU64,
}

impl Default for AtomicStreamMetrics {
    fn default() -> Self {
        Self {
            processed_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            retry_count: AtomicU64::new(0),
            dead_letter_count: AtomicU64::new(0),
            processing_rate_bits: AtomicU64::new(0),
            average_latency_bits: AtomicU64::new(0),
            last_rate_update_count: AtomicU64::new(0),
        }
    }
}

impl AtomicStreamMetrics {
    pub fn increment_processed(&self) {
        self.processed_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_dead_letter(&self) {
        self.dead_letter_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Update latency with exponential moving average
    pub fn update_latency(&self, latency_ms: u64) {
        let new_latency = latency_ms as f64;

        // Simple exponential moving average using compare-and-swap
        loop {
            let current_bits = self.average_latency_bits.load(Ordering::Relaxed);
            let current = f64::from_bits(current_bits);

            let updated =
                if current == 0.0 { new_latency } else { 0.9 * current + 0.1 * new_latency };

            if self
                .average_latency_bits
                .compare_exchange_weak(
                    current_bits,
                    updated.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Update rate every 100 messages
        let count = self.processed_count.load(Ordering::Relaxed);
        let last_update = self.last_rate_update_count.load(Ordering::Relaxed);

        if count >= last_update + 100
            && self
                .last_rate_update_count
                .compare_exchange_weak(last_update, count, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let avg_latency = f64::from_bits(self.average_latency_bits.load(Ordering::Relaxed));
            let avg_time_per_msg = avg_latency / 1000.0;
            if avg_time_per_msg > 0.0 {
                let rate = 1.0 / avg_time_per_msg;
                self.processing_rate_bits.store(rate.to_bits(), Ordering::Relaxed);
            }
        }
    }

    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> StreamMetrics {
        StreamMetrics {
            processed_count: self.processed_count.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            retry_count: self.retry_count.load(Ordering::Relaxed),
            dead_letter_count: self.dead_letter_count.load(Ordering::Relaxed),
            processing_rate: f64::from_bits(self.processing_rate_bits.load(Ordering::Relaxed)),
            average_latency_ms: f64::from_bits(self.average_latency_bits.load(Ordering::Relaxed)),
        }
    }
}

/// Processing metrics snapshot (non-atomic, for reading/display)
#[derive(Debug, Default, Clone)]
pub struct StreamMetrics {
    pub processed_count: u64,
    pub error_count: u64,
    pub retry_count: u64,
    pub dead_letter_count: u64,
    pub processing_rate: f64, // messages per second
    pub average_latency_ms: f64,
}

/// Per-consumer metrics (atomic for thread-safe updates)
#[derive(Debug)]
pub struct AtomicConsumerMetrics {
    /// Consumer identifier
    consumer_id: String,
    /// Stream key this consumer processes
    stream_key: String,
    /// Total messages successfully processed
    processed_count: AtomicU64,
    /// Total processing errors
    error_count: AtomicU64,
    /// Total retried messages
    retry_count: AtomicU64,
    /// Average processing latency (stored as f64 bits)
    average_latency_bits: AtomicU64,
    /// Minimum latency observed (ms)
    min_latency_ms: AtomicU64,
    /// Maximum latency observed (ms)
    max_latency_ms: AtomicU64,
    /// Last active timestamp (Unix ms)
    last_active_ms: AtomicU64,
    /// Current batch size being processed
    current_batch_size: AtomicU64,
}

impl AtomicConsumerMetrics {
    /// Create metrics for a new consumer
    pub fn new(consumer_id: String, stream_key: String) -> Self {
        Self {
            consumer_id,
            stream_key,
            processed_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            retry_count: AtomicU64::new(0),
            average_latency_bits: AtomicU64::new(0),
            min_latency_ms: AtomicU64::new(u64::MAX),
            max_latency_ms: AtomicU64::new(0),
            last_active_ms: AtomicU64::new(0),
            current_batch_size: AtomicU64::new(0),
        }
    }

    /// Record successful message processing
    pub fn record_success(&self, latency_ms: u64) {
        self.processed_count.fetch_add(1, Ordering::Relaxed);
        self.update_latency(latency_ms);
        self.update_last_active();
    }

    /// Record processing error
    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        self.update_last_active();
    }

    /// Record retry
    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Set current batch size
    pub fn set_batch_size(&self, size: u64) {
        self.current_batch_size.store(size, Ordering::Relaxed);
    }

    fn update_latency(&self, latency_ms: u64) {
        // Update min latency
        let mut current_min = self.min_latency_ms.load(Ordering::Relaxed);
        while latency_ms < current_min {
            match self.min_latency_ms.compare_exchange_weak(
                current_min,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => current_min = v,
            }
        }

        // Update max latency
        let mut current_max = self.max_latency_ms.load(Ordering::Relaxed);
        while latency_ms > current_max {
            match self.max_latency_ms.compare_exchange_weak(
                current_max,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => current_max = v,
            }
        }

        // Update average (exponential moving average)
        let new_latency = latency_ms as f64;
        loop {
            let current_bits = self.average_latency_bits.load(Ordering::Relaxed);
            let current = f64::from_bits(current_bits);
            let updated =
                if current == 0.0 { new_latency } else { 0.9 * current + 0.1 * new_latency };

            if self
                .average_latency_bits
                .compare_exchange_weak(
                    current_bits,
                    updated.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    fn update_last_active(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_active_ms.store(now, Ordering::Relaxed);
    }

    /// Get a snapshot of consumer metrics
    pub fn snapshot(&self) -> ConsumerMetricsSnapshot {
        let min_latency = self.min_latency_ms.load(Ordering::Relaxed);
        ConsumerMetricsSnapshot {
            consumer_id: self.consumer_id.clone(),
            stream_key: self.stream_key.clone(),
            processed_count: self.processed_count.load(Ordering::Relaxed),
            error_count: self.error_count.load(Ordering::Relaxed),
            retry_count: self.retry_count.load(Ordering::Relaxed),
            average_latency_ms: f64::from_bits(self.average_latency_bits.load(Ordering::Relaxed)),
            min_latency_ms: if min_latency == u64::MAX { 0 } else { min_latency },
            max_latency_ms: self.max_latency_ms.load(Ordering::Relaxed),
            last_active_ms: self.last_active_ms.load(Ordering::Relaxed),
            current_batch_size: self.current_batch_size.load(Ordering::Relaxed),
        }
    }
}

/// Consumer metrics snapshot (non-atomic, for reading/display)
#[derive(Debug, Clone)]
pub struct ConsumerMetricsSnapshot {
    pub consumer_id: String,
    pub stream_key: String,
    pub processed_count: u64,
    pub error_count: u64,
    pub retry_count: u64,
    pub average_latency_ms: f64,
    pub min_latency_ms: u64,
    pub max_latency_ms: u64,
    pub last_active_ms: u64,
    pub current_batch_size: u64,
}

impl ConsumerMetricsSnapshot {
    /// Calculate error rate (errors / total operations)
    pub fn error_rate(&self) -> f64 {
        let total = self.processed_count + self.error_count;
        if total == 0 { 0.0 } else { self.error_count as f64 / total as f64 }
    }

    /// Check if consumer is idle (no activity for given duration)
    pub fn is_idle(&self, threshold_ms: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now.saturating_sub(self.last_active_ms) > threshold_ms
    }
}

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Registry for managing per-consumer metrics across all streams
#[derive(Debug, Default)]
pub struct ConsumerMetricsRegistry {
    /// Consumer metrics indexed by consumer_id
    consumers: RwLock<HashMap<String, Arc<AtomicConsumerMetrics>>>,
}

impl ConsumerMetricsRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self { consumers: RwLock::new(HashMap::new()) }
    }

    /// Get or create metrics for a consumer
    pub fn get_or_create(&self, consumer_id: &str, stream_key: &str) -> Arc<AtomicConsumerMetrics> {
        // Try read lock first for existing entry
        {
            let consumers = self.consumers.read().unwrap();
            if let Some(metrics) = consumers.get(consumer_id) {
                return Arc::clone(metrics);
            }
        }

        // Need write lock to insert new entry
        let mut consumers = self.consumers.write().unwrap();
        // Double-check in case another thread inserted while we waited
        if let Some(metrics) = consumers.get(consumer_id) {
            return Arc::clone(metrics);
        }

        let metrics =
            Arc::new(AtomicConsumerMetrics::new(consumer_id.to_string(), stream_key.to_string()));
        consumers.insert(consumer_id.to_string(), Arc::clone(&metrics));
        metrics
    }

    /// Get metrics for a specific consumer (if exists)
    pub fn get(&self, consumer_id: &str) -> Option<Arc<AtomicConsumerMetrics>> {
        let consumers = self.consumers.read().unwrap();
        consumers.get(consumer_id).cloned()
    }

    /// Remove a consumer from the registry
    pub fn remove(&self, consumer_id: &str) -> Option<Arc<AtomicConsumerMetrics>> {
        let mut consumers = self.consumers.write().unwrap();
        consumers.remove(consumer_id)
    }

    /// Get snapshots of all consumer metrics
    pub fn all_snapshots(&self) -> Vec<ConsumerMetricsSnapshot> {
        let consumers = self.consumers.read().unwrap();
        consumers.values().map(|m| m.snapshot()).collect()
    }

    /// Get snapshots filtered by stream key
    pub fn snapshots_for_stream(&self, stream_key: &str) -> Vec<ConsumerMetricsSnapshot> {
        let consumers = self.consumers.read().unwrap();
        consumers.values().filter(|m| m.stream_key == stream_key).map(|m| m.snapshot()).collect()
    }

    /// Get count of registered consumers
    pub fn consumer_count(&self) -> usize {
        let consumers = self.consumers.read().unwrap();
        consumers.len()
    }

    /// Remove idle consumers (no activity for given duration)
    pub fn cleanup_idle(&self, threshold_ms: u64) -> usize {
        let mut consumers = self.consumers.write().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let idle_keys: Vec<String> = consumers
            .iter()
            .filter(|(_, m)| {
                let last_active = m.last_active_ms.load(Ordering::Relaxed);
                now.saturating_sub(last_active) > threshold_ms
            })
            .map(|(k, _)| k.clone())
            .collect();

        let count = idle_keys.len();
        for key in idle_keys {
            consumers.remove(&key);
        }
        count
    }

    /// Get aggregate metrics across all consumers
    pub fn aggregate_metrics(&self) -> AggregateConsumerMetrics {
        let consumers = self.consumers.read().unwrap();
        let mut aggregate = AggregateConsumerMetrics::default();

        for metrics in consumers.values() {
            let snapshot = metrics.snapshot();
            aggregate.total_processed += snapshot.processed_count;
            aggregate.total_errors += snapshot.error_count;
            aggregate.total_retries += snapshot.retry_count;
            aggregate.consumer_count += 1;

            if snapshot.average_latency_ms > 0.0 {
                aggregate.avg_latency_sum += snapshot.average_latency_ms;
                aggregate.latency_samples += 1;
            }
        }

        if aggregate.latency_samples > 0 {
            aggregate.average_latency_ms =
                aggregate.avg_latency_sum / aggregate.latency_samples as f64;
        }

        aggregate
    }
}

/// Aggregate metrics across all consumers
#[derive(Debug, Default, Clone)]
pub struct AggregateConsumerMetrics {
    pub consumer_count: u64,
    pub total_processed: u64,
    pub total_errors: u64,
    pub total_retries: u64,
    pub average_latency_ms: f64,
    // Internal fields for calculation
    avg_latency_sum: f64,
    latency_samples: u64,
}

impl AggregateConsumerMetrics {
    /// Calculate overall error rate
    pub fn error_rate(&self) -> f64 {
        let total = self.total_processed + self.total_errors;
        if total == 0 { 0.0 } else { self.total_errors as f64 / total as f64 }
    }
}

/// Consumer group health information
#[derive(Debug)]
pub struct ConsumerGroupHealth {
    pub group_name: String,
    pub stream_key: String,
    pub pending_count: u64,
    pub consumers: Vec<ConsumerInfo>,
    pub lag: u64, // How far behind the consumer group is
}

#[derive(Debug)]
pub struct ConsumerInfo {
    pub name: String,
    pub pending_count: u64,
    pub idle_time: u64,
}

/// Redis pool health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolHealthStatus {
    /// Pool is healthy and operating normally
    Healthy,
    /// Pool is under pressure but still functional
    Degraded,
    /// Pool is unhealthy, connections may be failing
    Unhealthy,
    /// Pool is disconnected
    Disconnected,
}

impl PoolHealthStatus {
    /// Returns true if the pool is usable (Healthy or Degraded)
    pub fn is_usable(&self) -> bool {
        matches!(self, PoolHealthStatus::Healthy | PoolHealthStatus::Degraded)
    }
}

/// Comprehensive Redis pool health information
#[derive(Debug, Clone)]
pub struct PoolHealth {
    /// Overall health status
    pub status: PoolHealthStatus,
    /// Connection state from fred
    pub connection_state: String,
    /// Whether circuit breaker is open
    pub circuit_breaker_open: bool,
    /// Current backpressure level (if available)
    pub backpressure_level: Option<String>,
    /// Configured pool size
    pub pool_size: u32,
    /// Number of in-flight operations (if tracked)
    pub in_flight_ops: Option<u32>,
    /// Average latency in milliseconds (if tracked)
    pub avg_latency_ms: Option<f64>,
    /// Error rate (errors per total operations)
    pub error_rate: Option<f64>,
    /// Total operations since startup
    pub total_operations: u64,
    /// Total errors since startup
    pub total_errors: u64,
}
