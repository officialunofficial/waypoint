//! Parallel processing utilities for Redis stream data
//!
//! Provides rayon-based parallel decoding and processing for batch operations.

use prost::Message;
use rayon::prelude::*;

/// Result of parallel protobuf decoding
#[derive(Debug)]
pub struct DecodedEntry<T> {
    /// Original entry ID
    pub id: String,
    /// Decoded message or error
    pub result: Result<T, DecodeError>,
}

/// Error during decoding
#[derive(Debug)]
pub struct DecodeError {
    pub message: String,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Decode error: {}", self.message)
    }
}

impl std::error::Error for DecodeError {}

/// Batch of entries to decode in parallel
pub struct BatchDecoder<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: Message + Default + Send> BatchDecoder<T> {
    /// Decode a batch of raw entries in parallel using rayon
    ///
    /// This is useful for CPU-bound protobuf decoding when processing
    /// large batches of messages.
    ///
    /// # Arguments
    /// * `entries` - Slice of (id, data) pairs to decode
    ///
    /// # Returns
    /// Vector of decoded entries maintaining original order
    ///
    /// # Example
    /// ```ignore
    /// let entries = vec![
    ///     ("id1".to_string(), bytes1),
    ///     ("id2".to_string(), bytes2),
    /// ];
    /// let decoded = BatchDecoder::<HubEvent>::decode_batch(&entries);
    /// ```
    pub fn decode_batch(entries: &[(String, Vec<u8>)]) -> Vec<DecodedEntry<T>> {
        entries
            .par_iter()
            .map(|(id, data)| {
                let result =
                    T::decode(data.as_slice()).map_err(|e| DecodeError { message: e.to_string() });
                DecodedEntry { id: id.clone(), result }
            })
            .collect()
    }

    /// Decode a batch with a minimum threshold for parallel processing
    ///
    /// For small batches, sequential processing might be faster due to
    /// rayon overhead. This function uses sequential processing for
    /// batches smaller than the threshold.
    ///
    /// # Arguments
    /// * `entries` - Slice of (id, data) pairs to decode
    /// * `parallel_threshold` - Minimum batch size for parallel processing
    pub fn decode_batch_adaptive(
        entries: &[(String, Vec<u8>)],
        parallel_threshold: usize,
    ) -> Vec<DecodedEntry<T>> {
        if entries.len() < parallel_threshold {
            // Sequential for small batches
            entries
                .iter()
                .map(|(id, data)| {
                    let result = T::decode(data.as_slice())
                        .map_err(|e| DecodeError { message: e.to_string() });
                    DecodedEntry { id: id.clone(), result }
                })
                .collect()
        } else {
            // Parallel for large batches
            Self::decode_batch(entries)
        }
    }
}

/// Extension trait for parallel decoding on stream entries
pub trait ParallelDecode {
    /// Convert to a format suitable for parallel decoding
    fn to_decode_input(&self) -> Vec<(String, Vec<u8>)>;
}

impl ParallelDecode for Vec<crate::redis::stream::StreamEntry> {
    fn to_decode_input(&self) -> Vec<(String, Vec<u8>)> {
        self.iter().map(|e| (e.id.clone(), e.data.clone())).collect()
    }
}

/// Configuration for parallel processing
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Minimum batch size to use parallel processing
    pub parallel_threshold: usize,
    /// Whether parallel processing is enabled
    pub enabled: bool,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            parallel_threshold: 10, // Default: parallelize if >= 10 items
            enabled: true,
        }
    }
}

/// Decode HubEvents from stream entries with optional parallelization
pub fn decode_hub_events(
    entries: &[crate::redis::stream::StreamEntry],
    config: &ParallelConfig,
) -> Vec<DecodedEntry<crate::proto::HubEvent>> {
    let input: Vec<(String, Vec<u8>)> =
        entries.iter().map(|e| (e.id.clone(), e.data.clone())).collect();

    if !config.enabled || input.len() < config.parallel_threshold {
        // Sequential decoding
        input
            .into_iter()
            .map(|(id, data)| {
                use prost::Message;
                let result = crate::proto::HubEvent::decode(data.as_slice())
                    .map_err(|e| DecodeError { message: e.to_string() });
                DecodedEntry { id, result }
            })
            .collect()
    } else {
        // Parallel decoding with rayon
        input
            .into_par_iter()
            .map(|(id, data)| {
                use prost::Message;
                let result = crate::proto::HubEvent::decode(data.as_slice())
                    .map_err(|e| DecodeError { message: e.to_string() });
                DecodedEntry { id, result }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_config_default() {
        let config = ParallelConfig::default();
        assert!(config.enabled);
        assert_eq!(config.parallel_threshold, 10);
    }

    #[test]
    fn test_decode_error_display() {
        let err = DecodeError { message: "test error".to_string() };
        assert_eq!(format!("{}", err), "Decode error: test error");
    }
}
