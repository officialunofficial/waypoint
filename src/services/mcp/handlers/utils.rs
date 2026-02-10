//! Utility functions for MCP handlers

use crate::core::types::{Fid, Message as FarcasterMessage, MessageType};
use percent_encoding::percent_decode_str;
use prost::Message as ProstMessage;
use url::Url;

/// Process a cast message to extract relevant data
pub fn process_cast_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Cast {
        return None;
    }

    // Extract cast data
    let cast_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::CastAddBody(cast_add)) => cast_add,
        _ => return None,
    };

    // Create cast object
    let mut cast_obj = serde_json::Map::new();

    // Add cast metadata
    cast_obj.insert(
        "fid".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.fid)),
    );
    cast_obj.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.timestamp)),
    );
    cast_obj.insert("hash".to_string(), serde_json::Value::String(message.id.value().to_string()));

    // Add cast content
    cast_obj.insert("text".to_string(), serde_json::Value::String(cast_body.text.clone()));

    // Add mentions if present
    if !cast_body.mentions.is_empty() {
        let mentions: Vec<serde_json::Value> = cast_body
            .mentions
            .iter()
            .map(|&fid| serde_json::Value::Number(serde_json::Number::from(fid)))
            .collect();
        cast_obj.insert("mentions".to_string(), serde_json::Value::Array(mentions));
    }

    // Add mention positions if present
    if !cast_body.mentions_positions.is_empty() {
        let positions: Vec<serde_json::Value> = cast_body
            .mentions_positions
            .iter()
            .map(|&pos| serde_json::Value::Number(serde_json::Number::from(pos)))
            .collect();
        cast_obj.insert("mentions_positions".to_string(), serde_json::Value::Array(positions));
    }

    // Add embeds if present
    if !cast_body.embeds.is_empty() {
        let embeds: Vec<serde_json::Value> = cast_body
            .embeds
            .iter()
            .map(|embed| match &embed.embed {
                Some(crate::proto::embed::Embed::Url(url)) => {
                    serde_json::json!({ "type": "url", "url": url })
                },
                Some(crate::proto::embed::Embed::CastId(cast_id)) => {
                    serde_json::json!({
                        "type": "cast",
                        "fid": cast_id.fid,
                        "hash": hex::encode(&cast_id.hash)
                    })
                },
                None => serde_json::json!(null),
            })
            .collect();
        cast_obj.insert("embeds".to_string(), serde_json::Value::Array(embeds));
    }

    // Add parent if present
    match &cast_body.parent {
        Some(crate::proto::cast_add_body::Parent::ParentCastId(parent_cast_id)) => {
            let parent = serde_json::json!({
                "type": "cast",
                "fid": parent_cast_id.fid,
                "hash": hex::encode(&parent_cast_id.hash)
            });
            cast_obj.insert("parent".to_string(), parent);
        },
        Some(crate::proto::cast_add_body::Parent::ParentUrl(url)) => {
            let parent = serde_json::json!({
                "type": "url",
                "url": url
            });
            cast_obj.insert("parent".to_string(), parent);
        },
        None => {},
    }

    Some(cast_obj)
}

