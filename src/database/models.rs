use chrono::{DateTime, Utc};
use uuid::Uuid;

// Common types
pub type Fid = u64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HashScheme {
    None = 0,
    Blake3 = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignatureScheme {
    None = 0,
    Ed25519 = 1,
    Eip712 = 2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    None = 0,
    CastAdd = 1,
    CastRemove = 2,
    ReactionAdd = 3,
    ReactionRemove = 4,
    LinkAdd = 5,
    LinkRemove = 6,
    VerificationAddEthAddress = 7,
    VerificationRemove = 8,
    UserDataAdd = 11,
    UsernameProof = 12,
    FrameAction = 13,
    LinkCompactState = 14,
    LendStorage = 15,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UserDataType {
    None = 0,
    Pfp = 1,
    Display = 2,
    Bio = 3,
    Url = 5,
    Username = 6,
    Location = 7,
    Twitter = 8,
    Github = 9,
    Banner = 10,
    PrimaryAddressEthereum = 11,
    PrimaryAddressSolana = 12,
    ProfileToken = 13,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    Ethereum = 0,
    Solana = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReactionType {
    None = 0,
    Like = 1,
    Recast = 2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastType {
    Cast = 0,
    LongCast = 1,
    TenKCast = 2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnChainEventType {
    None = 0,
    Signer = 1,
    SignerMigrated = 2,
    IdRegister = 3,
    StorageRent = 4,
    TierPurchase = 5,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TierType {
    None = 0,
    Pro = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignerEventType {
    None = 0,
    Add = 1,
    Remove = 2,
    AdminReset = 3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IdRegisterEventType {
    None = 0,
    Register = 1,
    Transfer = 2,
    ChangeRecovery = 3,
}

// Main Message struct for the database
#[derive(Debug)]
pub struct Message {
    pub id: Uuid,
    pub fid: Fid,
    pub message_type: MessageType,
    pub timestamp: DateTime<Utc>,
    pub network: i32, // FarcasterNetwork enum value
    pub hash_scheme: HashScheme,
    pub signature_scheme: SignatureScheme,
    pub hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub signer: Vec<u8>,
    pub raw: Vec<u8>, // Original protobuf bytes
    pub deleted_at: Option<DateTime<Utc>>,
    pub pruned_at: Option<DateTime<Utc>>,
}

// Row type for SQLx
#[derive(Debug)]
pub struct MessageRow {
    pub id: Uuid,
    pub fid: Fid,
    pub message_type: i32, // Store as integer for easier DB mapping
    pub timestamp: DateTime<Utc>,
    pub network: i32,
    pub hash_scheme: i32,
    pub signature_scheme: i32,
    pub hash: Vec<u8>,
    pub signature: Vec<u8>,
    pub signer: Vec<u8>,
    pub raw: Vec<u8>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub pruned_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct AutoFollow {
    pub id: Uuid,
    pub fid: i64,
    pub added_by: i64,
    pub created_at: DateTime<Utc>,
    pub active: bool,
}

#[derive(Debug)]
pub struct AutoFollowRow {
    pub id: Uuid,
    pub fid: i64,
    pub added_by: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub active: Option<bool>,
}

#[derive(Debug)]
pub struct TierPurchase {
    pub id: Uuid,
    pub fid: Fid,
    pub tier_type: TierType,
    pub for_days: u64,
    pub payer: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub block_hash: Vec<u8>,
    pub log_index: u32,
    pub tx_index: u32,
    pub tx_hash: Vec<u8>,
    pub block_timestamp: DateTime<Utc>,
    pub chain_id: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct SignerEvent {
    pub id: Uuid,
    pub fid: Fid,
    pub key: Vec<u8>,
    pub key_type: u32,
    pub event_type: SignerEventType,
    pub metadata: Option<Vec<u8>>,
    pub metadata_type: Option<u32>,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub block_hash: Vec<u8>,
    pub log_index: u32,
    pub tx_index: u32,
    pub tx_hash: Vec<u8>,
    pub block_timestamp: DateTime<Utc>,
    pub chain_id: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct SignerMigratedEvent {
    pub id: Uuid,
    pub fid: Fid,
    pub migrated_at: u64,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub block_hash: Vec<u8>,
    pub log_index: u32,
    pub tx_index: u32,
    pub tx_hash: Vec<u8>,
    pub block_timestamp: DateTime<Utc>,
    pub chain_id: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct IdRegisterEvent {
    pub id: Uuid,
    pub fid: Fid,
    pub to_address: Vec<u8>,
    pub event_type: IdRegisterEventType,
    pub from_address: Option<Vec<u8>>,
    pub recovery_address: Option<Vec<u8>>,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub block_hash: Vec<u8>,
    pub log_index: u32,
    pub tx_index: u32,
    pub tx_hash: Vec<u8>,
    pub block_timestamp: DateTime<Utc>,
    pub chain_id: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct StorageRentEvent {
    pub id: Uuid,
    pub fid: Fid,
    pub payer: Vec<u8>,
    pub units: u32,
    pub expiry: u32,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
    pub block_hash: Vec<u8>,
    pub log_index: u32,
    pub tx_index: u32,
    pub tx_hash: Vec<u8>,
    pub block_timestamp: DateTime<Utc>,
    pub chain_id: u64,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Represents a spammy user (label_value = 0)
/// These users are completely filtered from the feed
#[derive(Debug, Clone)]
pub struct SpammyUser {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub fid: i64,
    pub source: String,
}

/// Represents a nerfed user (label_value = 3)
/// These users have reduced visibility but are not completely filtered
#[derive(Debug, Clone)]
pub struct NerfedUser {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub fid: i64,
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_tier_purchase_creation() {
        let now = Utc::now();
        let tier_purchase = TierPurchase {
            id: Uuid::new_v4(),
            fid: 12345,
            tier_type: TierType::Pro,
            for_days: 365,
            payer: vec![0x01, 0x02, 0x03],
            timestamp: now,
            block_number: 1000000,
            block_hash: vec![0xaa; 32],
            log_index: 5,
            tx_index: 10,
            tx_hash: vec![0xbb; 32],
            block_timestamp: now,
            chain_id: 10, // Optimism
            deleted_at: None,
        };

        assert_eq!(tier_purchase.fid, 12345);
        assert_eq!(tier_purchase.tier_type, TierType::Pro);
        assert_eq!(tier_purchase.for_days, 365);
        assert_eq!(tier_purchase.payer, vec![0x01, 0x02, 0x03]);
        assert!(tier_purchase.deleted_at.is_none());
    }
}
