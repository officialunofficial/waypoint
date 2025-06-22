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
use tonic::metadata::MetadataValue;

/// Add custom headers to a gRPC request
/// Converts underscores to hyphens in header names for common conventions
pub fn add_custom_headers<T>(mut request: tonic::Request<T>, headers: &HashMap<String, String>) -> tonic::Request<T> {
    for (key, value) in headers {
        // Convert underscores to hyphens for header names
        // This allows env vars like WAYPOINT_HUB__HEADERS__x_api_key to become x-api-key
        let header_name = key.replace('_', "-");
        
        if let Ok(metadata_value) = MetadataValue::try_from(value) {
            if let Ok(header_key) = tonic::metadata::MetadataKey::from_bytes(header_name.as_bytes()) {
                request.metadata_mut().insert(header_key, metadata_value);
            }
        }
    }
    request
}