/// Process a reaction message to extract relevant data
pub fn process_reaction_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Reaction {
        return None;
    }

    // Extract reaction data
    let reaction_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::ReactionBody(reaction)) => reaction,
        _ => return None,
    };

    // Create reaction object
    let mut reaction_obj = serde_json::Map::new();

    // Add reaction metadata
    reaction_obj.insert(
        "fid".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.fid)),
    );
    reaction_obj.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.timestamp)),
    );
    reaction_obj
        .insert("hash".to_string(), serde_json::Value::String(message.id.value().to_string()));

    // Add reaction type
    let reaction_type_str = match reaction_body.r#type {
        1 => "like",
        2 => "recast",
        _ => "unknown",
    };
    reaction_obj.insert(
        "reaction_type".to_string(),
        serde_json::Value::String(reaction_type_str.to_string()),
    );
    reaction_obj.insert(
        "reaction_type_id".to_string(),
        serde_json::Value::Number(serde_json::Number::from(reaction_body.r#type)),
    );

    // Add target
    match &reaction_body.target {
        Some(crate::proto::reaction_body::Target::TargetCastId(cast_id)) => {
            let target = serde_json::json!({
                "type": "cast",
                "fid": cast_id.fid,
                "hash": hex::encode(&cast_id.hash)
            });
            reaction_obj.insert("target".to_string(), target);
        },
        Some(crate::proto::reaction_body::Target::TargetUrl(url)) => {
            let target = serde_json::json!({
                "type": "url",
                "url": url
            });
            reaction_obj.insert("target".to_string(), target);
        },
        None => {},
    }

    Some(reaction_obj)
}

/// Process a link message to extract relevant data
pub fn process_link_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Link {
        return None;
    }

    // Extract link data
    let link_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::LinkBody(link)) => link,
        _ => return None,
    };

    // Create link object
    let mut link_obj = serde_json::Map::new();

    // Add link metadata
    link_obj.insert(
        "fid".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.fid)),
    );
    link_obj.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.timestamp)),
    );
    link_obj.insert("hash".to_string(), serde_json::Value::String(message.id.value().to_string()));

    // Add link type
    link_obj.insert("link_type".to_string(), serde_json::Value::String(link_body.r#type.clone()));

    // Add target FID from link_body.target
    if let Some(crate::proto::link_body::Target::TargetFid(target_fid)) = &link_body.target {
        link_obj.insert(
            "target_fid".to_string(),
            serde_json::Value::Number(serde_json::Number::from(*target_fid)),
        );
    }

    // Add display timestamp if present
    if let Some(display_timestamp) = link_body.display_timestamp
        && display_timestamp > 0
    {
        link_obj.insert(
            "display_timestamp".to_string(),
            serde_json::Value::Number(serde_json::Number::from(display_timestamp)),
        );
    }

    Some(link_obj)
}

/// Format an array of cast messages into a JSON response
pub fn format_casts_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No casts found for FID {}", fid),
            None => "No casts found".to_string(),
        };
    }

    // Create a structured array of cast objects
    let mut casts = Vec::new();

    // Process each cast message
    for message in messages {
        // Try to decode the message payload as MessageData
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;

            // Process the cast
            if let Some(cast_obj) = process_cast_message(&message, &msg_data) {
                casts.push(serde_json::Value::Object(cast_obj));
            }
        }
    }

    // Wrap in a result object with metadata
    let result = match fid {
        Some(fid) => serde_json::json!({
            "fid": fid.value(),
            "count": casts.len(),
            "casts": casts
        }),
        None => serde_json::json!({
            "count": casts.len(),
            "casts": casts
        }),
    };

    // Convert to JSON string
    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "Error formatting casts".to_string())
}

/// Format an array of reaction messages into a JSON response
pub fn format_reactions_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No reactions found for FID {}", fid),
            None => "No reactions found".to_string(),
        };
    }

    // Create a structured array of reaction objects
    let mut reactions = Vec::new();

    // Process each reaction message
    for message in messages {
        // Try to decode the message payload as MessageData
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;

            // Process the reaction
            if let Some(reaction_obj) = process_reaction_message(&message, &msg_data) {
                reactions.push(serde_json::Value::Object(reaction_obj));
            }
        }
    }

    // Wrap in a result object with metadata
    let result = match fid {
        Some(fid) => serde_json::json!({
            "fid": fid.value(),
            "count": reactions.len(),
            "reactions": reactions
        }),
        None => serde_json::json!({
            "count": reactions.len(),
            "reactions": reactions
        }),
    };

    // Convert to JSON string
    serde_json::to_string_pretty(&result)
        .unwrap_or_else(|_| "Error formatting reactions".to_string())
}

