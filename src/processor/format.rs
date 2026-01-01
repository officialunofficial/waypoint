use crate::{
    core::util::{from_farcaster_time, get_time_diff},
    proto::{
        CastAddBody, CastId, CastRemoveBody, LendStorageBody, Message, MessageType, Protocol,
        ReactionBody, ReactionType, StorageUnitType, UserDataBody, UserDataType,
        VerificationAddAddressBody, VerificationRemoveBody, message_data::Body,
    },
};
use std::convert::TryFrom;

pub fn format_eth_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

pub fn format_eth_address(bytes: &[u8]) -> String {
    if bytes.len() == 20 {
        // If it's an address-length bytes, just format as hex
        // We're no longer using alloy's Address type for formatting
        format!("0x{}", hex::encode(bytes))
    } else {
        // Otherwise just format as regular hex
        format_eth_hex(bytes)
    }
}

fn format_verification_add(verification: &VerificationAddAddressBody) -> String {
    let address = format_eth_address(&verification.address);
    let block_hash = format_eth_hex(&verification.block_hash);

    // Format verification type
    let verification_type = match verification.verification_type {
        0 => "EOA",
        1 => "Contract",
        other => return format!("Unknown type {}", other),
    };

    // Format protocol
    let protocol = match Protocol::try_from(verification.protocol).unwrap_or(Protocol::Ethereum) {
        Protocol::Ethereum => "ETH",
        Protocol::Solana => "SOL",
    };

    // Format chain ID
    let chain = match verification.chain_id {
        0 => "EOA".to_string(),
        1 => "Mainnet".to_string(),
        10 => "Optimism".to_string(),
        other => format!("Chain {}", other),
    };

    format!(
        "Verify {} {} ({}) at block {} [{}]",
        protocol,
        address,
        verification_type,
        &block_hash[..10],
        chain
    )
}

fn format_verification_remove(verification: &VerificationRemoveBody) -> String {
    let address = format_eth_address(&verification.address);

    let protocol = match Protocol::try_from(verification.protocol).unwrap_or(Protocol::Ethereum) {
        Protocol::Ethereum => "ETH",
        Protocol::Solana => "SOL",
    };

    format!("Remove {} verification for {}", protocol, address)
}

pub fn format_message(msg: &Message) -> String {
    let data = msg.data.as_ref().map_or("No data".to_string(), |data| {
        let body_str = match &data.body {
            Some(Body::CastAddBody(cast)) => format_cast_add(cast),
            Some(Body::CastRemoveBody(cast)) => format_cast_remove(cast),
            Some(Body::ReactionBody(reaction)) => format_reaction(reaction),
            Some(Body::UserDataBody(user_data)) => format_user_data(user_data),
            Some(Body::VerificationAddAddressBody(verification)) => {
                format_verification_add(verification)
            },
            Some(Body::VerificationRemoveBody(verification)) => {
                format_verification_remove(verification)
            },
            Some(Body::LendStorageBody(lend_storage)) => format_lend_storage(lend_storage),
            Some(body) => format!("{:?}", body),
            None => "Empty body".to_string(),
        };

        format!(
            "{} | {} | {} | {}",
            get_time_diff(from_farcaster_time(data.timestamp)),
            data.fid,
            format_message_type(data.r#type),
            body_str,
        )
    });

    let hash = &format_eth_hex(&msg.hash)[..12];
    let signer = &format_eth_address(&msg.signer)[..12];

    format!("{} [hash: {}, signer: {}]", data, hash, signer)
}
fn format_message_type(msg_type: i32) -> &'static str {
    match MessageType::try_from(msg_type).unwrap_or(MessageType::None) {
        MessageType::None => "Unknown",
        MessageType::CastAdd => "Cast",
        MessageType::CastRemove => "Cast Remove",
        MessageType::ReactionAdd => "React Add",
        MessageType::ReactionRemove => "React Remove",
        MessageType::LinkAdd => "Link Add",
        MessageType::LinkRemove => "Link Remove",
        MessageType::VerificationAddEthAddress => "Verify ETH",
        MessageType::VerificationRemove => "Remove Verify",
        MessageType::UserDataAdd => "Profile Update",
        MessageType::UsernameProof => "Username",
        MessageType::FrameAction => "Frame Action",
        MessageType::LinkCompactState => "Link Compact",
        MessageType::LendStorage => "Lend Storage",
    }
}

