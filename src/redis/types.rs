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

            let updated = if current == 0.0 {
                new_latency
            } else {
                0.9 * current + 0.1 * new_latency
            };

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