/// Format an array of link messages into a JSON response
pub fn format_links_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No links found for FID {}", fid),
            None => "No links found".to_string(),
        };
    }

    // Create a structured array of link objects
    let mut links = Vec::new();

    // Process each link message
    for message in messages {
        // Try to decode the message payload as MessageData
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;

            // Process the link
            if let Some(link_obj) = process_link_message(&message, &msg_data) {
                links.push(serde_json::Value::Object(link_obj));
            }
        }
    }

    // Wrap in a result object with metadata
    let result = match fid {
        Some(fid) => serde_json::json!({
            "fid": fid.value(),
            "count": links.len(),
            "links": links
        }),
        None => serde_json::json!({
            "count": links.len(),
            "links": links
        }),
    };

    // Convert to JSON string
    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "Error formatting links".to_string())
}

/// Represents a parsed waypoint:// resource URI
///
/// URI structure follows RFC 3986 and RFC 6570 (URI Templates):
/// - Path segments identify resource type and primary identifiers (fid, hash)
/// - Query parameters are used for complex values like URLs (`?url=...`)
///
/// # Examples
/// - `waypoint://users/{fid}` - User by FID
/// - `waypoint://users/by-username/{username}` - User by username
/// - `waypoint://casts/{fid}/{hash}` - Specific cast
/// - `waypoint://conversations/{fid}/{hash}` - Conversation thread for a cast
/// - `waypoint://casts/by-parent-url?url={url}` - Casts by parent URL (query param)
/// - `waypoint://reactions/by-target-url?url={url}` - Reactions by target URL (query param)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaypointResource {
    /// User profile by Farcaster ID
    UserByFid { fid: u64 },
    /// User profile by username
    UserByUsername { username: String },
    /// Verifications for a user by FID
    VerificationsByFid { fid: u64 },
    /// Specific verification by FID and address
    VerificationByAddress { fid: u64, address: String },
    /// All verification messages for a user by FID
    AllVerificationMessagesByFid { fid: u64 },
    /// Specific cast by FID and hash
    Cast { fid: u64, hash: String },
    /// Conversation thread for a cast (includes replies, participants, context)
    Conversation { fid: u64, hash: String },
    /// All casts by a user's FID
    CastsByFid { fid: u64 },
    /// Casts mentioning a user by their FID
    CastsByMention { fid: u64 },
    /// Casts that are replies to a specific cast
    CastsByParent { fid: u64, hash: String },
    /// Casts that are replies to a URL (e.g., channel URL)
    CastsByParentUrl { url: String },
    /// All reactions by a user's FID
    ReactionsByFid { fid: u64 },
    /// Reactions targeting a specific cast
    ReactionsByTargetCast { fid: u64, hash: String },
    /// Reactions targeting a URL
    ReactionsByTargetUrl { url: String },
    /// Links created by a user (follows, etc.)
    LinksByFid { fid: u64 },
    /// Links targeting a specific user
    LinksByTarget { fid: u64 },
    /// Compact link state for a user
    LinkCompactStateByFid { fid: u64 },
    /// Username proof for a specific name
    UsernameProofByName { name: String },
    /// Username proofs for a user
    UsernameProofsByFid { fid: u64 },
}

