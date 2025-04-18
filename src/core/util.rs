use crate::core::types::FARCASTER_EPOCH;

#[derive(Debug)]
pub struct HubError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for HubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HubError {}

pub fn to_farcaster_time(time_ms: u64) -> Result<u32, HubError> {
    if time_ms < FARCASTER_EPOCH {
        return Err(HubError {
            code: "bad_request.invalid_param".to_string(),
            message: format!("time_ms is before the farcaster epoch: {}", time_ms),
        });
    }
    let seconds_since_epoch = ((time_ms - FARCASTER_EPOCH) / 1000) as u32;
    Ok(seconds_since_epoch)
}

pub fn from_farcaster_time(time: u32) -> u64 {
    (time as u64) * 1000 + FARCASTER_EPOCH
}

pub fn get_farcaster_time() -> Result<u32, HubError> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| {
        HubError {
            code: "internal_error".to_string(),
            message: format!("failed to get time: {}", e),
        }
    })?;
    to_farcaster_time(now.as_millis() as u64)
}

pub fn get_time_diff(timestamp: u64) -> String {
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
            as u64;

    let diff_ms = if timestamp > now { timestamp - now } else { now - timestamp };

    let diff_secs = diff_ms / 1000;
    let is_future = timestamp > now;

    let (value, unit) = if diff_secs < 60 {
        (diff_secs, "s")
    } else if diff_secs < 3600 {
        (diff_secs / 60, "m")
    } else if diff_secs < 86400 {
        (diff_secs / 3600, "h")
    } else {
        (diff_secs / 86400, "d")
    };

    format!("({}{} {})", value, unit, if is_future { "in future" } else { "ago" })
}

pub fn calculate_message_hash(data_bytes: &[u8]) -> Vec<u8> {
    blake3::hash(data_bytes).as_bytes()[0..20].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_diff() {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
                as u64;

        // For absolute time differences, use exact assertions
        assert_eq!(get_time_diff(now), "(0s ago)");
        assert_eq!(get_time_diff(now - 30_000), "(30s ago)");
        assert_eq!(get_time_diff(now - 90_000), "(1m ago)");
        assert_eq!(get_time_diff(now - 3_600_000), "(1h ago)");
        assert_eq!(get_time_diff(now - 86_400_000), "(1d ago)");

        // For future time, just check that it contains the expected text since timing can vary
        let future_text = get_time_diff(now + 30_000);
        assert!(future_text.contains("in future"), "Expected 'in future' in text: {}", future_text);
    }

    #[test]
    fn test_get_farcaster_time() {
        let time = get_farcaster_time().unwrap();
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
                as u32;
        assert!(time <= now);
    }

    #[test]
    fn test_to_farcaster_time() {
        // It is an error to pass a time before the farcaster epoch
        let time = to_farcaster_time(0);
        assert!(time.is_err());

        let time = to_farcaster_time(FARCASTER_EPOCH - 1);
        assert!(time.is_err());

        let time = to_farcaster_time(FARCASTER_EPOCH).unwrap();
        assert_eq!(time, 0);

        let time = to_farcaster_time(FARCASTER_EPOCH + 1000).unwrap();
        assert_eq!(time, 1);
    }

    #[test]
    fn test_from_farcaster_time() {
        assert_eq!(from_farcaster_time(0), FARCASTER_EPOCH);
        assert_eq!(from_farcaster_time(1), FARCASTER_EPOCH + 1000);
        assert_eq!(from_farcaster_time(1000), FARCASTER_EPOCH + 1_000_000);
    }

    #[test]
    fn test_time_conversion_roundtrip() {
        let current_time =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
                as u64;

        let farcaster_time = to_farcaster_time(current_time).unwrap();
        let reconverted_time = from_farcaster_time(farcaster_time);

        // We might lose some precision due to seconds conversion
        assert!((current_time as i64 - reconverted_time as i64).abs() < 1000);
    }

    #[test]
    fn test_message_hash() {
        let data = b"test message";
        let hash = calculate_message_hash(data);
        assert_eq!(hash.len(), 20);

        // Test deterministic behavior
        let hash2 = calculate_message_hash(data);
        assert_eq!(hash, hash2);
    }
}
