use std::time::{SystemTime, UNIX_EPOCH};
use alloy_primitives::U256;
use color_eyre::eyre::{Result, eyre};

/// Farcaster epoch (January 1, 2021 UTC)
pub const FARCASTER_EPOCH: u64 = 1609459200;

/// Gets the current Unix timestamp in seconds
///
/// # Returns
///
/// The current Unix timestamp in seconds
///
/// # Panics
///
/// This function will panic if the system time is before the Unix epoch
pub fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch")
        .as_secs() as u64
}

/// Gets the current Farcaster timestamp (seconds since Farcaster epoch)
///
/// # Returns
///
/// The number of seconds elapsed since the Farcaster epoch
///
/// # Panics
///
/// This function will panic if the system time is before the Unix epoch
pub fn get_current_farcaster_timestamp() -> u32 {
    let now = get_current_timestamp();
    (now - FARCASTER_EPOCH) as u32
}

/// Gets a deadline timestamp (current time + seconds)
///
/// # Arguments
///
/// * `seconds` - The number of seconds to add to the current time
///
/// # Returns
///
/// A U256 representing the deadline timestamp
///
/// # Panics
///
/// This function will panic if the system time is before the Unix epoch
pub fn get_deadline_timestamp(seconds: u64) -> U256 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch")
        .as_secs();
    U256::from(now + seconds)
}

/// Converts a Unix timestamp to Farcaster timestamp
///
/// # Arguments
///
/// * `unix_timestamp` - The Unix timestamp to convert
///
/// # Returns
///
/// A Result containing the Farcaster timestamp or an error if the timestamp
/// is before the Farcaster epoch
pub fn unix_to_farcaster_timestamp(unix_timestamp: u64) -> Result<u32> {
    if unix_timestamp < FARCASTER_EPOCH {
        return Err(eyre!("Timestamp {} is before Farcaster epoch {}", unix_timestamp, FARCASTER_EPOCH));
    }
    Ok((unix_timestamp - FARCASTER_EPOCH) as u32)
}

/// Converts a Farcaster timestamp to Unix timestamp
///
/// # Arguments
///
/// * `farcaster_timestamp` - The Farcaster timestamp to convert
///
/// # Returns
///
/// The Unix timestamp
pub fn farcaster_to_unix_timestamp(farcaster_timestamp: u32) -> u64 {
    farcaster_timestamp as u64 + FARCASTER_EPOCH
}
