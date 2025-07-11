use crate::{
    core::{data_context::HubClient, types::Fid},
    proto::{MessageData, cast_add_body::Parent, message_data::Body},
    redis::client::Redis,
};
use prost::Message as ProstMessage;
use std::collections::HashSet;
use tracing::{trace, warn, debug};

/// Maximum depth to traverse when finding root parent to prevent infinite loops
const MAX_DEPTH: usize = 100;

/// TTL for negative cache entries (5 minutes)
const NEGATIVE_CACHE_TTL: u64 = 300;

/// Redis stream key for cast retries
const CAST_RETRY_STREAM: &str = "cast_retry_stream";

/// Redis stream key for dead letter queue
const CAST_RETRY_DEAD: &str = "cast_retry_dead";

/// Maximum retry attempts before moving to dead letter queue
const MAX_RETRY_ATTEMPTS: u64 = 3;

/// Represents the root parent information for a cast
#[derive(Debug, Clone)]
pub struct RootParentInfo {
    pub root_parent_fid: Option<i64>,
    pub root_parent_hash: Option<Vec<u8>>,
    pub root_parent_url: Option<String>,
}

/// Result of cast processing with failure handling
#[derive(Debug)]
pub enum CastProcessingResult {
    Success(Option<RootParentInfo>),
    TemporaryFailure { 
        error: String,
        retry_after_seconds: u64 
    },
    PermanentFailure { 
        error: String 
    },
}

/// Classify error for retry strategy
fn classify_error(error: &str) -> (u64, bool) {
    match error {
        e if e.contains("Max depth") => (300, false), // 5 min retry, not permanent
        e if e.contains("timeout") || e.contains("connection") => (60, false), // 1 min retry
        e if e.contains("not found") => (300, false), // 5 min retry
        e if e.contains("decode") || e.contains("Invalid") => (0, true), // Permanent failure
        _ => (120, false), // Default: 2 min retry
    }
}

/// Check if parent lookup should be skipped (negative cache)
async fn is_parent_lookup_cached_failure(
    redis: &Redis,
    parent_hash: &[u8],
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let key = format!("failed_lookup:{}", hex::encode(parent_hash));
    let mut conn = redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
    
    let exists: Result<bool, _> = bb8_redis::redis::cmd("EXISTS")
        .arg(&key)
        .query_async(&mut *conn)
        .await;
    
    Ok(exists.unwrap_or(false))
}

/// Cache a failed parent lookup
async fn cache_parent_lookup_failure(
    redis: &Redis,
    parent_hash: &[u8],
    retry_after_seconds: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = format!("failed_lookup:{}", hex::encode(parent_hash));
    let mut conn = redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
    
    let _: Result<(), _> = bb8_redis::redis::cmd("SETEX")
        .arg(&key)
        .arg(retry_after_seconds)
        .arg("1")
        .query_async(&mut *conn)
        .await;
    
    Ok(())
}

