use crate::core::types::FARCASTER_EPOCH;
use std::borrow::Cow;

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

    let diff_ms = timestamp.abs_diff(now);

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

/// Strips null bytes from a string for PostgreSQL text insertion.
///
/// PostgreSQL text/varchar columns reject null bytes (`\x00`). This function returns
/// a `Cow<str>` for efficiency: borrowed if no null bytes are present, owned if sanitization
/// was needed. This is the idiomatic Rust approach for conditional string transformation.
#[inline]
pub fn sanitize_string_for_postgres(s: &str) -> Cow<'_, str> {
    if s.contains('\0') { Cow::Owned(s.replace('\0', "")) } else { Cow::Borrowed(s) }
}

/// Strips null bytes from all strings in a JSON Value tree for PostgreSQL jsonb insertion.
///
/// PostgreSQL jsonb rejects `\u0000`. Simple string replacement on serialized JSON is unsafe
/// because it can corrupt valid escape sequences like `\\u0000`. This function walks the
/// parsed Value tree and strips null bytes from decoded strings, then re-serializes.
pub fn sanitize_json_for_postgres(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.contains('\0') {
                serde_json::Value::String(s.replace('\0', ""))
            } else {
                serde_json::Value::String(s)
            }
        },
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sanitize_json_for_postgres).collect())
        },
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter().map(|(k, v)| (k, sanitize_json_for_postgres(v))).collect(),
        ),
        other => other, // Null, Bool, Number - pass through
    }
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

    #[test]
    fn test_sanitize_string_no_null_bytes() {
        let s = "Hello, world!";
        let result = sanitize_string_for_postgres(s);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_sanitize_string_with_null_byte() {
        let s = "Hello\0World";
        let result = sanitize_string_for_postgres(s);
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result, "HelloWorld");
    }

    #[test]
    fn test_sanitize_string_multiple_null_bytes() {
        let s = "\0start\0middle\0end\0";
        let result = sanitize_string_for_postgres(s);
        assert_eq!(result, "startmiddleend");
    }

    #[test]
    fn test_sanitize_string_empty() {
        let s = "";
        let result = sanitize_string_for_postgres(s);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "");
    }

    #[test]
    fn test_sanitize_string_only_null_bytes() {
        let s = "\0\0\0";
        let result = sanitize_string_for_postgres(s);
        assert_eq!(result, "");
    }

    #[test]
    fn test_sanitize_string_realistic_display_name() {
        // Simulates the issue: display name with trailing null terminator
        let s = "charlesmorales\0";
        let result = sanitize_string_for_postgres(s);
        assert_eq!(result, "charlesmorales");
    }

    #[test]
    fn test_sanitize_json_no_null_bytes() {
        let value = serde_json::json!({"text": "Hello, world!"});
        let result = sanitize_json_for_postgres(value.clone());
        assert_eq!(result, value);
    }

    #[test]
    fn test_sanitize_json_with_null_byte() {
        // String containing actual null byte
        let value = serde_json::json!({"text": "Hello\0World"});
        let result = sanitize_json_for_postgres(value);
        assert_eq!(result, serde_json::json!({"text": "HelloWorld"}));
    }

    #[test]
    fn test_sanitize_json_nested() {
        let value = serde_json::json!({
            "outer": {
                "inner": "has\0null"
            },
            "array": ["a\0b", "clean"]
        });
        let result = sanitize_json_for_postgres(value);
        assert_eq!(
            result,
            serde_json::json!({
                "outer": {"inner": "hasnull"},
                "array": ["ab", "clean"]
            })
        );
    }

    #[test]
    fn test_sanitize_json_preserves_escaped_backslash() {
        // This is the key test - \\u0000 in source becomes \u0000 in the string
        // (a literal backslash followed by u0000), which should NOT be corrupted
        let value = serde_json::json!({"text": r"\u0000"});
        let result = sanitize_json_for_postgres(value.clone());
        // The string contains literal \u0000 chars, no null byte, so unchanged
        assert_eq!(result, value);
    }

    #[test]
    fn test_sanitize_json_numbers_unchanged() {
        let value = serde_json::json!({"count": 42, "rate": 1.5});
        let result = sanitize_json_for_postgres(value.clone());
        assert_eq!(result, value);
    }

    #[test]
    fn test_sanitize_json_realistic_message() {
        // Simulate a protobuf message with null byte in text
        let value = serde_json::json!({
            "fid": 123,
            "text": "gm\0everyone",
            "type": 1
        });
        let result = sanitize_json_for_postgres(value);
        assert_eq!(
            result,
            serde_json::json!({
                "fid": 123,
                "text": "gmeveryone",
                "type": 1
            })
        );
    }
}
