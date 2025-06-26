use crate::{
    core::{data_context::HubClient, types::Fid},
    proto::{MessageData, cast_add_body::Parent, message_data::Body},
};
use prost::Message as ProstMessage;
use std::collections::HashSet;
use tracing::{debug, warn};

/// Maximum depth to traverse when finding root parent to prevent infinite loops
const MAX_DEPTH: usize = 100;

/// Represents the root parent information for a cast
#[derive(Debug, Clone)]
pub struct RootParentInfo {
    pub root_parent_fid: Option<i64>,
    pub root_parent_hash: Option<Vec<u8>>,
    pub root_parent_url: Option<String>,
}

/// Finds the root parent of a cast by recursively calling the Hub
/// Returns None if the cast has no parent (it is itself a root)
pub async fn find_root_parent_hub<H: HubClient>(
    hub_client: &H,
    parent_fid: Option<i64>,
    parent_hash: Option<&[u8]>,
    parent_url: Option<&str>,
) -> Result<Option<RootParentInfo>, Box<dyn std::error::Error + Send + Sync>> {
    // If there's no parent, this cast is a root
    if parent_fid.is_none() && parent_hash.is_none() && parent_url.is_none() {
        return Ok(None);
    }

    // If parent is a URL, return it as the root
    if let Some(url) = parent_url {
        return Ok(Some(RootParentInfo {
            root_parent_fid: None,
            root_parent_hash: None,
            root_parent_url: Some(url.to_string()),
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
        debug!(
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
                            // Parent is a URL, this is the root
                            return Ok(Some(RootParentInfo {
                                root_parent_fid: None,
                                root_parent_hash: None,
                                root_parent_url: Some(url.clone()),
                            }));
                        },
                        None => {
                            // No parent, this cast is the root
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
}
