use std::time::Duration;

use tracing::info;

pub struct ProcessingStats {
    pub total_events: usize,
    pub total_bytes: usize,
    pub batch_count: usize,
    pub filtered_events: usize,
    pub avg_batch_size: f64,
    pub processing_time: Duration,
    pub first_event_id: Option<u64>,
    pub last_event_id: Option<u64>,
}

impl Default for ProcessingStats {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessingStats {
    pub fn new() -> Self {
        Self {
            total_events: 0,
            total_bytes: 0,
            batch_count: 0,
            filtered_events: 0,
            avg_batch_size: 0.0,
            processing_time: Duration::ZERO,
            first_event_id: None,
            last_event_id: None,
        }
    }

    pub fn log(&self) {
        info!(
            "Stats: time={:?} batches={}, events={}, filtered={}, avg_batch={:.2}, first_id={}, last_id={}",
            self.processing_time,
            self.batch_count,
            self.total_events,
            self.filtered_events,
            self.avg_batch_size,
            self.first_event_id.unwrap_or(0),
            self.last_event_id.unwrap_or(0)
        );
    }
}
