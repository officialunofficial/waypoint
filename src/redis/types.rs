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

/// Processing metrics for a stream
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
