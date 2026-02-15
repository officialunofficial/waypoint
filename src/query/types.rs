//! Transport-neutral query DTOs.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeRange {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
}

impl TimeRange {
    pub const fn new(start_time: Option<u64>, end_time: Option<u64>) -> Self {
        Self { start_time, end_time }
    }

    pub fn describe(&self) -> String {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => format!(" between timestamps {} and {}", start, end),
            (Some(start), None) => format!(" after timestamp {}", start),
            (None, Some(end)) => format!(" before timestamp {}", end),
            (None, None) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationParams {
    pub recursive: bool,
    pub max_depth: usize,
    pub limit: usize,
}
