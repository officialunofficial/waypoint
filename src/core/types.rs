//! Core domain types for the application

use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use std::fmt;
use std::str::FromStr;

/// Farcaster timestamp epoch (January 1, 2021 UTC in milliseconds)
pub const FARCASTER_EPOCH: u64 = 1609459200000;

/// Farcaster Identifier (FID)
///
/// A newtype wrapper around u64 to provide type safety for FIDs
#[serde_as]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Fid(#[serde_as(as = "DisplayFromStr")] u64);

impl Fid {
    /// Create a new FID
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner value
    #[inline]
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for Fid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Fid {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id = s.parse::<u64>()?;
        Ok(Self(id))
    }
}

impl From<u64> for Fid {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

/// Message ID
///
/// A newtype wrapper around String to provide type safety for message IDs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(String);

impl MessageId {
    /// Create a new MessageId
    #[inline]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner value
    #[inline]
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Message types supported by the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Cast (post) messages
    Cast,
    /// Reaction (like, etc.) messages
    Reaction,
    /// Link messages
    Link,
    /// Verification messages
    Verification,
    /// User data messages
    UserData,
    /// Username proof messages
    UsernameProof,
    /// On-chain signer messages
    OnchainSigner,
    /// On-chain signer migrated messages
    OnchainSignerMigrated,
    /// On-chain ID register messages
    OnchainIdRegister,
    /// On-chain storage rent messages
    OnchainStorageRent,
    /// On-chain tier purchase messages
    OnchainTierPurchase,
    /// Lend storage messages
    LendStorage,
    /// Raw messages stream (all message types combined for external consumption)
    Messages,
}

impl MessageType {
    /// Convert to stream key suffix
    pub fn to_stream_key(&self) -> &'static str {
        match self {
            Self::Cast => "casts",
            Self::Reaction => "reactions",
            Self::Link => "links",
            Self::Verification => "verifications",
            Self::UserData => "user_data",
            Self::UsernameProof => "username_proofs",
            Self::OnchainSigner => "onchain:signer",
            Self::OnchainSignerMigrated => "onchain:signer_migrated",
            Self::OnchainIdRegister => "onchain:id_register",
            Self::OnchainStorageRent => "onchain:storage_rent",
            Self::OnchainTierPurchase => "onchain:tier_purchase",
            Self::LendStorage => "lend_storage",
            Self::Messages => "messages",
        }
    }

    /// Convert to consumer group suffix
    pub fn to_group_suffix(&self) -> &'static str {
        match self {
            Self::Cast => "casts",
            Self::Reaction => "reactions",
            Self::Link => "links",
            Self::Verification => "verifications",
            Self::UserData => "user_data",
            Self::UsernameProof => "username_proofs",
            Self::OnchainSigner
            | Self::OnchainSignerMigrated
            | Self::OnchainIdRegister
            | Self::OnchainStorageRent
            | Self::OnchainTierPurchase => "onchain",
            Self::LendStorage => "lend_storage",
            Self::Messages => "messages",
        }
    }

    /// Get all message types
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Cast,
            Self::Reaction,
            Self::Link,
            Self::Verification,
            Self::UserData,
            Self::UsernameProof,
            Self::OnchainSigner,
            Self::OnchainSignerMigrated,
            Self::OnchainIdRegister,
            Self::OnchainStorageRent,
            Self::OnchainTierPurchase,
            Self::LendStorage,
            Self::Messages,
        ]
        .into_iter()
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Cast => "cast",
                Self::Reaction => "reaction",
                Self::Link => "link",
                Self::Verification => "verification",
                Self::UserData => "user_data",
                Self::UsernameProof => "username_proof",
                Self::OnchainSigner => "onchain_signer",
                Self::OnchainSignerMigrated => "onchain_signer_migrated",
                Self::OnchainIdRegister => "onchain_id_register",
                Self::OnchainStorageRent => "onchain_storage_rent",
                Self::OnchainTierPurchase => "onchain_tier_purchase",
                Self::LendStorage => "lend_storage",
                Self::Messages => "messages",
            }
        )
    }
}

/// A message with its payload and ID
#[derive(Debug, Clone)]
pub struct Message {
    /// Unique message ID
    pub id: MessageId,
    /// Message type
    pub message_type: MessageType,
    /// Raw message payload
    pub payload: Vec<u8>,
}

