//! Data providers for the Farcaster Hub

use crate::core::{
    data_context::{DataAccessError, HubClient, Result},
    types::{Fid, Message, MessageType},
};
use crate::hub::client::Hub;
use crate::proto::{FidRequest, UserDataRequest, UserDataType as ProtoUserDataType};
use async_trait::async_trait;
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// Farcaster Hub data provider
#[derive(Clone)]
pub struct FarcasterHubClient {
    hub: Arc<Mutex<Hub>>,
}

impl std::fmt::Debug for FarcasterHubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FarcasterHubClient").field("hub", &"<Arc<Mutex<Hub>>>").finish()
    }
}

impl FarcasterHubClient {
    /// Create a new Hub client implementation
    pub fn new(hub: Arc<Mutex<Hub>>) -> Self {
        Self { hub }
    }

    /// Format a hex string from bytes
    fn format_hex(bytes: &[u8]) -> String {
        let mut hash_string = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            let _ = write!(hash_string, "{:02x}", byte);
        }
        hash_string
    }
}

#[async_trait]
impl HubClient for FarcasterHubClient {
    async fn get_user_data_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        info!("Fetching user data for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the request
        let request = FidRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: None,
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_user_data_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                // Convert proto message to domain message
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::UserData,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    async fn get_user_data(&self, fid: Fid, data_type: &str) -> Result<Option<Message>> {
        info!("Fetching specific user data for FID: {} and type: {}", fid, data_type);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Convert string data type to proto enum
        let user_data_type = match data_type.to_lowercase().as_str() {
            "pfp" => ProtoUserDataType::Pfp,
            "display" => ProtoUserDataType::Display,
            "bio" => ProtoUserDataType::Bio,
            "url" => ProtoUserDataType::Url,
            "username" => ProtoUserDataType::Username,
            _ => {
                return Err(DataAccessError::Other(format!(
                    "Invalid user data type: {}",
                    data_type
                )));
            },
        };

        // Create the request
        let request = UserDataRequest { fid: fid.value(), user_data_type: user_data_type.into() };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_user_data(tonic::Request::new(request))
            .await;

        match response {
            Ok(proto_msg) => {
                let proto_msg = proto_msg.into_inner();
                // Convert proto message to domain message
                Ok(Some(Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::UserData,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )))
            },
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(DataAccessError::HubClient(e.to_string())),
        }
    }

