use std::sync::Arc;

use alloy_primitives::{Address, Bytes, U256};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{eip712_domain, sol, Eip712Domain, SolValue};
use color_eyre::eyre::{Result, eyre};
use tracing::info;
use crate::{
    contracts::{
        id_gateway::IdGateway, 
        id_registry::IdRegistry, 
        key_gateway::{KeyGateway, SIGNED_KEY_REQUEST_VALIDATOR_ADDRESS}
    }, 
    eth::EthClient
};
use super::{get_deadline_timestamp, KeyManager};
use tokio::time::{sleep, Duration};

sol! {
    /// Metadata for a signed key request
    struct SignedKeyRequestMetadata {
        uint256 requestFid;
        address requestSigner;
        bytes signature;
        uint256 deadline;
    }
 
    /// A request to register a new signer key
    struct SignedKeyRequest {
        uint256 requestFid;
        bytes key;
        uint256 deadline;
    }
}

/// Client for registering Farcaster IDs (FIDs)
pub struct RegisterFID {
    eth_client: Arc<EthClient>,
    key_manager: Arc<KeyManager>,
    signer: Arc<PrivateKeySigner>,
}

impl RegisterFID {
    /// Creates a new RegisterFID instance
    ///
    /// # Arguments
    ///
    /// * `eth_client` - The Ethereum client for blockchain interactions
    /// * `key_manager` - The key manager for signing operations
    /// * `signer` - The signer for EIP-712 typed data
    pub fn new(eth_client: EthClient, key_manager: KeyManager, signer: PrivateKeySigner) -> Self {
        Self { 
            eth_client: Arc::new(eth_client),
            key_manager: Arc::new(key_manager),
            signer: Arc::new(signer)
        }
    }

    /// Registers a new FID for the given owner address
    ///
    /// # Arguments
    ///
    /// * `owner_address` - The address that will own the FID
    ///
    /// # Returns
    ///
    /// The registered FID
    pub async fn register_fid(&self, owner_address: Address) -> Result<U256> {
        let mut fid = self.get_fid_for_address(owner_address).await?;
        
        if fid == U256::ZERO {
            fid = self.register_fid_for_address(owner_address).await?;

        }
        // Register the signer for the FID
        self.register_signer_for_fid(fid, owner_address).await?;   
        Ok(fid)
    }

    /// Gets the FID for a given address
    async fn get_fid_for_address(&self, owner_address: Address) -> Result<U256> {
        let id_registry = IdRegistry::new(&self.eth_client);
        id_registry.id_of(owner_address).await
    }

    /// Registers a new FID for the given address
    async fn register_fid_for_address(&self, owner_address: Address) -> Result<U256> {
        let id_gateway = IdGateway::new(&self.eth_client);
        let price = id_gateway.price(U256::ZERO).await?;
        
        let balance = self.eth_client.get_balance(owner_address).await?;
        if balance < price {
            return Err(eyre!("Insufficient balance: required {} wei, got {} wei", price, balance));
        }   

        let hash = id_gateway.register(owner_address, Some(price)).await?;
        self.eth_client.wait_for_transaction_receipt(hash).await?;
        
        self.get_fid_for_address(owner_address).await
    }

    /// Registers a signer key for the given FID
    async fn register_signer_for_fid(&self, fid: U256, owner_address: Address) -> Result<()> {
        let key_gateway = KeyGateway::new(&self.eth_client);
        let key = self.key_manager.public_key_bytes();
        
        let signed_key_request_metadata = self.create_register_signer_request(fid, owner_address).await?;
        let hash = key_gateway.add(
            key, 
            signed_key_request_metadata.abi_encode().into()
        ).await?;
        
        self.eth_client.wait_for_transaction_receipt(hash).await?;
        // Wait for the transaction to be processed
        sleep(Duration::from_secs(10)).await;
        
        info!("Registered signer for FID: {}", fid);
        Ok(())
    }

    /// Gets the EIP-712 domain for signed key request validation
    fn get_eip_domain_for_signed_key_request_validation(&self) -> Result<Eip712Domain> {
        Ok(eip712_domain! {
            name: "Farcaster SignedKeyRequestValidator",
            version: "1",
            chain_id: 10,
            verifying_contract: SIGNED_KEY_REQUEST_VALIDATOR_ADDRESS,
        })
    }

    /// Creates a signed key request for registering a new signer
    async fn create_register_signer_request(&self, fid: U256, owner_address: Address) -> Result<SignedKeyRequestMetadata> {
        let key = self.key_manager.public_key_bytes();
        let key_validity: alloy_primitives::Uint<256, 4> = get_deadline_timestamp(3600); // one hour from now
        
        let data = SignedKeyRequest {
            requestFid: fid,
            key,
            deadline: key_validity,
        };

        let domain = self.get_eip_domain_for_signed_key_request_validation()?;
        let signed_key_request_signature = self.signer.sign_typed_data(&data, &domain).await?;

        Ok(SignedKeyRequestMetadata {
            requestFid: fid,
            requestSigner: owner_address,
            signature: Bytes::from(signed_key_request_signature.as_bytes()),
            deadline: key_validity,
        })
    }
}