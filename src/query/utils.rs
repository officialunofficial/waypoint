//! Shared query helpers for protobuf decoding and typed mapping.

use crate::core::types::{Message as FarcasterMessage, MessageType};
use crate::query::responses::{
    Cast, CastEmbed, CastParent, Link, Reaction, ReactionTarget, TypedCastReference,
    TypedUrlReference,
};
use prost::Message as ProstMessage;

pub fn process_cast_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<Cast> {
    if message.message_type != MessageType::Cast {
        return None;
    }

    let cast_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::CastAddBody(cast_add)) => cast_add,
        _ => return None,
    };

    let mentions =
        if cast_body.mentions.is_empty() { None } else { Some(cast_body.mentions.clone()) };

    let mentions_positions = if cast_body.mentions_positions.is_empty() {
        None
    } else {
        Some(cast_body.mentions_positions.clone())
    };

    let embeds = if cast_body.embeds.is_empty() {
        None
    } else {
        let values: Vec<CastEmbed> = cast_body
            .embeds
            .iter()
            .filter_map(|embed| match &embed.embed {
                Some(crate::proto::embed::Embed::Url(url)) => {
                    Some(CastEmbed::Url(TypedUrlReference {
                        target_type: "url".to_string(),
                        url: url.clone(),
                    }))
                },
                Some(crate::proto::embed::Embed::CastId(cast_id)) => {
                    Some(CastEmbed::Cast(TypedCastReference {
                        target_type: "cast".to_string(),
                        fid: cast_id.fid,
                        hash: hex::encode(&cast_id.hash),
                    }))
                },
                None => None,
            })
            .collect();

        if values.is_empty() { None } else { Some(values) }
    };

    let parent = match &cast_body.parent {
        Some(crate::proto::cast_add_body::Parent::ParentCastId(parent_cast_id)) => {
            Some(CastParent::Cast(TypedCastReference {
                target_type: "cast".to_string(),
                fid: parent_cast_id.fid,
                hash: hex::encode(&parent_cast_id.hash),
            }))
        },
        Some(crate::proto::cast_add_body::Parent::ParentUrl(url)) => {
            Some(CastParent::Url(TypedUrlReference {
                target_type: "url".to_string(),
                url: url.clone(),
            }))
        },
        None => None,
    };

    Some(Cast {
        fid: msg_data.fid,
        timestamp: msg_data.timestamp,
        hash: message.id.value().to_string(),
        text: cast_body.text.clone(),
        mentions,
        mentions_positions,
        embeds,
        parent,
    })
}

pub fn parse_cast_messages(messages: &[FarcasterMessage]) -> Vec<Cast> {
    messages
        .iter()
        .filter_map(|message| {
            ProstMessage::decode(&*message.payload).ok().and_then(
                |msg_data: crate::proto::MessageData| process_cast_message(message, &msg_data),
            )
        })
        .collect()
}

pub fn process_reaction_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<Reaction> {
    if message.message_type != MessageType::Reaction {
        return None;
    }

    let reaction_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::ReactionBody(reaction)) => reaction,
        _ => return None,
    };

    let reaction_type = match reaction_body.r#type {
        1 => "like",
        2 => "recast",
        _ => "unknown",
    }
    .to_string();

    let target = match &reaction_body.target {
        Some(crate::proto::reaction_body::Target::TargetCastId(cast_id)) => {
            Some(ReactionTarget::Cast(TypedCastReference {
                target_type: "cast".to_string(),
                fid: cast_id.fid,
                hash: hex::encode(&cast_id.hash),
            }))
        },
        Some(crate::proto::reaction_body::Target::TargetUrl(url)) => {
            Some(ReactionTarget::Url(TypedUrlReference {
                target_type: "url".to_string(),
                url: url.clone(),
            }))
        },
        None => None,
    };

    Some(Reaction {
        fid: msg_data.fid,
        timestamp: msg_data.timestamp,
        hash: message.id.value().to_string(),
        reaction_type,
        reaction_type_id: reaction_body.r#type,
        target,
    })
}

pub fn parse_reaction_messages(messages: &[FarcasterMessage]) -> Vec<Reaction> {
    messages
        .iter()
        .filter_map(|message| {
            ProstMessage::decode(&*message.payload).ok().and_then(
                |msg_data: crate::proto::MessageData| process_reaction_message(message, &msg_data),
            )
        })
        .collect()
}

pub fn process_link_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Option<Link> {
    if message.message_type != MessageType::Link {
        return None;
    }

    let link_body = match &msg_data.body {
        Some(crate::proto::message_data::Body::LinkBody(link)) => link,
        _ => return None,
    };

    let target_fid =
        if let Some(crate::proto::link_body::Target::TargetFid(target_fid)) = &link_body.target {
            Some(*target_fid)
        } else {
            None
        };

    let display_timestamp = link_body.display_timestamp.filter(|value| *value > 0);

    Some(Link {
        fid: msg_data.fid,
        timestamp: msg_data.timestamp,
        hash: message.id.value().to_string(),
        link_type: link_body.r#type.clone(),
        target_fid,
        display_timestamp,
    })
}

pub fn parse_link_messages(messages: &[FarcasterMessage]) -> Vec<Link> {
    messages
        .iter()
        .filter_map(|message| {
            ProstMessage::decode(&*message.payload).ok().and_then(
                |msg_data: crate::proto::MessageData| process_link_message(message, &msg_data),
            )
        })
        .collect()
}

fn process_link_compact_state_message(
    message: &FarcasterMessage,
    msg_data: &crate::proto::MessageData,
) -> Vec<Link> {
    if message.message_type != MessageType::Link {
        return Vec::new();
    }

    match &msg_data.body {
        Some(crate::proto::message_data::Body::LinkCompactStateBody(compact_state)) => {
            let hash = message.id.value().to_string();
            if compact_state.target_fids.is_empty() {
                return vec![Link {
                    fid: msg_data.fid,
                    timestamp: msg_data.timestamp,
                    hash,
                    link_type: compact_state.r#type.clone(),
                    target_fid: None,
                    display_timestamp: None,
                }];
            }

            compact_state
                .target_fids
                .iter()
                .map(|target_fid| Link {
                    fid: msg_data.fid,
                    timestamp: msg_data.timestamp,
                    hash: hash.clone(),
                    link_type: compact_state.r#type.clone(),
                    target_fid: Some(*target_fid),
                    display_timestamp: None,
                })
                .collect()
        },
        Some(crate::proto::message_data::Body::LinkBody(_)) => {
            process_link_message(message, msg_data).into_iter().collect()
        },
        _ => Vec::new(),
    }
}

pub fn parse_link_compact_state_messages(messages: &[FarcasterMessage]) -> Vec<Link> {
    messages
        .iter()
        .filter_map(|message| {
            ProstMessage::decode(&*message.payload)
                .ok()
                .map(|msg_data: crate::proto::MessageData| (message, msg_data))
        })
        .flat_map(|(message, msg_data)| process_link_compact_state_message(message, &msg_data))
        .collect()
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
