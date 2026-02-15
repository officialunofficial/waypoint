//! Shared query helpers for message decoding and JSON formatting.

use crate::core::types::{Fid, Message as FarcasterMessage, MessageType};
use prost::Message as ProstMessage;

pub fn process_cast_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Cast {
        return None;
    }

    let cast_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::CastAddBody(cast_add)) => cast_add,
        _ => return None,
    };

    let mut cast_obj = serde_json::Map::new();

    cast_obj.insert(
        "fid".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.fid)),
    );
    cast_obj.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.timestamp)),
    );
    cast_obj.insert("hash".to_string(), serde_json::Value::String(message.id.value().to_string()));

    cast_obj.insert("text".to_string(), serde_json::Value::String(cast_body.text.clone()));

    if !cast_body.mentions.is_empty() {
        let mentions: Vec<serde_json::Value> = cast_body
            .mentions
            .iter()
            .map(|&fid| serde_json::Value::Number(serde_json::Number::from(fid)))
            .collect();
        cast_obj.insert("mentions".to_string(), serde_json::Value::Array(mentions));
    }

    if !cast_body.mentions_positions.is_empty() {
        let positions: Vec<serde_json::Value> = cast_body
            .mentions_positions
            .iter()
            .map(|&pos| serde_json::Value::Number(serde_json::Number::from(pos)))
            .collect();
        cast_obj.insert("mentions_positions".to_string(), serde_json::Value::Array(positions));
    }

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

pub fn process_reaction_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Reaction {
        return None;
    }

    let reaction_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::ReactionBody(reaction)) => reaction,
        _ => return None,
    };

    let mut reaction_obj = serde_json::Map::new();

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

pub fn process_link_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if message.message_type != MessageType::Link {
        return None;
    }

    let link_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::LinkBody(link)) => link,
        _ => return None,
    };

    let mut link_obj = serde_json::Map::new();

    link_obj.insert(
        "fid".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.fid)),
    );
    link_obj.insert(
        "timestamp".to_string(),
        serde_json::Value::Number(serde_json::Number::from(msg_data.timestamp)),
    );
    link_obj.insert("hash".to_string(), serde_json::Value::String(message.id.value().to_string()));

    link_obj.insert("link_type".to_string(), serde_json::Value::String(link_body.r#type.clone()));

    if let Some(crate::proto::link_body::Target::TargetFid(target_fid)) = &link_body.target {
        link_obj.insert(
            "target_fid".to_string(),
            serde_json::Value::Number(serde_json::Number::from(*target_fid)),
        );
    }

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

pub fn format_casts_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No casts found for FID {}", fid),
            None => "No casts found".to_string(),
        };
    }

    let mut casts = Vec::new();

    for message in messages {
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;
            if let Some(cast_obj) = process_cast_message(&message, &msg_data) {
                casts.push(serde_json::Value::Object(cast_obj));
            }
        }
    }

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

    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "Error formatting casts".to_string())
}

pub fn format_reactions_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No reactions found for FID {}", fid),
            None => "No reactions found".to_string(),
        };
    }

    let mut reactions = Vec::new();

    for message in messages {
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;
            if let Some(reaction_obj) = process_reaction_message(&message, &msg_data) {
                reactions.push(serde_json::Value::Object(reaction_obj));
            }
        }
    }

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

    serde_json::to_string_pretty(&result)
        .unwrap_or_else(|_| "Error formatting reactions".to_string())
}

pub fn format_links_response(messages: Vec<FarcasterMessage>, fid: Option<Fid>) -> String {
    if messages.is_empty() {
        return match fid {
            Some(fid) => format!("No links found for FID {}", fid),
            None => "No links found".to_string(),
        };
    }

    let mut links = Vec::new();

    for message in messages {
        if let Ok(data) = ProstMessage::decode(&*message.payload) {
            let msg_data: crate::proto::MessageData = data;
            if let Some(link_obj) = process_link_message(&message, &msg_data) {
                links.push(serde_json::Value::Object(link_obj));
            }
        }
    }

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

    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "Error formatting links".to_string())
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
}
