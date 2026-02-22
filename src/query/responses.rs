//! Typed query response DTOs shared across adapters.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct UserProfile {
    pub fid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pfp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub twitter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UserNotFound {
    pub fid: u64,
    pub found: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum UserByFidResponse {
    Found(UserProfile),
    NotFound(UserNotFound),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UserByUsernameNotFound {
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fid: Option<u64>,
    pub found: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum UserByUsernameResponse {
    Found(UserProfile),
    NotFound(UserByUsernameNotFound),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FidByUsernameResponse {
    pub username: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Verification {
    pub fid: u64,
    pub address: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub verification_type: Option<String>,
    pub action: String,
    pub timestamp: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerificationsByFidResponse {
    pub fid: u64,
    pub count: usize,
    pub verifications: Vec<Verification>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerificationLookupFound {
    pub fid: u64,
    pub address: String,
    pub found: bool,
    pub verification: Verification,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerificationLookupNotFound {
    pub fid: u64,
    pub address: String,
    pub found: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum VerificationLookupResponse {
    Found(VerificationLookupFound),
    NotFound(VerificationLookupNotFound),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AllVerificationMessagesResponse {
    pub fid: u64,
    pub count: usize,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub verifications: Vec<Verification>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsernameProof {
    pub name: String,
    #[serde(rename = "type")]
    pub proof_type: String,
    pub fid: u64,
    pub timestamp: u64,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsernameProofsByFidResponse {
    pub fid: u64,
    pub count: usize,
    pub proofs: Vec<UsernameProof>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsernameProofByNameResponse {
    pub name: String,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub proof_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastReference {
    pub fid: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TypedCastReference {
    #[serde(rename = "type")]
    pub target_type: String,
    pub fid: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TypedUrlReference {
    #[serde(rename = "type")]
    pub target_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CastParent {
    Cast(TypedCastReference),
    Url(TypedUrlReference),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CastEmbed {
    Cast(TypedCastReference),
    Url(TypedUrlReference),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Cast {
    pub fid: u64,
    pub timestamp: u32,
    pub hash: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions_positions: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<CastEmbed>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<CastParent>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastNotFoundResponse {
    pub fid: u64,
    pub hash: String,
    pub found: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CastLookupResponse {
    Found(Cast),
    NotFound(CastNotFoundResponse),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastsByFidResponse {
    pub fid: u64,
    pub count: usize,
    pub casts: Vec<Cast>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastsByMentionResponse {
    pub mention_fid: u64,
    pub count: usize,
    pub casts: Vec<Cast>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastsByParentResponse {
    pub parent: CastReference,
    pub count: usize,
    pub replies: Vec<Cast>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CastsByParentUrlResponse {
    pub parent_url: String,
    pub count: usize,
    pub replies: Vec<Cast>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConversationParentCast {
    pub fid: u64,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConversationCast {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ConversationParentCast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<CastEmbed>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quoted_casts: Option<Vec<ConversationCast>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<ConversationCast>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more_replies: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConversationTree {
    pub replies: Vec<ConversationCast>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConversationParticipants {
    pub count: usize,
    pub fids: Vec<String>,
    pub user_data: BTreeMap<String, UserProfile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConversationFoundResponse {
    pub root_cast: ConversationCast,
    pub participants: ConversationParticipants,
    pub topic: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_casts: Option<Vec<ConversationCast>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quoted_casts: Option<Vec<ConversationCast>>,
    pub conversation: ConversationTree,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ConversationResponse {
    Found(Box<ConversationFoundResponse>),
    NotFound(CastNotFoundResponse),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ReactionTarget {
    Cast(TypedCastReference),
    Url(TypedUrlReference),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Reaction {
    pub fid: u64,
    pub timestamp: u32,
    pub hash: String,
    pub reaction_type: String,
    pub reaction_type_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReactionTarget>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReactionNotFoundResponse {
    pub fid: u64,
    pub reaction_type_id: i32,
    pub found: bool,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_cast: Option<CastReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ReactionLookupResponse {
    Found(Reaction),
    NotFound(ReactionNotFoundResponse),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReactionsByFidResponse {
    pub fid: u64,
    pub count: usize,
    pub reactions: Vec<Reaction>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReactionsByTargetResponse {
    pub count: usize,
    pub reactions: Vec<Reaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_cast: Option<CastReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Link {
    pub fid: u64,
    pub timestamp: u32,
    pub hash: String,
    pub link_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_fid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_timestamp: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinkNotFoundResponse {
    pub fid: u64,
    pub link_type: String,
    pub target_fid: u64,
    pub found: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LinkLookupResponse {
    Found(Link),
    NotFound(LinkNotFoundResponse),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinksByFidResponse {
    pub fid: u64,
    pub count: usize,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinksByTargetResponse {
    pub target_fid: u64,
    pub count: usize,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinkCompactStateResponse {
    pub fid: u64,
    pub count: usize,
    pub compact_links: Vec<Link>,
}
