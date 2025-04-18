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
}
