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
    use crate::proto::{Protocol, VerificationAddAddressBody, VerificationRemoveBody};

    #[test]
    fn test_truncate() {
        // Test basic ASCII string
        assert_eq!(truncate("Hello world", 5), "Hello...");

        // Test string shorter than max_chars
        assert_eq!(truncate("Hi", 5), "Hi");

        // Test with emojis
        assert_eq!(truncate("Hello 🌍🌎🌏", 7), "Hello 🌍...");

        // Test string with emoji at truncation point
        assert_eq!(truncate("🟨🟨🟨 Test", 3), "🟨🟨🟨...");

        // Test empty string
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_verification_add_formatting() {
        let verification = VerificationAddAddressBody {
            address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
            block_hash: hex::decode(
                "A9788AB993490646B7AE10A465B264F08DBDD7E346B3EC589C29BA1F85D98DD5",
            )
            .unwrap(),
            verification_type: 0,
            chain_id: 0,
            protocol: Protocol::Ethereum as i32,
            claim_signature: vec![/* ... */],
        };

        let formatted = format_verification_add(&verification);
        // Use case-insensitive check
        assert!(formatted.to_lowercase().contains("0x03118b1c6dc69c12047631538c646a099d851847"));
        assert!(formatted.contains("EOA"));
        assert!(formatted.contains("ETH"));
    }

    #[test]
    fn test_verification_remove_formatting() {
        let verification = VerificationRemoveBody {
            address: hex::decode("03118B1C6DC69C12047631538C646A099D851847").unwrap(),
            protocol: Protocol::Ethereum as i32,
        };

        let formatted = format_verification_remove(&verification);
        assert!(formatted.contains("Remove ETH verification"));
        // Use case-insensitive check
        assert!(formatted.to_lowercase().contains("0x03118b1c6dc69c12047631538c646a099d851847"));
    }
}