fn format_cast_add(cast: &CastAddBody) -> String {
    let parent = match &cast.parent {
        Some(parent) => match parent {
            crate::proto::cast_add_body::Parent::ParentCastId(CastId { fid, hash }) => {
                format!("reply to {}:{}", fid, hex::encode(&hash[..4]))
            },
            crate::proto::cast_add_body::Parent::ParentUrl(url) => {
                format!("reply to url: {}", url)
            },
        },
        None => "new thread".to_string(),
    };

    let mentions = if !cast.mentions.is_empty() {
        format!(
            " mentions: [{}]",
            cast.mentions.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(",")
        )
    } else {
        "".to_string()
    };

    format!("{} ({}){}", truncate(&cast.text, 50), parent, mentions)
}

fn format_cast_remove(cast: &CastRemoveBody) -> String {
    format!("Remove cast {}", hex::encode(&cast.target_hash[..4]))
}

fn format_reaction(reaction: &ReactionBody) -> String {
    let target = match &reaction.target {
        Some(target) => match target {
            crate::proto::reaction_body::Target::TargetCastId(CastId { fid, hash }) => {
                format!("cast {}:{}", fid, hex::encode(&hash[..4]))
            },
            crate::proto::reaction_body::Target::TargetUrl(url) => {
                format!("url: {}", url)
            },
        },
        None => "unknown target".to_string(),
    };

    let reaction_type = ReactionType::try_from(reaction.r#type)
        .map(|rt| match rt {
            ReactionType::None => "none",
            ReactionType::Like => "like",
            ReactionType::Recast => "recast",
        })
        .unwrap_or("unknown");

    format!("{} -> {}", reaction_type, target)
}

fn format_user_data(user_data: &UserDataBody) -> String {
    let data_type = UserDataType::try_from(user_data.r#type)
        .map(|dt| match dt {
            UserDataType::None => "unknown",
            UserDataType::Pfp => "pfp",
            UserDataType::Display => "display name",
            UserDataType::Bio => "bio",
            UserDataType::Url => "url",
            UserDataType::Username => "username",
            UserDataType::Location => "location",
            UserDataType::Twitter => "twitter",
            UserDataType::Github => "github",
            UserDataType::Banner => "banner",
            UserDataType::UserDataPrimaryAddressEthereum => "primary ethereum address",
            UserDataType::UserDataPrimaryAddressSolana => "primary solana address",
            UserDataType::ProfileToken => "profile token",
        })
        .unwrap_or("unknown");

    format!("Set {} to: {}", data_type, truncate(&user_data.value, 30))
}

fn format_lend_storage(lend_storage: &LendStorageBody) -> String {
    let unit_type = StorageUnitType::try_from(lend_storage.unit_type)
        .map(|ut| match ut {
            StorageUnitType::UnitTypeLegacy => "legacy",
            StorageUnitType::UnitType2024 => "2024",
            StorageUnitType::UnitType2025 => "2025",
        })
        .unwrap_or("unknown");

    format!("Lend {} {} units to FID {}", lend_storage.num_units, unit_type, lend_storage.to_fid)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        // Collect the first max_chars characters into a String
        let mut result: String = s.chars().take(max_chars).collect();
        result.push_str("...");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{
        CastId, LendStorageBody, Protocol, ReactionType, StorageUnitType, UserDataType,
        VerificationAddAddressBody, VerificationRemoveBody,
    };

    mod truncate_tests {
        use super::*;

        #[test]
        fn test_truncate_basic_ascii() {
            assert_eq!(truncate("Hello world", 5), "Hello...");
        }

        #[test]
        fn test_truncate_shorter_than_max() {
            assert_eq!(truncate("Hi", 5), "Hi");
        }

        #[test]
        fn test_truncate_exact_length() {
            assert_eq!(truncate("Hello", 5), "Hello");
        }

        #[test]
        fn test_truncate_with_emojis() {
            assert_eq!(truncate("Hello 🌍🌎🌏", 7), "Hello 🌍...");
        }

        #[test]
        fn test_truncate_emoji_at_truncation_point() {
            assert_eq!(truncate("🟨🟨🟨 Test", 3), "🟨🟨🟨...");
        }

        #[test]
        fn test_truncate_empty_string() {
            assert_eq!(truncate("", 5), "");
        }

        #[test]
        fn test_truncate_unicode_characters() {
            // Japanese characters - 5 chars
            assert_eq!(truncate("こんにちは世界", 5), "こんにちは...");
        }
    }

    mod format_eth_tests {
        use super::*;

        #[test]
        fn test_format_eth_hex_empty() {
            assert_eq!(format_eth_hex(&[]), "0x");
        }

        #[test]
        fn test_format_eth_hex_single_byte() {
            assert_eq!(format_eth_hex(&[0xab]), "0xab");
        }

        #[test]
        fn test_format_eth_hex_multiple_bytes() {
            assert_eq!(format_eth_hex(&[0x01, 0x23, 0x45]), "0x012345");
        }

        #[test]
        fn test_format_eth_hex_leading_zeros() {
            assert_eq!(format_eth_hex(&[0x00, 0x01, 0x02]), "0x000102");
        }

        #[test]
        fn test_format_eth_address_20_bytes() {
            let address = hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap();
            let formatted = format_eth_address(&address);
            assert_eq!(formatted.to_lowercase(), "0x03118b1c6dc69c12047631538c646a099d851847");
        }

        #[test]
        fn test_format_eth_address_not_20_bytes() {
            let address = hex::decode("03118B1C").unwrap();
            let formatted = format_eth_address(&address);
            assert_eq!(formatted.to_lowercase(), "0x03118b1c");
        }
    }

    mod message_type_tests {
        use super::*;

        #[test]
        fn test_format_message_type_cast_add() {
            assert_eq!(format_message_type(MessageType::CastAdd as i32), "Cast");
        }

        #[test]
        fn test_format_message_type_cast_remove() {
            assert_eq!(format_message_type(MessageType::CastRemove as i32), "Cast Remove");
        }

        #[test]
        fn test_format_message_type_reaction_add() {
            assert_eq!(format_message_type(MessageType::ReactionAdd as i32), "React Add");
        }

        #[test]
        fn test_format_message_type_reaction_remove() {
            assert_eq!(format_message_type(MessageType::ReactionRemove as i32), "React Remove");
        }

        #[test]
        fn test_format_message_type_link_add() {
            assert_eq!(format_message_type(MessageType::LinkAdd as i32), "Link Add");
        }

        #[test]
        fn test_format_message_type_link_remove() {
            assert_eq!(format_message_type(MessageType::LinkRemove as i32), "Link Remove");
        }

        #[test]
        fn test_format_message_type_verify_eth() {
            assert_eq!(
                format_message_type(MessageType::VerificationAddEthAddress as i32),
                "Verify ETH"
            );
        }

        #[test]
        fn test_format_message_type_verify_remove() {
            assert_eq!(
                format_message_type(MessageType::VerificationRemove as i32),
                "Remove Verify"
            );
        }

        #[test]
        fn test_format_message_type_user_data() {
            assert_eq!(format_message_type(MessageType::UserDataAdd as i32), "Profile Update");
        }

        #[test]
        fn test_format_message_type_username_proof() {
            assert_eq!(format_message_type(MessageType::UsernameProof as i32), "Username");
        }

        #[test]
        fn test_format_message_type_frame_action() {
            assert_eq!(format_message_type(MessageType::FrameAction as i32), "Frame Action");
        }

        #[test]
        fn test_format_message_type_lend_storage() {
            assert_eq!(format_message_type(MessageType::LendStorage as i32), "Lend Storage");
        }

        #[test]
        fn test_format_message_type_unknown() {
            assert_eq!(format_message_type(999), "Unknown");
        }
    }

    mod verification_tests {
        use super::*;

        #[test]
        fn test_verification_add_eoa_ethereum_mainnet() {
            let verification = VerificationAddAddressBody {
                address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
                block_hash: hex::decode(
                    "A9788AB993490646B7AE10A465B264F08DBDD7E346B3EC589C29BA1F85D98DD5",
                )
                .unwrap(),
                verification_type: 0,
                chain_id: 1,
                protocol: Protocol::Ethereum as i32,
                claim_signature: vec![],
            };

            let formatted = format_verification_add(&verification);
            assert!(
                formatted.to_lowercase().contains("0x03118b1c6dc69c12047631538c646a099d851847")
            );
            assert!(formatted.contains("EOA"));
            assert!(formatted.contains("ETH"));
            assert!(formatted.contains("Mainnet"));
        }

        #[test]
        fn test_verification_add_contract_optimism() {
            let verification = VerificationAddAddressBody {
                address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
                block_hash: hex::decode(
                    "A9788AB993490646B7AE10A465B264F08DBDD7E346B3EC589C29BA1F85D98DD5",
                )
                .unwrap(),
                verification_type: 1,
                chain_id: 10,
                protocol: Protocol::Ethereum as i32,
                claim_signature: vec![],
            };

            let formatted = format_verification_add(&verification);
            assert!(formatted.contains("Contract"));
            assert!(formatted.contains("Optimism"));
        }

        #[test]
        fn test_verification_add_solana() {
            let verification = VerificationAddAddressBody {
                address: vec![1, 2, 3, 4, 5, 6, 7, 8],
                block_hash: vec![0; 32],
                verification_type: 0,
                chain_id: 0,
                protocol: Protocol::Solana as i32,
                claim_signature: vec![],
            };

            let formatted = format_verification_add(&verification);
            assert!(formatted.contains("SOL"));
        }

        #[test]
        fn test_verification_add_unknown_chain() {
            let verification = VerificationAddAddressBody {
                address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
                block_hash: vec![0; 32],
                verification_type: 0,
                chain_id: 42161, // Arbitrum
                protocol: Protocol::Ethereum as i32,
                claim_signature: vec![],
            };

            let formatted = format_verification_add(&verification);
            assert!(formatted.contains("Chain 42161"));
        }

        #[test]
        fn test_verification_add_unknown_type() {
            let verification = VerificationAddAddressBody {
                address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
                block_hash: vec![0; 32],
                verification_type: 99,
                chain_id: 1,
                protocol: Protocol::Ethereum as i32,
                claim_signature: vec![],
            };

            let formatted = format_verification_add(&verification);
            assert!(formatted.contains("Unknown type 99"));
        }

        #[test]
        fn test_verification_remove_ethereum() {
            let verification = VerificationRemoveBody {
                address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
                protocol: Protocol::Ethereum as i32,
            };

            let formatted = format_verification_remove(&verification);
            assert!(formatted.contains("Remove ETH verification"));
            assert!(
                formatted.to_lowercase().contains("0x03118b1c6dc69c12047631538c646a099d851847")
            );
        }

        #[test]
        fn test_verification_remove_solana() {
            let verification = VerificationRemoveBody {
                address: vec![1, 2, 3, 4],
                protocol: Protocol::Solana as i32,
            };

            let formatted = format_verification_remove(&verification);
            assert!(formatted.contains("Remove SOL verification"));
        }
    }

    mod reaction_tests {
        use super::*;

        #[test]
        fn test_format_reaction_like_cast() {
            let reaction = ReactionBody {
                r#type: ReactionType::Like as i32,
                target: Some(crate::proto::reaction_body::Target::TargetCastId(CastId {
                    fid: 12345,
                    hash: vec![0xab, 0xcd, 0xef, 0x12, 0x34, 0x56],
                })),
            };

            let formatted = format_reaction(&reaction);
            assert!(formatted.contains("like"));
            assert!(formatted.contains("cast 12345:abcdef12"));
        }

        #[test]
        fn test_format_reaction_recast_url() {
            let reaction = ReactionBody {
                r#type: ReactionType::Recast as i32,
                target: Some(crate::proto::reaction_body::Target::TargetUrl(
                    "https://example.com".to_string(),
                )),
            };

            let formatted = format_reaction(&reaction);
            assert!(formatted.contains("recast"));
            assert!(formatted.contains("url: https://example.com"));
        }

        #[test]
        fn test_format_reaction_no_target() {
            let reaction = ReactionBody { r#type: ReactionType::Like as i32, target: None };

            let formatted = format_reaction(&reaction);
            assert!(formatted.contains("like"));
            assert!(formatted.contains("unknown target"));
        }

        #[test]
        fn test_format_reaction_unknown_type() {
            let reaction = ReactionBody {
                r#type: 999,
                target: Some(crate::proto::reaction_body::Target::TargetCastId(CastId {
                    fid: 1,
                    hash: vec![0, 0, 0, 0],
                })),
            };

            let formatted = format_reaction(&reaction);
            assert!(formatted.contains("unknown"));
        }
    }

    mod user_data_tests {
        use super::*;

        #[test]
        fn test_format_user_data_display_name() {
            let user_data =
                UserDataBody { r#type: UserDataType::Display as i32, value: "Alice".to_string() };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("display name"));
            assert!(formatted.contains("Alice"));
        }

        #[test]
        fn test_format_user_data_bio() {
            let user_data =
                UserDataBody { r#type: UserDataType::Bio as i32, value: "Hello world".to_string() };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("bio"));
            assert!(formatted.contains("Hello world"));
        }

        #[test]
        fn test_format_user_data_pfp() {
            let user_data = UserDataBody {
                r#type: UserDataType::Pfp as i32,
                value: "https://example.com/avatar.jpg".to_string(),
            };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("pfp"));
        }

        #[test]
        fn test_format_user_data_username() {
            let user_data =
                UserDataBody { r#type: UserDataType::Username as i32, value: "alice".to_string() };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("username"));
            assert!(formatted.contains("alice"));
        }

        #[test]
        fn test_format_user_data_url() {
            let user_data = UserDataBody {
                r#type: UserDataType::Url as i32,
                value: "https://alice.com".to_string(),
            };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("url"));
        }

        #[test]
        fn test_format_user_data_location() {
            let user_data = UserDataBody {
                r#type: UserDataType::Location as i32,
                value: "New York".to_string(),
            };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("location"));
            assert!(formatted.contains("New York"));
        }

        #[test]
        fn test_format_user_data_twitter() {
            let user_data =
                UserDataBody { r#type: UserDataType::Twitter as i32, value: "@alice".to_string() };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("twitter"));
        }

        #[test]
        fn test_format_user_data_truncates_long_value() {
            let user_data = UserDataBody {
                r#type: UserDataType::Bio as i32,
                value: "This is a very long bio that should be truncated because it exceeds thirty characters"
                    .to_string(),
            };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("..."));
        }

        #[test]
        fn test_format_user_data_unknown_type() {
            let user_data = UserDataBody { r#type: 999, value: "test".to_string() };

            let formatted = format_user_data(&user_data);
            assert!(formatted.contains("unknown"));
        }
    }

    mod lend_storage_tests {
        use super::*;

        #[test]
        fn test_format_lend_storage_legacy() {
            let lend_storage = LendStorageBody {
                to_fid: 12345,
                num_units: 10,
                unit_type: StorageUnitType::UnitTypeLegacy as i32,
            };

            let formatted = format_lend_storage(&lend_storage);
            assert!(formatted.contains("Lend 10 legacy units to FID 12345"));
        }

        #[test]
        fn test_format_lend_storage_2024() {
            let lend_storage = LendStorageBody {
                to_fid: 67890,
                num_units: 5,
                unit_type: StorageUnitType::UnitType2024 as i32,
            };

            let formatted = format_lend_storage(&lend_storage);
            assert!(formatted.contains("Lend 5 2024 units to FID 67890"));
        }

        #[test]
        fn test_format_lend_storage_2025() {
            let lend_storage = LendStorageBody {
                to_fid: 11111,
                num_units: 3,
                unit_type: StorageUnitType::UnitType2025 as i32,
            };

            let formatted = format_lend_storage(&lend_storage);
            assert!(formatted.contains("Lend 3 2025 units to FID 11111"));
        }

        #[test]
        fn test_format_lend_storage_unknown_type() {
            let lend_storage = LendStorageBody { to_fid: 1, num_units: 1, unit_type: 999 };

            let formatted = format_lend_storage(&lend_storage);
            assert!(formatted.contains("unknown"));
        }
    }

    mod cast_tests {
        use super::*;

        #[test]
        fn test_format_cast_add_new_thread() {
            let cast = CastAddBody {
                text: "Hello world!".to_string(),
                parent: None,
                mentions: vec![],
                embeds_deprecated: vec![],
                embeds: vec![],
                mentions_positions: vec![],
                r#type: 0,
            };

            let formatted = format_cast_add(&cast);
            assert!(formatted.contains("Hello world!"));
            assert!(formatted.contains("new thread"));
        }

        #[test]
        fn test_format_cast_add_reply_to_cast() {
            let cast = CastAddBody {
                text: "Great post!".to_string(),
                parent: Some(crate::proto::cast_add_body::Parent::ParentCastId(CastId {
                    fid: 12345,
                    hash: vec![0xab, 0xcd, 0xef, 0x12, 0x34, 0x56],
                })),
                mentions: vec![],
                embeds_deprecated: vec![],
                embeds: vec![],
                mentions_positions: vec![],
                r#type: 0,
            };

            let formatted = format_cast_add(&cast);
            assert!(formatted.contains("Great post!"));
            assert!(formatted.contains("reply to 12345:abcdef12"));
        }

        #[test]
        fn test_format_cast_add_reply_to_url() {
            let cast = CastAddBody {
                text: "Interesting article".to_string(),
                parent: Some(crate::proto::cast_add_body::Parent::ParentUrl(
                    "https://example.com/article".to_string(),
                )),
                mentions: vec![],
                embeds_deprecated: vec![],
                embeds: vec![],
                mentions_positions: vec![],
                r#type: 0,
            };

            let formatted = format_cast_add(&cast);
            assert!(formatted.contains("reply to url: https://example.com/article"));
        }

        #[test]
        fn test_format_cast_add_with_mentions() {
            let cast = CastAddBody {
                text: "Hey @alice and @bob!".to_string(),
                parent: None,
                mentions: vec![100, 200],
                embeds_deprecated: vec![],
                embeds: vec![],
                mentions_positions: vec![],
                r#type: 0,
            };

            let formatted = format_cast_add(&cast);
            assert!(formatted.contains("mentions: [100,200]"));
        }

        #[test]
        fn test_format_cast_add_truncates_long_text() {
            let cast = CastAddBody {
                text: "This is a very long cast that should be truncated at fifty characters for display purposes"
                    .to_string(),
                parent: None,
                mentions: vec![],
                embeds_deprecated: vec![],
                embeds: vec![],
                mentions_positions: vec![],
                r#type: 0,
            };

            let formatted = format_cast_add(&cast);
            assert!(formatted.contains("..."));
        }

        #[test]
        fn test_format_cast_remove() {
            let cast = CastRemoveBody { target_hash: vec![0xab, 0xcd, 0xef, 0x12, 0x34, 0x56] };

            let formatted = format_cast_remove(&cast);
            assert!(formatted.contains("Remove cast abcdef12"));
        }
    }
}