impl Message {
    /// Create a new message
    pub fn new(id: impl Into<MessageId>, message_type: MessageType, payload: Vec<u8>) -> Self {
        Self { id: id.into(), message_type, payload }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod fid_tests {
        use super::*;

        #[test]
        fn test_fid_new_and_value() {
            let fid = Fid::new(12345);
            assert_eq!(fid.value(), 12345);
        }

        #[test]
        fn test_fid_from_u64() {
            let fid: Fid = 42u64.into();
            assert_eq!(fid.value(), 42);
        }

        #[test]
        fn test_fid_display() {
            let fid = Fid::new(999);
            assert_eq!(format!("{}", fid), "999");
        }

        #[test]
        fn test_fid_from_str() {
            let fid: Fid = "12345".parse().unwrap();
            assert_eq!(fid.value(), 12345);
        }

        #[test]
        fn test_fid_from_str_invalid() {
            let result: Result<Fid, _> = "not_a_number".parse();
            assert!(result.is_err());
        }

        #[test]
        fn test_fid_ordering() {
            let fid1 = Fid::new(100);
            let fid2 = Fid::new(200);
            let fid3 = Fid::new(100);

            assert!(fid1 < fid2);
            assert!(fid2 > fid1);
            assert_eq!(fid1, fid3);
        }

        #[test]
        fn test_fid_hash() {
            use std::collections::HashSet;
            let mut set = HashSet::new();
            set.insert(Fid::new(1));
            set.insert(Fid::new(2));
            set.insert(Fid::new(1)); // duplicate

            assert_eq!(set.len(), 2);
        }

        #[test]
        fn test_fid_serialization() {
            let fid = Fid::new(12345);
            let json = serde_json::to_string(&fid).unwrap();
            assert_eq!(json, "\"12345\"");

            let deserialized: Fid = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, fid);
        }
    }

    mod message_id_tests {
        use super::*;

        #[test]
        fn test_message_id_new_from_string() {
            let id = MessageId::new("test-id-123".to_string());
            assert_eq!(id.value(), "test-id-123");
        }

        #[test]
        fn test_message_id_new_from_str() {
            let id = MessageId::new("test-id");
            assert_eq!(id.value(), "test-id");
        }

        #[test]
        fn test_message_id_from_string() {
            let id: MessageId = "my-message".to_string().into();
            assert_eq!(id.value(), "my-message");
        }

        #[test]
        fn test_message_id_from_str() {
            let id: MessageId = "my-message".into();
            assert_eq!(id.value(), "my-message");
        }

        #[test]
        fn test_message_id_display() {
            let id = MessageId::new("display-test");
            assert_eq!(format!("{}", id), "display-test");
        }

        #[test]
        fn test_message_id_equality() {
            let id1 = MessageId::new("same");
            let id2 = MessageId::new("same");
            let id3 = MessageId::new("different");

            assert_eq!(id1, id2);
            assert_ne!(id1, id3);
        }

        #[test]
        fn test_message_id_hash() {
            use std::collections::HashSet;
            let mut set = HashSet::new();
            set.insert(MessageId::new("a"));
            set.insert(MessageId::new("b"));
            set.insert(MessageId::new("a")); // duplicate

            assert_eq!(set.len(), 2);
        }

        #[test]
        fn test_message_id_serialization() {
            let id = MessageId::new("test-123");
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, "\"test-123\"");

