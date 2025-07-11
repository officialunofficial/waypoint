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
