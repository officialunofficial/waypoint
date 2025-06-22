pub mod circuit_breaker;
pub mod client;
pub mod error;
pub mod filter;
pub mod providers;
pub mod stats;
pub mod stream;
pub mod subscriber;

// Re-export data providers
pub use providers::FarcasterHubClient;

use std::collections::HashMap;
use tonic::metadata::{MetadataKey, MetadataValue};
use tracing::warn;

/// Add custom headers to a gRPC request.
///
/// This function converts environment variable naming conventions to proper HTTP header format:
/// - Converts underscores to hyphens (e.g., `X_API_KEY` -> `x-api-key`)
/// - Converts to lowercase as per HTTP/2 requirements
/// 
/// # Examples
/// 
/// Environment variables like `WAYPOINT_HUB__HEADERS__X_API_KEY` will be sent as the `x-api-key` header.
pub fn add_custom_headers<T>(mut request: tonic::Request<T>, headers: &HashMap<String, String>) -> tonic::Request<T> {
    for (key, value) in headers {
        // Convert to lowercase and replace underscores with hyphens for HTTP header format
        let header_name = key.to_lowercase().replace('_', "-");
        
        match MetadataValue::try_from(value.as_str()) {
            Ok(metadata_value) => {
                match MetadataKey::from_bytes(header_name.as_bytes()) {
                    Ok(header_key) => {
                        request.metadata_mut().insert(header_key, metadata_value);
                    }
                    Err(e) => {
                        warn!(
                            header_name = %header_name,
                            error = %e,
                            "Failed to create metadata key for custom header"
                        );
                    }
                }
            }
            Err(e) => {
                warn!(
                    header_name = %header_name,
                    error = %e,
                    "Failed to create metadata value for custom header"
                );
            }
        }
    }
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[test]
    fn test_add_custom_headers_converts_underscores_to_hyphens() {
        let mut headers = HashMap::new();
        headers.insert("X_API_KEY".to_string(), "test-key-123".to_string());
        headers.insert("AUTHORIZATION".to_string(), "Bearer token123".to_string());
        headers.insert("X_CUSTOM_HEADER".to_string(), "custom-value".to_string());

        let request = Request::new(());
        let request = add_custom_headers(request, &headers);
        
        let metadata = request.metadata();
        
        // Check that headers are properly converted to lowercase with hyphens
        assert_eq!(metadata.get("x-api-key").unwrap().to_str().unwrap(), "test-key-123");
        assert_eq!(metadata.get("authorization").unwrap().to_str().unwrap(), "Bearer token123");
        assert_eq!(metadata.get("x-custom-header").unwrap().to_str().unwrap(), "custom-value");
    }

    #[test]
    fn test_add_custom_headers_empty_map() {
        let headers = HashMap::new();
        let request = Request::new(());
        let request = add_custom_headers(request, &headers);
        
        // Should not panic and should return request unchanged
        assert_eq!(request.metadata().len(), 0);
    }

    #[test]
    fn test_add_custom_headers_with_invalid_header_name() {
        let mut headers = HashMap::new();
        // Invalid header name with special characters that aren't allowed
        headers.insert("INVALID HEADER NAME".to_string(), "value".to_string());
        headers.insert("VALID_HEADER".to_string(), "valid-value".to_string());

        let request = Request::new(());
        let request = add_custom_headers(request, &headers);
        
        let metadata = request.metadata();
        
        // Valid header should be added
        assert_eq!(metadata.get("valid-header").unwrap().to_str().unwrap(), "valid-value");
        // Invalid header should be skipped (contains space which isn't valid)
        assert!(metadata.get("invalid header name").is_none());
    }

    #[test]
    fn test_add_custom_headers_preserves_existing_headers() {
        let headers = HashMap::new();
        let mut request = Request::new(());
        
        // Add an existing header
        request.metadata_mut().insert(
            "existing-header",
            MetadataValue::from_static("existing-value")
        );
        
        let request = add_custom_headers(request, &headers);
        
        // Existing header should still be present
        assert_eq!(
            request.metadata().get("existing-header").unwrap().to_str().unwrap(),
            "existing-value"
        );
    }

    #[test]
    fn test_add_custom_headers_with_special_characters() {
        let mut headers = HashMap::new();
        // Test with various valid header values
        headers.insert("X_BEARER_TOKEN".to_string(), "Bearer abc123-xyz789".to_string());
        headers.insert("X_API_VERSION".to_string(), "v2.0".to_string());
        
        let request = Request::new(());
        let request = add_custom_headers(request, &headers);
        
        let metadata = request.metadata();
        assert_eq!(metadata.get("x-bearer-token").unwrap().to_str().unwrap(), "Bearer abc123-xyz789");
        assert_eq!(metadata.get("x-api-version").unwrap().to_str().unwrap(), "v2.0");
    }
}
