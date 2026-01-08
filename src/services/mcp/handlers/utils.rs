//! Utility functions for MCP handlers

use crate::core::types::{Fid, Message as FarcasterMessage, MessageType};
use prost::Message as ProstMessage;

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
