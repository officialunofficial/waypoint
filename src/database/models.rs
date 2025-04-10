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