/// Add failed cast to retry queue
async fn add_to_retry_queue(
    redis: &Redis,
    cast_fid: Option<i64>,
    cast_hash: Option<&[u8]>,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
    error: &str,
    attempt: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cast_hash_hex = cast_hash.map(hex::encode).unwrap_or_default();
    let parent_hash_hex = parent_hash.map(hex::encode).unwrap_or_default();
    
    let stream_key = if attempt >= MAX_RETRY_ATTEMPTS {
        CAST_RETRY_DEAD
    } else {
        CAST_RETRY_STREAM
    };
    
    let mut conn = redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
    
    let _: Result<String, _> = bb8_redis::redis::cmd("XADD")
        .arg(stream_key)
        .arg("*")
        .arg("cast_fid").arg(cast_fid.unwrap_or(0))
        .arg("cast_hash").arg(&cast_hash_hex)
        .arg("parent_fid").arg(parent_fid.unwrap_or(0))
        .arg("parent_hash").arg(&parent_hash_hex)
        .arg("parent_url").arg(parent_url.unwrap_or(""))
        .arg("error").arg(error)
        .arg("attempt").arg(attempt)
        .arg("timestamp").arg(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
        .query_async(&mut *conn)
        .await;
    
    debug!("Added cast to retry queue: stream={}, attempt={}, error={}", stream_key, attempt, error);
    Ok(())
}

/// Finds the root parent of a cast by recursively calling the Hub with failure handling
/// Returns None if the cast has no parent (it is itself a root)
pub async fn find_root_parent_hub_with_retry<H: HubClient>(
    hub_client: &H,
    redis: &Redis,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
    cast_fid: Option<i64>,
    cast_hash: Option<&[u8]>,
) -> Result<Option<RootParentInfo>, Box<dyn std::error::Error + Send + Sync>> {
    match find_root_parent_hub_internal(hub_client, redis, parent_fid, parent_hash, parent_url, cast_fid, cast_hash, 1).await {
        CastProcessingResult::Success(info) => Ok(info),
        CastProcessingResult::TemporaryFailure { error, .. } => {
            warn!("Temporary failure in root parent lookup, cast queued for retry: {}", error);
            // Return None to allow cast processing to continue without root parent
            Ok(None)
        },
        CastProcessingResult::PermanentFailure { error } => {
            warn!("Permanent failure in root parent lookup: {}", error);
            // Return None to allow cast processing to continue without root parent
            Ok(None)
        }
    }
}

/// Original function for backward compatibility
pub async fn find_root_parent_hub<H: HubClient>(
    hub_client: &H,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
    cast_fid: Option<i64>,
    cast_hash: Option<&[u8]>,
) -> Result<Option<RootParentInfo>, Box<dyn std::error::Error + Send + Sync>> {
    // Use internal function without retry logic for backward compatibility
    match find_root_parent_hub_internal_simple(hub_client, parent_fid, parent_hash, parent_url, cast_fid, cast_hash).await {
        Ok(info) => Ok(info),
        Err(e) => Err(e),
    }
}

/// Internal implementation with failure handling
async fn find_root_parent_hub_internal<H: HubClient>(
    hub_client: &H,
    redis: &Redis,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
    cast_fid: Option<i64>,
    cast_hash: Option<&[u8]>,
    attempt: u64,
) -> CastProcessingResult {
    // If there's no parent, this cast is a root
    if parent_fid.is_none() && parent_hash.is_none() && parent_url.is_none() {
        return CastProcessingResult::Success(None);
    }

    // If parent is only a URL (no cast parent), this cast is the root with a URL parent
    if parent_url.is_some() && parent_fid.is_none() && parent_hash.is_none() {
        return CastProcessingResult::Success(Some(RootParentInfo {
            root_parent_fid: cast_fid,
            root_parent_hash: cast_hash.map(|h| h.to_vec()),
            root_parent_url: parent_url.map(|s| s.to_string()),
        }));
    }

    // Must have both fid and hash for cast parent
    let (mut current_fid, mut current_hash) = match (parent_fid, parent_hash) {
        (Some(fid), Some(hash)) => (fid, hash.to_vec()),
        _ => {
            let error = "Invalid parent cast reference: missing fid or hash";
            warn!("{}", error);
            return CastProcessingResult::PermanentFailure { error: error.to_string() };
        },
    };

    // Check negative cache for this parent hash
    if let Some(hash) = parent_hash {
        if let Ok(is_cached_failure) = is_parent_lookup_cached_failure(redis, hash).await {
            if is_cached_failure {
                let error = format!("Parent lookup cached as failed: {}", hex::encode(hash));
                debug!("{}", error);
                // Add to retry queue and return failure
                let _ = add_to_retry_queue(redis, cast_fid, cast_hash, parent_fid, parent_hash, parent_url, &error, attempt).await;
                return CastProcessingResult::TemporaryFailure { 
                    error, 
                    retry_after_seconds: NEGATIVE_CACHE_TTL 
                };
            }
        }
    }

    // Track visited casts to detect cycles
    let mut visited = HashSet::new();
    visited.insert((current_fid, current_hash.clone()));

    // Traverse up the parent chain
    for depth in 0..MAX_DEPTH {
        trace!(
            "Finding root parent: depth={}, current_fid={}, current_hash={:?}",
            depth,
            current_fid,
            hex::encode(&current_hash)
        );

        // Get the cast from the Hub
        match hub_client.get_cast(Fid::from(current_fid as u64), &current_hash).await {
            Ok(Some(msg)) => {
                // Decode the message data from the payload
                let message_data = if !msg.payload.is_empty() {
                    match MessageData::decode(&msg.payload[..]) {
                        Ok(data) => data,
                        Err(e) => {
                            let error = format!("Failed to decode message data: {}", e);
                            warn!("{}", error);
                            return CastProcessingResult::TemporaryFailure { 
                                error, 
                                retry_after_seconds: 60 
                            };
                        },
                    }
                } else {
                    let error = "Empty message payload";
                    warn!("{}", error);
                    return CastProcessingResult::TemporaryFailure { 
                        error: error.to_string(), 
                        retry_after_seconds: 60 
                    };
                };

                // Check if this is a cast message and extract parent info
                if let Some(Body::CastAddBody(cast_body)) = &message_data.body {
                    match &cast_body.parent {
                        Some(Parent::ParentCastId(parent_cast_id)) => {
                            let parent_fid = parent_cast_id.fid as i64;
                            let parent_hash = parent_cast_id.hash.clone();

                            // Check for cycles
                            if visited.contains(&(parent_fid, parent_hash.clone())) {
                                let error = format!("Cycle detected in cast parent chain at fid={}", parent_fid);
                                warn!("{}", error);
                                return CastProcessingResult::Success(Some(RootParentInfo {
                                    root_parent_fid: Some(current_fid),
                                    root_parent_hash: Some(current_hash),
                                    root_parent_url: None,
                                }));
                            }

                            // Continue traversing
                            visited.insert((parent_fid, parent_hash.clone()));
                            current_fid = parent_fid;
                            current_hash = parent_hash;
                        },
                        Some(Parent::ParentUrl(url)) => {
                            // This cast has a URL parent but no cast parent, so it's the root
                            return CastProcessingResult::Success(Some(RootParentInfo {
                                root_parent_fid: Some(current_fid),
                                root_parent_hash: Some(current_hash),
                                root_parent_url: Some(url.clone()),
                            }));
                        },
                        None => {
                            // No parent at all, this cast is the root
                            return CastProcessingResult::Success(Some(RootParentInfo {
                                root_parent_fid: Some(current_fid),
                                root_parent_hash: Some(current_hash),
                                root_parent_url: None,
                            }));
                        },
                    }
                } else {
                    // Not a cast message, treat current as root
                    return CastProcessingResult::Success(Some(RootParentInfo {
                        root_parent_fid: Some(current_fid),
                        root_parent_hash: Some(current_hash),
                        root_parent_url: None,
                    }));
                }
            },
            Ok(None) => {
                // Cast not found - add to negative cache and retry queue
                let error = format!("Cast not found in hub: fid={}, hash={}", current_fid, hex::encode(&current_hash));
                warn!("{}", error);
                
                // Cache this failure
                let _ = cache_parent_lookup_failure(redis, &current_hash, NEGATIVE_CACHE_TTL).await;
                
                // Add to retry queue
                let _ = add_to_retry_queue(redis, cast_fid, cast_hash, parent_fid, parent_hash, parent_url, &error, attempt).await;
                
                return CastProcessingResult::TemporaryFailure { 
                    error, 
                    retry_after_seconds: NEGATIVE_CACHE_TTL 
                };
            },
            Err(e) => {
                // Hub error - classify and handle appropriately
                let error = format!("Error fetching cast from hub: {}", e);
                warn!("{}", error);
                
                let (retry_after, is_permanent) = classify_error(&error);
                
                if is_permanent {
                    return CastProcessingResult::PermanentFailure { error };
                } else {
                    // Cache temporary failure
                    let _ = cache_parent_lookup_failure(redis, &current_hash, retry_after).await;
                    
                    // Add to retry queue
                    let _ = add_to_retry_queue(redis, cast_fid, cast_hash, parent_fid, parent_hash, parent_url, &error, attempt).await;
                    
                    return CastProcessingResult::TemporaryFailure { 
                        error, 
                        retry_after_seconds: retry_after 
                    };
                }
            },
        }
    }

    // Hit max depth
    let error = format!("Max depth {} reached when finding root parent", MAX_DEPTH);
    warn!("{}", error);
    
    // Cache this failure and add to retry queue
    let _ = cache_parent_lookup_failure(redis, &current_hash, NEGATIVE_CACHE_TTL).await;
    let _ = add_to_retry_queue(redis, cast_fid, cast_hash, parent_fid, parent_hash, parent_url, &error, attempt).await;
    
    CastProcessingResult::TemporaryFailure { 
        error, 
        retry_after_seconds: NEGATIVE_CACHE_TTL 
    }
}

/// Simple internal implementation for backward compatibility
async fn find_root_parent_hub_internal_simple<H: HubClient>(
    hub_client: &H,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
    cast_fid: Option<i64>,
    cast_hash: Option<&[u8]>,
) -> Result<Option<RootParentInfo>, Box<dyn std::error::Error + Send + Sync>> {
    // If there's no parent, this cast is a root
    if parent_fid.is_none() && parent_hash.is_none() && parent_url.is_none() {
        return Ok(None);
    }

    // If parent is only a URL (no cast parent), this cast is the root with a URL parent
    if parent_url.is_some() && parent_fid.is_none() && parent_hash.is_none() {
        return Ok(Some(RootParentInfo {
            root_parent_fid: cast_fid,
            root_parent_hash: cast_hash.map(|h| h.to_vec()),
            root_parent_url: parent_url.map(|s| s.to_string()),
        }));
    }

    // Must have both fid and hash for cast parent
    let (mut current_fid, mut current_hash) = match (parent_fid, parent_hash) {
        (Some(fid), Some(hash)) => (fid, hash.to_vec()),
        _ => {
            warn!("Invalid parent cast reference: missing fid or hash");
            return Ok(None);
        },
    };

    // Track visited casts to detect cycles
    let mut visited = HashSet::new();
    visited.insert((current_fid, current_hash.clone()));

    // Traverse up the parent chain
    for depth in 0..MAX_DEPTH {
        trace!(
            "Finding root parent: depth={}, current_fid={}, current_hash={:?}",
            depth,
            current_fid,
            hex::encode(&current_hash)
        );

        // Get the cast from the Hub
        match hub_client.get_cast(Fid::from(current_fid as u64), &current_hash).await {
            Ok(Some(msg)) => {
                // Decode the message data from the payload
                let message_data = if !msg.payload.is_empty() {
                    match MessageData::decode(&msg.payload[..]) {
                        Ok(data) => data,
                        Err(e) => {
                            warn!("Failed to decode message data: {}", e);
                            // Treat this cast as root if we can't decode it
                            return Ok(Some(RootParentInfo {
                                root_parent_fid: Some(current_fid),
                                root_parent_hash: Some(current_hash),
                                root_parent_url: None,
                            }));
                        },
                    }
                } else {
                    warn!("Empty message payload");
                    return Ok(Some(RootParentInfo {
                        root_parent_fid: Some(current_fid),
                        root_parent_hash: Some(current_hash),
                        root_parent_url: None,
                    }));
                };

                // Check if this is a cast message and extract parent info
                if let Some(Body::CastAddBody(cast_body)) = &message_data.body {
                    match &cast_body.parent {
                        Some(Parent::ParentCastId(parent_cast_id)) => {
                            let parent_fid = parent_cast_id.fid as i64;
                            let parent_hash = parent_cast_id.hash.clone();

                            // Check for cycles
                            if visited.contains(&(parent_fid, parent_hash.clone())) {
                                warn!("Cycle detected in cast parent chain at fid={}", parent_fid);
                                // Return the current cast as root to break the cycle
                                return Ok(Some(RootParentInfo {
                                    root_parent_fid: Some(current_fid),
                                    root_parent_hash: Some(current_hash),
                                    root_parent_url: None,
                                }));
                            }

                            // Continue traversing
                            visited.insert((parent_fid, parent_hash.clone()));
                            current_fid = parent_fid;
                            current_hash = parent_hash;
                        },
                        Some(Parent::ParentUrl(url)) => {
                            // This cast has a URL parent but no cast parent, so it's the root
                            return Ok(Some(RootParentInfo {
                                root_parent_fid: Some(current_fid),
                                root_parent_hash: Some(current_hash),
                                root_parent_url: Some(url.clone()),
                            }));
                        },
                        None => {
                            // No parent at all, this cast is the root
                            return Ok(Some(RootParentInfo {
                                root_parent_fid: Some(current_fid),
                                root_parent_hash: Some(current_hash),
                                root_parent_url: None,
                            }));
                        },
                    }
                } else {
                    // Not a cast message, treat current as root
                    return Ok(Some(RootParentInfo {
                        root_parent_fid: Some(current_fid),
                        root_parent_hash: Some(current_hash),
                        root_parent_url: None,
                    }));
                }
            },
            Ok(None) => {
                // Cast not found, treat it as root
                warn!(
                    "Cast not found in hub: fid={}, hash={}",
                    current_fid,
                    hex::encode(&current_hash)
                );
                return Ok(Some(RootParentInfo {
                    root_parent_fid: Some(current_fid),
                    root_parent_hash: Some(current_hash),
                    root_parent_url: None,
                }));
            },
            Err(e) => {
                warn!("Error fetching cast from hub: {}", e);
                // On error, treat the current cast as root
                return Ok(Some(RootParentInfo {
                    root_parent_fid: Some(current_fid),
                    root_parent_hash: Some(current_hash),
                    root_parent_url: None,
                }));
            },
        }
    }

    // Hit max depth, use the current cast as root
    warn!("Max depth {} reached when finding root parent", MAX_DEPTH);
    Ok(Some(RootParentInfo {
        root_parent_fid: Some(current_fid),
        root_parent_hash: Some(current_hash),
        root_parent_url: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_parent_info_creation() {
        let info = RootParentInfo {
            root_parent_fid: Some(123),
            root_parent_hash: Some(vec![0xaa, 0xbb]),
            root_parent_url: None,
        };

        assert_eq!(info.root_parent_fid, Some(123));
        assert_eq!(info.root_parent_hash, Some(vec![0xaa, 0xbb]));
        assert_eq!(info.root_parent_url, None);
    }

    #[test]
    fn test_url_root_parent() {
        let info = RootParentInfo {
            root_parent_fid: None,
            root_parent_hash: None,
            root_parent_url: Some("https://example.com".to_string()),
        };

        assert_eq!(info.root_parent_fid, None);
        assert_eq!(info.root_parent_hash, None);
        assert_eq!(info.root_parent_url, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_combined_root_parent() {
        let info = RootParentInfo {
            root_parent_fid: Some(123),
            root_parent_hash: Some(vec![0xaa, 0xbb]),
            root_parent_url: Some("https://example.com".to_string()),
        };

        assert_eq!(info.root_parent_fid, Some(123));
        assert_eq!(info.root_parent_hash, Some(vec![0xaa, 0xbb]));
        assert_eq!(info.root_parent_url, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_root_parent_scenarios() {
        // Scenario 1: Root cast has no parent at all
        let info1 = RootParentInfo {
            root_parent_fid: Some(100),
            root_parent_hash: Some(vec![1, 2, 3]),
            root_parent_url: None,
        };
        assert!(info1.root_parent_url.is_none());

        // Scenario 2: Root cast has a URL parent
        let info2 = RootParentInfo {
            root_parent_fid: Some(200),
            root_parent_hash: Some(vec![4, 5, 6]),
            root_parent_url: Some("https://example.com".to_string()),
        };
        assert_eq!(info2.root_parent_url, Some("https://example.com".to_string()));
    }
}
