use color_eyre::eyre::{Result, eyre};
use crate::proto::hub_service_client::HubServiceClient;
use crate::proto::{HashScheme, Message, MessageData, SignatureScheme};
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use super::KeyManager;

/// Client for sending messages to the Farcaster hub
#[derive(Clone)]
pub struct SendMessageClient {
    key_manager: Arc<KeyManager>,
    hub_client: Arc<Mutex<HubServiceClient<Channel>>>,
}

impl SendMessageClient {
    /// Creates a new SendMessageClient instance
    ///
    /// # Arguments
    ///
    /// * `key_manager` - The key manager for signing messages
    /// * `hub_client` - The client for communicating with the Farcaster hub
    pub fn new(key_manager: KeyManager, hub_client: HubServiceClient<Channel>) -> Self {
        Self { 
            key_manager: Arc::new(key_manager), 
            hub_client: Arc::new(Mutex::new(hub_client))
        }
    }

    /// Sends a message to the Farcaster hub
    ///
    /// # Arguments
    ///
    /// * `message_data` - The message data to send
    ///
    /// # Returns
    ///
    /// A Result indicating success or failure of the operation
    pub async fn send_message(&mut self, message_data: MessageData) -> Result<()> {
        let message = self.create_ed25519_signed_message(message_data).await?;
        self.hub_client.lock().await
            .submit_message(message)
            .await
            .map_err(|e| eyre!("Failed to submit message to hub: {}", e))?;
            
        Ok(())
    }

    /// Creates an Ed25519 signed message from the given message data
    ///
    /// # Arguments
    ///
    /// * `message_data` - The message data to sign
    ///
    /// # Returns
    ///
    /// A Result containing the signed message or an error
    async fn create_ed25519_signed_message(&self, message_data: MessageData) -> Result<Message> {
        let data_bytes = message_data.encode_to_vec();
        let hash_full = blake3::hash(&data_bytes);
        let hash = hash_full.as_bytes()[..20].to_vec();
        let signature = self.key_manager.sign(&hash);
        println!("Signature: {:?}", hex::encode(&signature));
        Ok(Message {
            data: Some(message_data),
            hash,
            hash_scheme: HashScheme::Blake3 as i32,
            signature,
            signature_scheme: SignatureScheme::Ed25519 as i32,
            signer: self.key_manager.verifying_key_bytes(),
            data_bytes: Some(data_bytes),
        })
    }
}