            let deserialized: MessageId = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, id);
        }
    }

    mod message_type_tests {
        use super::*;

        #[test]
        fn test_to_stream_key() {
            assert_eq!(MessageType::Cast.to_stream_key(), "casts");
            assert_eq!(MessageType::Reaction.to_stream_key(), "reactions");
            assert_eq!(MessageType::Link.to_stream_key(), "links");
            assert_eq!(MessageType::Verification.to_stream_key(), "verifications");
            assert_eq!(MessageType::UserData.to_stream_key(), "user_data");
            assert_eq!(MessageType::UsernameProof.to_stream_key(), "username_proofs");
            assert_eq!(MessageType::OnchainSigner.to_stream_key(), "onchain:signer");
            assert_eq!(
                MessageType::OnchainSignerMigrated.to_stream_key(),
                "onchain:signer_migrated"
            );
            assert_eq!(MessageType::OnchainIdRegister.to_stream_key(), "onchain:id_register");
            assert_eq!(MessageType::OnchainStorageRent.to_stream_key(), "onchain:storage_rent");
            assert_eq!(MessageType::OnchainTierPurchase.to_stream_key(), "onchain:tier_purchase");
            assert_eq!(MessageType::LendStorage.to_stream_key(), "lend_storage");
            assert_eq!(MessageType::Messages.to_stream_key(), "messages");
        }

        #[test]
        fn test_to_group_suffix() {
            assert_eq!(MessageType::Cast.to_group_suffix(), "casts");
            assert_eq!(MessageType::Reaction.to_group_suffix(), "reactions");
            assert_eq!(MessageType::Link.to_group_suffix(), "links");
            assert_eq!(MessageType::Verification.to_group_suffix(), "verifications");
            assert_eq!(MessageType::UserData.to_group_suffix(), "user_data");
            assert_eq!(MessageType::UsernameProof.to_group_suffix(), "username_proofs");
            // All onchain types share the same group suffix
            assert_eq!(MessageType::OnchainSigner.to_group_suffix(), "onchain");
            assert_eq!(MessageType::OnchainSignerMigrated.to_group_suffix(), "onchain");
            assert_eq!(MessageType::OnchainIdRegister.to_group_suffix(), "onchain");
            assert_eq!(MessageType::OnchainStorageRent.to_group_suffix(), "onchain");
            assert_eq!(MessageType::OnchainTierPurchase.to_group_suffix(), "onchain");
            assert_eq!(MessageType::LendStorage.to_group_suffix(), "lend_storage");
            assert_eq!(MessageType::Messages.to_group_suffix(), "messages");
        }

        #[test]
        fn test_all_returns_all_types() {
            let all_types: Vec<_> = MessageType::all().collect();

            assert_eq!(all_types.len(), 13);
            assert!(all_types.contains(&MessageType::Cast));
            assert!(all_types.contains(&MessageType::Reaction));
            assert!(all_types.contains(&MessageType::Link));
            assert!(all_types.contains(&MessageType::Verification));
            assert!(all_types.contains(&MessageType::UserData));
            assert!(all_types.contains(&MessageType::UsernameProof));
            assert!(all_types.contains(&MessageType::OnchainSigner));
            assert!(all_types.contains(&MessageType::OnchainSignerMigrated));
            assert!(all_types.contains(&MessageType::OnchainIdRegister));
            assert!(all_types.contains(&MessageType::OnchainStorageRent));
            assert!(all_types.contains(&MessageType::OnchainTierPurchase));
            assert!(all_types.contains(&MessageType::LendStorage));
            assert!(all_types.contains(&MessageType::Messages));
        }

        #[test]
        fn test_display() {
            assert_eq!(format!("{}", MessageType::Cast), "cast");
            assert_eq!(format!("{}", MessageType::Reaction), "reaction");
            assert_eq!(format!("{}", MessageType::Link), "link");
            assert_eq!(format!("{}", MessageType::Verification), "verification");
            assert_eq!(format!("{}", MessageType::UserData), "user_data");
            assert_eq!(format!("{}", MessageType::UsernameProof), "username_proof");
            assert_eq!(format!("{}", MessageType::OnchainSigner), "onchain_signer");
            assert_eq!(
                format!("{}", MessageType::OnchainSignerMigrated),
                "onchain_signer_migrated"
            );
            assert_eq!(format!("{}", MessageType::OnchainIdRegister), "onchain_id_register");
            assert_eq!(format!("{}", MessageType::OnchainStorageRent), "onchain_storage_rent");
            assert_eq!(format!("{}", MessageType::OnchainTierPurchase), "onchain_tier_purchase");
            assert_eq!(format!("{}", MessageType::LendStorage), "lend_storage");
            assert_eq!(format!("{}", MessageType::Messages), "messages");
        }

        #[test]
        fn test_serialization() {
            let msg_type = MessageType::Cast;
            let json = serde_json::to_string(&msg_type).unwrap();
            assert_eq!(json, "\"cast\"");

            let deserialized: MessageType = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, msg_type);
        }

        #[test]
        fn test_equality_and_hash() {
            use std::collections::HashSet;

            let t1 = MessageType::Cast;
            let t2 = MessageType::Cast;
            let t3 = MessageType::Reaction;

            assert_eq!(t1, t2);
            assert_ne!(t1, t3);

            let mut set = HashSet::new();
            set.insert(t1);
            set.insert(t2); // duplicate
            set.insert(t3);

            assert_eq!(set.len(), 2);
        }
    }

    mod message_tests {
        use super::*;

        #[test]
        fn test_message_new() {
            let msg = Message::new("id-123", MessageType::Cast, vec![1, 2, 3]);

            assert_eq!(msg.id.value(), "id-123");
            assert_eq!(msg.message_type, MessageType::Cast);
            assert_eq!(msg.payload, vec![1, 2, 3]);
        }

        #[test]
        fn test_message_with_message_id() {
            let id = MessageId::new("msg-id");
            let msg = Message::new(id, MessageType::Reaction, vec![4, 5, 6]);

            assert_eq!(msg.id.value(), "msg-id");
            assert_eq!(msg.message_type, MessageType::Reaction);
        }

        #[test]
        fn test_message_with_empty_payload() {
            let msg = Message::new("empty", MessageType::Link, vec![]);
            assert!(msg.payload.is_empty());
        }

        #[test]
        fn test_message_clone() {
            let msg1 = Message::new("clone-test", MessageType::UserData, vec![7, 8, 9]);
            let msg2 = msg1.clone();

            assert_eq!(msg1.id, msg2.id);
            assert_eq!(msg1.message_type, msg2.message_type);
            assert_eq!(msg1.payload, msg2.payload);
        }
    }

    mod farcaster_epoch_tests {
        use super::*;

        #[test]
        fn test_farcaster_epoch_value() {
            // January 1, 2021 00:00:00 UTC in milliseconds
            assert_eq!(FARCASTER_EPOCH, 1609459200000);
        }

        #[test]
        fn test_farcaster_epoch_is_2021() {
            use std::time::{Duration, UNIX_EPOCH};

            let epoch_time = UNIX_EPOCH + Duration::from_millis(FARCASTER_EPOCH);
            let datetime = chrono::DateTime::<chrono::Utc>::from(epoch_time);

            assert_eq!(datetime.format("%Y-%m-%d").to_string(), "2021-01-01");
        }
    }
}