/// Parse a waypoint:// resource URI into a WaypointResource
///
/// URI structure follows RFC 3986 (URI syntax) and RFC 6570 (URI Templates):
/// - Path segments identify resource type and simple identifiers (fid, hash)
/// - Query parameters (`?url=...`) are used for complex values like URLs
///
/// # Supported URI patterns
///
/// ## Users
/// - `waypoint://users/{fid}` - User by FID
/// - `waypoint://users/by-username/{username}` - User by username
///
/// ## Verifications
/// - `waypoint://verifications/{fid}` - Verifications for a user
/// - `waypoint://verifications/{fid}/{address}` - Specific verification by FID and address
/// - `waypoint://verifications/all-by-fid/{fid}` - All verification messages for a user
///
/// ## Casts
/// - `waypoint://casts/{fid}/{hash}` - Specific cast
/// - `waypoint://casts/by-fid/{fid}` - All casts by a user
/// - `waypoint://casts/by-mention/{fid}` - Casts mentioning a user
/// - `waypoint://casts/by-parent/{fid}/{hash}` - Replies to a cast
/// - `waypoint://casts/by-parent-url?url={url}` - Replies to a URL (RFC 6570 query)
///
/// ## Conversations
/// - `waypoint://conversations/{fid}/{hash}` - Conversation thread for a cast
///
/// ## Reactions
/// - `waypoint://reactions/by-fid/{fid}` - All reactions by a user
/// - `waypoint://reactions/by-target-cast/{fid}/{hash}` - Reactions to a cast
/// - `waypoint://reactions/by-target-url?url={url}` - Reactions to a URL (RFC 6570 query)
///
/// ## Links
/// - `waypoint://links/by-fid/{fid}` - Links created by a user
/// - `waypoint://links/by-target/{fid}` - Links targeting a user
/// - `waypoint://links/compact-state/{fid}` - Compact link state
///
/// ## Username Proofs
/// - `waypoint://username-proofs/by-name/{name}` - Username proof for a specific name
/// - `waypoint://username-proofs/{fid}` - Username proofs for a user
pub fn parse_waypoint_resource_uri(uri: &str) -> Result<WaypointResource, String> {
    let url = Url::parse(uri).map_err(|err| format!("Invalid resource URI: {err}"))?;

    if url.scheme() != "waypoint" {
        return Err("Unsupported resource scheme".to_string());
    }

    let mut segments: Vec<String> = Vec::new();

    // Handle host as first segment (waypoint://users/123 -> host is "users")
    if let Some(host) = url.host_str()
        && !host.is_empty()
    {
        segments.push(host.to_string());
    }

    // Collect path segments, percent-decoding each one
    if let Some(path_segments) = url.path_segments() {
        for segment in path_segments {
            if !segment.is_empty() {
                let decoded = percent_decode_str(segment)
                    .decode_utf8()
                    .map_err(|_| format!("Invalid UTF-8 in path segment: {segment}"))?;
                segments.push(decoded.into_owned());
            }
        }
    }

    if segments.is_empty() {
        return Err("Missing resource path".to_string());
    }

    // Helper to extract URL query parameter (RFC 6570 Level 1 query expansion)
    let get_url_param = || -> Result<String, String> {
        url.query_pairs()
            .find(|(k, _)| k == "url")
            .map(|(_, v)| v.into_owned())
            .ok_or_else(|| "Missing required 'url' query parameter".to_string())
    };

    let segment_refs: Vec<&str> = segments.iter().map(String::as_str).collect();

    match segment_refs.as_slice() {
        // Users
        ["users", fid] => Ok(WaypointResource::UserByFid { fid: parse_fid(fid)? }),
        ["users", "by-username", username] => {
            Ok(WaypointResource::UserByUsername { username: username.to_string() })
        },

        // Verifications
        ["verifications", fid] => Ok(WaypointResource::VerificationsByFid { fid: parse_fid(fid)? }),
        ["verifications", "all-by-fid", fid] => {
            Ok(WaypointResource::AllVerificationMessagesByFid { fid: parse_fid(fid)? })
        },
        ["verifications", fid, address] => Ok(WaypointResource::VerificationByAddress {
            fid: parse_fid(fid)?,
            address: address.to_string(),
        }),

        // Casts
        ["casts", "by-fid", fid] => Ok(WaypointResource::CastsByFid { fid: parse_fid(fid)? }),
        ["casts", "by-mention", fid] => {
            Ok(WaypointResource::CastsByMention { fid: parse_fid(fid)? })
        },
        ["casts", "by-parent", fid, hash] => {
            Ok(WaypointResource::CastsByParent { fid: parse_fid(fid)?, hash: hash.to_string() })
        },
        ["casts", "by-parent-url"] => {
            Ok(WaypointResource::CastsByParentUrl { url: get_url_param()? })
        },
        ["casts", fid, hash] => {
            Ok(WaypointResource::Cast { fid: parse_fid(fid)?, hash: hash.to_string() })
        },

        // Conversations
        ["conversations", fid, hash] => {
            Ok(WaypointResource::Conversation { fid: parse_fid(fid)?, hash: hash.to_string() })
        },

        // Reactions
        ["reactions", "by-fid", fid] => {
            Ok(WaypointResource::ReactionsByFid { fid: parse_fid(fid)? })
        },
        ["reactions", "by-target-cast", fid, hash] => Ok(WaypointResource::ReactionsByTargetCast {
            fid: parse_fid(fid)?,
            hash: hash.to_string(),
        }),
        ["reactions", "by-target-url"] => {
            Ok(WaypointResource::ReactionsByTargetUrl { url: get_url_param()? })
        },

        // Links
        ["links", "by-fid", fid] => Ok(WaypointResource::LinksByFid { fid: parse_fid(fid)? }),
        ["links", "by-target", fid] => Ok(WaypointResource::LinksByTarget { fid: parse_fid(fid)? }),
        ["links", "compact-state", fid] => {
            Ok(WaypointResource::LinkCompactStateByFid { fid: parse_fid(fid)? })
        },

        // Username proofs
        ["username-proofs", "by-name", name] => {
            Ok(WaypointResource::UsernameProofByName { name: name.to_string() })
        },
        ["username-proofs", fid] => {
            Ok(WaypointResource::UsernameProofsByFid { fid: parse_fid(fid)? })
        },

        _ => Err(format!("Unsupported resource path: {}", segments.join("/"))),
    }
}

