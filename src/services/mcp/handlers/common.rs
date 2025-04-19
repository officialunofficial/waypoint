//! Common request types for MCP handlers

use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

/// Default limit for most requests
pub(crate) fn default_limit() -> usize {
    10
}

/// Default link type is "follow"
pub(crate) fn default_link_type() -> String {
    "follow".to_string()
}

/// Request for a user by FID
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UserByFidRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
}

/// Request for a list of casts
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCastsRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for a list of verifications
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetVerificationsRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for a specific cast
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCastRequest {
    #[schemars(description = "Farcaster user ID of the cast author")]
    pub fid: u64,
    #[schemars(description = "Hash of the cast in hex format")]
    pub hash: String,
}

/// Request for casts mentioning a user
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCastMentionsRequest {
    #[schemars(description = "Farcaster user ID being mentioned")]
    pub fid: u64,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for cast replies
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCastRepliesRequest {
    #[schemars(description = "Farcaster user ID of the parent cast author")]
    pub parent_fid: u64,
    #[schemars(description = "Hash of the parent cast in hex format")]
    pub parent_hash: String,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for cast replies by URL
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCastRepliesByUrlRequest {
    #[schemars(description = "URL of the parent content")]
    pub parent_url: String,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for casts with timestamp filtering
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAllCastsWithTimeRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[schemars(description = "Start timestamp (optional)")]
    pub start_time: Option<u64>,
    #[schemars(description = "End timestamp (optional)")]
    pub end_time: Option<u64>,
}

/// Request for a specific reaction
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReactionRequest {
    #[schemars(description = "Farcaster user ID of the reaction author")]
    pub fid: u64,
    #[schemars(description = "Reaction type (1 = Like, 2 = Recast)")]
    pub reaction_type: u8,
    #[schemars(description = "Target cast FID (required if target_cast_hash is provided)")]
    pub target_cast_fid: Option<u64>,
    #[schemars(
        description = "Target cast hash in hex format (required if target_cast_fid is provided)"
    )]
    pub target_cast_hash: Option<String>,
    #[schemars(description = "Target URL (alternative to target_cast_fid and target_cast_hash)")]
    pub target_url: Option<String>,
}

/// Request for reactions by FID
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReactionsByFidRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(description = "Reaction type (1 = Like, 2 = Recast, omit for all types)")]
    pub reaction_type: Option<u8>,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for reactions by target
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReactionsByTargetRequest {
    #[schemars(description = "Target cast FID (required if target_cast_hash is provided)")]
    pub target_cast_fid: Option<u64>,
    #[schemars(
        description = "Target cast hash in hex format (required if target_cast_fid is provided)"
    )]
    pub target_cast_hash: Option<String>,
    #[schemars(description = "Target URL (alternative to target_cast_fid and target_cast_hash)")]
    pub target_url: Option<String>,
    #[schemars(description = "Reaction type (1 = Like, 2 = Recast, omit for all types)")]
    pub reaction_type: Option<u8>,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for a specific link
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkRequest {
    #[schemars(description = "Farcaster user ID of the link author")]
    pub fid: u64,
    #[schemars(description = "Link type (e.g., 'follow')")]
    #[serde(default = "default_link_type")]
    pub link_type: String,
    #[schemars(description = "Target Farcaster user ID")]
    pub target_fid: u64,
}

/// Request for links by FID
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinksByFidRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(
        description = "Link type (defaults to 'follow' if not specified, use null for all types)"
    )]
    #[serde(default)]
    pub link_type: Option<String>,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for links by target
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinksByTargetRequest {
    #[schemars(description = "Target Farcaster user ID")]
    pub target_fid: u64,
    #[schemars(
        description = "Link type (defaults to 'follow' if not specified, use null for all types)"
    )]
    #[serde(default)]
    pub link_type: Option<String>,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Request for FID with timestamp constraints
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FidTimestampRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
    #[schemars(description = "Maximum number of results to return")]
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[schemars(description = "Start timestamp (optional)")]
    pub start_time: Option<u64>,
    #[schemars(description = "End timestamp (optional)")]
    pub end_time: Option<u64>,
}

/// Request for FID
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FidRequest {
    #[schemars(description = "Farcaster user ID")]
    pub fid: u64,
}

/// Request to get FID by username
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFidByUsernameRequest {
    #[schemars(description = "Farcaster username (without the @ symbol)")]
    pub username: String,
}
