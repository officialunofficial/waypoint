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
