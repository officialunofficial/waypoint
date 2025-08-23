#[derive(Debug)]
pub struct Message {
    pub id: String,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(id: String, payload: Vec<u8>) -> Self {
        Message { id, payload }
    }
}

pub const FARCASTER_EPOCH: u64 = 1609459200; // January 1, 2021 UTC

/// Helper function to generate consistent Redis stream keys across components
///
/// This function creates a standardized stream key pattern used throughout the application:
/// Format: hub:<hub_host>:stream:<event_type> or hub:<hub_host>:stream:<event_type>:<shard_suffix>
///
/// # Arguments
/// * `hub_host` - Host identifier for the Hub
/// * `event_type` - Type of event (e.g. "casts", "reactions", "user_data")
/// * `shard_suffix` - Optional shard suffix (e.g. "shard_0", "shard_1")
///
/// # Returns
/// A formatted stream key string
pub fn get_stream_key(hub_host: &str, event_type: &str, shard_suffix: Option<&str>) -> String {
    // Clean the hub_host to ensure consistent key format - strip any port numbers
    let clean_host = hub_host.split(':').next().unwrap_or(hub_host);

    // Generate the key - only add suffix if provided and non-empty
    match shard_suffix {
        Some(suffix) if !suffix.is_empty() => {
            format!("hub:{}:stream:{}:{}", clean_host, event_type, suffix)
        },
        _ => format!("hub:{}:stream:{}", clean_host, event_type),
    }
}
