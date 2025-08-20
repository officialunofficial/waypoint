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
/// - Base format (without sharding): hub:<hub_host>:evt:msg:<event_type>
/// - With sharding: hub:<hub_host>:evt:msg:<shard_key>:<event_type>
///
/// IMPORTANT: For compatibility between subscriber and processor, always use "evt" as the
/// shard_key parameter. Using different shard keys will cause messages to be published
/// to streams that processors cannot find.
///
/// # Arguments
/// * `hub_host` - Host identifier for the Hub
/// * `event_type` - Type of event (e.g. "casts", "reactions", "user_data")
/// * `shard_key` - Optional shard key (ignored in current implementation for simplicity)
///
/// # Returns
/// A formatted stream key string
pub fn get_stream_key(hub_host: &str, event_type: &str, shard_key: Option<&str>) -> String {
    // Clean the hub_host to ensure consistent key format - strip any port numbers
    let clean_host = hub_host.split(':').next().unwrap_or(hub_host);

    // Use a simpler, non-redundant format
    match shard_key {
        Some(suffix) => format!("hub:{}:stream:{}:{}", clean_host, event_type, suffix),
        None => format!("hub:{}:stream:{}", clean_host, event_type),
    }
}