fn parse_fid(segment: &str) -> Result<u64, String> {
    segment.parse::<u64>().map_err(|_| format!("Invalid FID: {segment}"))
}

pub fn parse_hash_bytes(hash: &str) -> Result<Vec<u8>, String> {
    let trimmed = hash.trim_start_matches("0x");
    if trimmed.is_empty() {
        return Err("Missing hash value".to_string());
    }

    hex::decode(trimmed).map_err(|_| format!("Invalid hash format: {hash}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://users/123").unwrap();
        assert_eq!(result, WaypointResource::UserByFid { fid: 123 });

        // Also works with triple slash (empty host)
        let result = parse_waypoint_resource_uri("waypoint:///users/456").unwrap();
        assert_eq!(result, WaypointResource::UserByFid { fid: 456 });
    }

    #[test]
    fn test_parse_user_by_username() {
        let result = parse_waypoint_resource_uri("waypoint://users/by-username/alice").unwrap();
        assert_eq!(result, WaypointResource::UserByUsername { username: "alice".to_string() });
    }

    #[test]
    fn test_parse_verifications() {
        let result = parse_waypoint_resource_uri("waypoint://verifications/123").unwrap();
        assert_eq!(result, WaypointResource::VerificationsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_verification_by_address() {
        let result = parse_waypoint_resource_uri("waypoint://verifications/123/0xabc123").unwrap();
        assert_eq!(
            result,
            WaypointResource::VerificationByAddress { fid: 123, address: "0xabc123".to_string() }
        );
    }

    #[test]
    fn test_parse_all_verification_messages_by_fid() {
        let result =
            parse_waypoint_resource_uri("waypoint://verifications/all-by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::AllVerificationMessagesByFid { fid: 123 });
    }

    #[test]
    fn test_parse_cast() {
        let result = parse_waypoint_resource_uri("waypoint://casts/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::Cast { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_casts_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::CastsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_casts_by_mention() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-mention/123").unwrap();
        assert_eq!(result, WaypointResource::CastsByMention { fid: 123 });
    }

    #[test]
    fn test_parse_casts_by_parent() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-parent/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::CastsByParent { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_casts_by_parent_url_query_param() {
        // RFC 6570 compliant: URL in query parameter
        let result = parse_waypoint_resource_uri(
            "waypoint://casts/by-parent-url?url=https%3A%2F%2Fexample.com%2Fpost%2F123",
        )
        .unwrap();
        assert_eq!(
            result,
            WaypointResource::CastsByParentUrl { url: "https://example.com/post/123".to_string() }
        );
    }

    #[test]
    fn test_parse_casts_by_parent_url_missing_param() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-parent-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required 'url' query parameter"));
    }

    #[test]
    fn test_parse_conversation() {
        let result = parse_waypoint_resource_uri("waypoint://conversations/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::Conversation { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_reactions_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://reactions/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::ReactionsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_reactions_by_target_cast() {
        let result =
            parse_waypoint_resource_uri("waypoint://reactions/by-target-cast/123/0xdef").unwrap();
        assert_eq!(
            result,
            WaypointResource::ReactionsByTargetCast { fid: 123, hash: "0xdef".to_string() }
        );
    }

    #[test]
    fn test_parse_reactions_by_target_url_query_param() {
        // RFC 6570 compliant: URL in query parameter
        let result = parse_waypoint_resource_uri(
            "waypoint://reactions/by-target-url?url=https%3A%2F%2Fwarpcast.com%2F~%2Fchannel%2Ftest",
        )
        .unwrap();
        assert_eq!(
            result,
            WaypointResource::ReactionsByTargetUrl {
                url: "https://warpcast.com/~/channel/test".to_string()
            }
        );
    }

    #[test]
    fn test_parse_reactions_by_target_url_missing_param() {
        let result = parse_waypoint_resource_uri("waypoint://reactions/by-target-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required 'url' query parameter"));
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let result = parse_waypoint_resource_uri("http://users/123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported resource scheme"));
    }

    #[test]
    fn test_parse_invalid_fid() {
        let result = parse_waypoint_resource_uri("waypoint://users/notanumber");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid FID"));
    }

    #[test]
    fn test_parse_missing_path() {
        let result = parse_waypoint_resource_uri("waypoint://");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unsupported_path() {
        let result = parse_waypoint_resource_uri("waypoint://unknown/resource");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported resource path"));
    }

    #[test]
    fn test_parse_links_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://links/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::LinksByFid { fid: 123 });
    }

    #[test]
    fn test_parse_links_by_target() {
        let result = parse_waypoint_resource_uri("waypoint://links/by-target/456").unwrap();
        assert_eq!(result, WaypointResource::LinksByTarget { fid: 456 });
    }

    #[test]
    fn test_parse_links_compact_state() {
        let result = parse_waypoint_resource_uri("waypoint://links/compact-state/123").unwrap();
        assert_eq!(result, WaypointResource::LinkCompactStateByFid { fid: 123 });
    }

    #[test]
    fn test_parse_hash_bytes_with_prefix() {
        let bytes = parse_hash_bytes("0x0abc").unwrap();
        assert_eq!(bytes, vec![0x0a, 0xbc]);
    }

    #[test]
    fn test_parse_hash_bytes_without_prefix() {
        let bytes = parse_hash_bytes("0abc").unwrap();
        assert_eq!(bytes, vec![0x0a, 0xbc]);
    }

    #[test]
    fn test_parse_hash_bytes_empty() {
        let result = parse_hash_bytes("0x");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_hash_bytes_invalid() {
        let result = parse_hash_bytes("not-hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_username_proofs() {
        let result = parse_waypoint_resource_uri("waypoint://username-proofs/123").unwrap();
        assert_eq!(result, WaypointResource::UsernameProofsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_username_proof_by_name() {
        let result =
            parse_waypoint_resource_uri("waypoint://username-proofs/by-name/vitalik.eth").unwrap();
        assert_eq!(
            result,
            WaypointResource::UsernameProofByName { name: "vitalik.eth".to_string() }
        );
    }
}