    async fn get_username_proofs_by_fid(&self, fid: Fid) -> Result<Vec<Message>> {
        info!("Fetching username proofs for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the request
        let request = FidRequest {
            fid: fid.value(),
            page_size: Some(10), // Reasonable default
            page_token: None,
            reverse: None,
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_user_name_proofs_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .proofs
            .into_iter()
            .map(|proof| {
                // Convert username proof to domain message
                // We store the serialized proof in the payload
                let payload = serde_json::to_vec(&proof)
                    .map_err(|e| DataAccessError::Serialization(e.to_string()))
                    .unwrap_or_default();

                Message::new(
                    format!("proof_{}_{}", fid.value(), String::from_utf8_lossy(&proof.name)),
                    MessageType::UsernameProof,
                    payload,
                )
            })
            .collect();

        Ok(messages)
    }

    async fn get_verifications_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        info!("Fetching verifications for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the request
        let request = FidRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: None,
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_verifications_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                // Convert proto message to domain message
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Verification,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get casts by FID with pagination support
    async fn get_casts_by_fid(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        info!("Fetching casts for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the request - reuse FidRequest which is what the API actually expects
        let request = FidRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest casts first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_casts_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                // Convert proto message to domain message
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get a specific cast by ID
    async fn get_cast(&self, fid: Fid, hash: &[u8]) -> Result<Option<Message>> {
        info!("Fetching specific cast with FID: {} and hash: {}", fid, hex::encode(hash));
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create CastId object
        let cast_id = crate::proto::CastId { fid: fid.value(), hash: hash.to_vec() };

        // Make the RPC call
        let result = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_cast(tonic::Request::new(cast_id))
            .await;

        match result {
            Ok(response) => {
                let proto_msg = response.into_inner();
                Ok(Some(Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )))
            },
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(DataAccessError::HubClient(e.to_string())),
        }
    }

    /// Get casts mentioning an FID
    async fn get_casts_by_mention(&self, fid: Fid, limit: usize) -> Result<Vec<Message>> {
        info!("Fetching casts mentioning FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create FidRequest which is what the API expects
        let request = FidRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest mentions first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_casts_by_mention(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get casts by parent
    async fn get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash: &[u8],
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!(
            "Fetching casts with parent FID: {} and hash: {}",
            parent_fid,
            hex::encode(parent_hash)
        );
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create parent CastId
        let parent_cast_id =
            crate::proto::CastId { fid: parent_fid.value(), hash: parent_hash.to_vec() };

        // Create CastsByParentRequest
        let request = crate::proto::CastsByParentRequest {
            parent: Some(crate::proto::casts_by_parent_request::Parent::ParentCastId(
                parent_cast_id,
            )),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest replies first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_casts_by_parent(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get casts by parent URL
    async fn get_casts_by_parent_url(
        &self,
        parent_url: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!("Fetching casts with parent URL: {}", parent_url);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create CastsByParentRequest with URL
        let request = crate::proto::CastsByParentRequest {
            parent: Some(crate::proto::casts_by_parent_request::Parent::ParentUrl(
                parent_url.to_string(),
            )),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest replies first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_casts_by_parent(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get all casts by FID with timestamp filtering
    async fn get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<Message>> {
        info!("Fetching all casts for FID: {} with timestamp filtering", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create FidTimestampRequest
        let request = crate::proto::FidTimestampRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true),         // Get newest casts first
            start_timestamp: start_time, // Optional start time
            stop_timestamp: end_time,    // Optional end time
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_all_cast_messages_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Cast,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get a specific reaction by params
    async fn get_reaction(
        &self,
        fid: Fid,
        reaction_type: u8,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
    ) -> Result<Option<Message>> {
        info!("Fetching specific reaction for FID: {} with type: {}", fid, reaction_type);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the target (either cast ID or URL)
        let target = match (target_cast_fid, target_cast_hash, target_url) {
            (Some(target_fid), Some(hash), _) => {
                Some(crate::proto::reaction_request::Target::TargetCastId(crate::proto::CastId {
                    fid: target_fid.value(),
                    hash: hash.to_vec(),
                }))
            },
            (_, _, Some(url)) => {
                Some(crate::proto::reaction_request::Target::TargetUrl(url.to_string()))
            },
            _ => {
                return Err(DataAccessError::Other(
                    "Either target cast or URL must be provided".to_string(),
                ));
            },
        };

        // Create ReactionRequest
        let reaction_type_enum = match reaction_type {
            1 => crate::proto::ReactionType::Like,
            2 => crate::proto::ReactionType::Recast,
            _ => {
                return Err(DataAccessError::Other(format!(
                    "Invalid reaction type: {}",
                    reaction_type
                )));
            },
        };

        let request = crate::proto::ReactionRequest {
            fid: fid.value(),
            reaction_type: reaction_type_enum.into(),
            target,
        };

        // Make the RPC call
        let result = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_reaction(tonic::Request::new(request))
            .await;

        match result {
            Ok(response) => {
                let proto_msg = response.into_inner();
                Ok(Some(Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Reaction,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )))
            },
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(DataAccessError::HubClient(e.to_string())),
        }
    }

    /// Get reactions by FID
    async fn get_reactions_by_fid(
        &self,
        fid: Fid,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!("Fetching reactions for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Convert reaction type
        let reaction_type_enum = reaction_type.map(|rt| match rt {
            1 => crate::proto::ReactionType::Like,
            2 => crate::proto::ReactionType::Recast,
            _ => crate::proto::ReactionType::Like, // Default to like
        });

        // Create ReactionsByFidRequest
        let request = crate::proto::ReactionsByFidRequest {
            fid: fid.value(),
            reaction_type: reaction_type_enum.map(|rt| rt as i32),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest reactions first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_reactions_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Reaction,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get reactions by target (cast or URL)
    async fn get_reactions_by_target(
        &self,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
        reaction_type: Option<u8>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!("Fetching reactions by target");
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create the target (either cast ID or URL)
        let target = match (target_cast_fid, target_cast_hash, target_url) {
            (Some(target_fid), Some(hash), _) => {
                Some(crate::proto::reactions_by_target_request::Target::TargetCastId(
                    crate::proto::CastId { fid: target_fid.value(), hash: hash.to_vec() },
                ))
            },
            (_, _, Some(url)) => {
                Some(crate::proto::reactions_by_target_request::Target::TargetUrl(url.to_string()))
            },
            _ => {
                return Err(DataAccessError::Other(
                    "Either target cast or URL must be provided".to_string(),
                ));
            },
        };

        // Convert reaction type
        let reaction_type_enum = reaction_type.map(|rt| match rt {
            1 => crate::proto::ReactionType::Like,
            2 => crate::proto::ReactionType::Recast,
            _ => crate::proto::ReactionType::Like, // Default to like
        });

        // Create ReactionsByTargetRequest
        let request = crate::proto::ReactionsByTargetRequest {
            target,
            reaction_type: reaction_type_enum.map(|rt| rt as i32),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest reactions first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_reactions_by_target(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Reaction,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get all reactions by FID with timestamp filtering
    async fn get_all_reactions_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<Message>> {
        info!("Fetching all reactions for FID: {} with timestamp filtering", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create FidTimestampRequest
        let request = crate::proto::FidTimestampRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true),         // Get newest reactions first
            start_timestamp: start_time, // Optional start time
            stop_timestamp: end_time,    // Optional end time
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_all_reaction_messages_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Reaction,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get a specific link by params
    async fn get_link(
        &self,
        fid: Fid,
        link_type: &str,
        target_fid: Fid,
    ) -> Result<Option<Message>> {
        info!(
            "Fetching specific link for FID: {} with type: {} and target: {}",
            fid, link_type, target_fid
        );
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create LinkRequest
        let request = crate::proto::LinkRequest {
            fid: fid.value(),
            link_type: link_type.to_string(),
            target: Some(crate::proto::link_request::Target::TargetFid(target_fid.value())),
        };

        // Make the RPC call
        let result = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_link(tonic::Request::new(request))
            .await;

        match result {
            Ok(response) => {
                let proto_msg = response.into_inner();
                Ok(Some(Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Link,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )))
            },
            Err(status) if status.code() == tonic::Code::NotFound => Ok(None),
            Err(e) => Err(DataAccessError::HubClient(e.to_string())),
        }
    }

    /// Get links by FID
    async fn get_links_by_fid(
        &self,
        fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!("Fetching links for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create LinksByFidRequest
        let request = crate::proto::LinksByFidRequest {
            fid: fid.value(),
            link_type: link_type.map(|lt| lt.to_string()),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest links first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_links_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Link,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get links by target
    async fn get_links_by_target(
        &self,
        target_fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        info!("Fetching links by target FID: {}", target_fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create LinksByTargetRequest
        let request = crate::proto::LinksByTargetRequest {
            target: Some(crate::proto::links_by_target_request::Target::TargetFid(
                target_fid.value(),
            )),
            link_type: link_type.map(|lt| lt.to_string()),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true), // Get newest links first
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_links_by_target(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Link,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get link compact state messages by FID
    async fn get_link_compact_state_by_fid(&self, fid: Fid) -> Result<Vec<Message>> {
        info!("Fetching link compact state for FID: {}", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create FidRequest
        let request = crate::proto::FidRequest {
            fid: fid.value(),
            page_size: None,
            page_token: None,
            reverse: None,
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_link_compact_state_message_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Link,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }

    /// Get all links by FID with timestamp filtering
    async fn get_all_links_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<Vec<Message>> {
        info!("Fetching all links for FID: {} with timestamp filtering", fid);
        let mut hub = self.hub.lock().await;

        // Ensure hub is connected
        if !hub.check_connection().await.map_err(|e| DataAccessError::HubClient(e.to_string()))? {
            return Err(DataAccessError::HubClient("Hub not connected".to_string()));
        }

        // Create FidTimestampRequest
        let request = crate::proto::FidTimestampRequest {
            fid: fid.value(),
            page_size: Some(limit as u32),
            page_token: None,
            reverse: Some(true),         // Get newest links first
            start_timestamp: start_time, // Optional start time
            stop_timestamp: end_time,    // Optional end time
        };

        // Make the RPC call
        let response = hub
            .client()
            .ok_or_else(|| DataAccessError::HubClient("Hub client not initialized".to_string()))?
            .get_all_link_messages_by_fid(tonic::Request::new(request))
            .await
            .map_err(|e| DataAccessError::HubClient(e.to_string()))?
            .into_inner();

        // Convert the response to domain messages
        let messages = response
            .messages
            .into_iter()
            .map(|proto_msg| {
                Message::new(
                    Self::format_hex(&proto_msg.hash),
                    MessageType::Link,
                    proto_msg.data.map(|d| d.encode_to_vec()).unwrap_or_default(),
                )
            })
            .collect();

        Ok(messages)
    }
}
