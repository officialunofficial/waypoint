use std::sync::Arc;
use alloy_primitives::{address, Address, U256};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{eip712_domain, sol, Eip712Domain};
use color_eyre::eyre::{Result, eyre};
use reqwest::Client;
use serde::Serialize;
use tracing::info;
use tokio::sync::Mutex;
use serde_json;
use crate::farcaster::get_current_timestamp;
use crate::proto::{
    MessageData, 
    MessageType, 
    message_data::Body, 
    FarcasterNetwork, 
    UserDataBody, 
    UserDataType
};
use super::{get_current_farcaster_timestamp, SendMessageClient};

/// Farcaster name verification contract address
pub const FNAME_VERIFICATION_ADDRESS: Address = address!("0xe3be01d99baa8db9905b33a3ca391238234b79d1");
/// Farcaster names service URL
pub const FARCASTER_NAMES_URL: &str = "https://fnames.farcaster.xyz/transfers";

sol! {
    /// Proof structure for username verification
    #[derive(Debug)]
    struct UserNameProof {
        string name;
        uint256 timestamp;
        address owner;
    }
}

/// Request structure for registering a Farcaster name
#[derive(Serialize)]
struct RegisterNameRequest {
    from: u64,
    to: u64,
    fid: u64,
    name: String,
    timestamp: u64,
    owner: String,
    signature: String,
}

/// Client for registering Farcaster names
pub struct RegisterFname {
    http_client: Arc<Client>,
    signer: Arc<PrivateKeySigner>,
    send_message_client: Arc<Mutex<SendMessageClient>>,
}

impl RegisterFname {
    /// Creates a new RegisterFname instance
    ///
    /// # Arguments
    ///
    /// * `signer` - The signer for EIP-712 typed data
    /// * `send_message_client` - The client for sending messages to the hub
    pub fn new(
        signer: PrivateKeySigner, 
        send_message_client: SendMessageClient,
    ) -> Self {
        Self { 
            http_client: Arc::new(Client::new()),
            signer: Arc::new(signer),
            send_message_client: Arc::new(Mutex::new(send_message_client))
        }
    }

    /// Registers a new Farcaster name for the given FID
    ///
    /// # Arguments
    ///
    /// * `fid` - The Farcaster ID to register the name for
    /// * `username` - The username to register
    pub async fn register_fname(&self, fid: U256, username: String) -> Result<()> {
        self.check_if_fname_is_available(&username).await?;
        self.register_fname_to_fid(fid, &username).await?;
        self.submit_fname_message(fid, username).await?;
        Ok(())
    }

    /// Checks if a Farcaster name is available
    async fn check_if_fname_is_available(&self, username: &str) -> Result<()> {
        let response = self.http_client
            .get(format!("{}?name={}", FARCASTER_NAMES_URL, username))
            .send()
            .await
            .map_err(|e| eyre!("Failed to check username availability: {}", e))?;

        let json: serde_json::Value = response.json()
            .await
            .map_err(|e| eyre!("Failed to parse availability response: {}", e))?;
        
        info!("Username availability response: {:?}", json);

        if let Some(transfers) = json.get("transfers") {
            if let Some(transfers_array) = transfers.as_array() {
                if !transfers_array.is_empty() {
                    return Err(eyre!("Username '{}' is already taken", username));
                }
            }
        }
        
        info!("Username '{}' is available", username);
        Ok(())
    }

    /// Registers a Farcaster name to a FID
    async fn register_fname_to_fid(&self, fid: U256, username: &str) -> Result<()> {
        let payload = self.create_register_name_request(fid, username).await?;
        
        let response = self.http_client
            .post(FARCASTER_NAMES_URL)
            .json(&payload)
            .send()
            .await
            .map_err(|e| eyre!("Failed to send username registration request: {}", e))?;

        let status = response.status();
        let json: serde_json::Value = response.json()
            .await
            .map_err(|e| eyre!("Failed to parse registration response: {}", e))?;

        info!("Username registration response: {:?}", json);

        if !status.is_success() {
            return Err(eyre!("Fname registration failed with status {}: {:?}", status, json));
        }
        
        Ok(())
    }
    
    /// Gets the EIP-712 domain for name verification
    fn get_eip_domain_for_name_verification(&self) -> Result<Eip712Domain> {
        Ok(eip712_domain! {
            name: "Farcaster name verification",
            version: "1",
            chain_id: 1,
            verifying_contract: FNAME_VERIFICATION_ADDRESS,
        })
    }

    /// Creates a request to register a Farcaster name
    async fn create_register_name_request(&self, fid: U256, username: &str) -> Result<RegisterNameRequest> {
        let timestamp = get_current_timestamp();
    
        let data = UserNameProof {
            name: username.to_string(),
            timestamp: U256::from(timestamp),
            owner: self.signer.address(),
        };

        info!("Signing data: {:?}", data);
        let eip_domain = self.get_eip_domain_for_name_verification()?;
        let hash = self.signer.sign_typed_data(&data, &eip_domain)
            .await
            .map_err(|e| eyre!("Failed to sign typed data: {}", e))?;

        let signature = hash.as_bytes().to_vec();
        let eip712_signature = hex::encode(signature);
        let formatted_eip712_signature = format!("0x{}", eip712_signature);

        println!("formatted_eip712_signature: {}", formatted_eip712_signature);

        let fid_u64 = fid.to_string().parse::<u64>()
            .map_err(|e| eyre!("Failed to parse FID: {}", e))?;

        Ok(RegisterNameRequest {
            name: username.to_string(),
            from: 0,
            to: fid_u64,
            fid: fid_u64,
            owner: self.signer.address().to_string(),
            timestamp,
            signature: formatted_eip712_signature
        })
    }

    /// Submits a Farcaster name message to the hub
    async fn submit_fname_message(&self, fid: U256, username: String) -> Result<()> {
        let message_data = self.create_useradd_message_data(fid, username)?;
        self.send_message_client.lock().await.send_message(message_data).await?;
        Ok(())
    }

    /// Creates a user data message for adding a username
    fn create_useradd_message_data(&self, fid: U256, username: String) -> Result<MessageData> {
        let fid_u64 = fid.to_string().parse::<u64>()
            .map_err(|e| eyre!("Failed to parse FID for useradd message: {}", e))?;

        Ok(MessageData {
            r#type: MessageType::UserDataAdd as i32,
            fid: fid_u64,
            timestamp: get_current_farcaster_timestamp() as u32,
            network: FarcasterNetwork::Mainnet as i32,
            body: Some(Body::UserDataBody(UserDataBody {
                r#type: UserDataType::Username as i32,
                value: username,
            }))
        })
    }
}