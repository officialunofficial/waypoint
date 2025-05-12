use alloy_primitives::{address, Address, Bytes, B256, U256};
use alloy_sol_types::{sol, SolCall};
use crate::eth::EthClient;
use color_eyre::eyre::{Result, eyre};

/// The Signed Key Request Validator contract address on the optimism network
pub const SIGNED_KEY_REQUEST_VALIDATOR_ADDRESS: Address = address!("0x00000000fc700472606ed4fa22623acf62c60553");

/// The Key Gateway contract address on the optimism network
pub const KEY_GATEWAY_ADDRESS: Address = address!("0x00000000fc56947c7e7183f8ca4b62398caadf0b");

sol! {
    function add(
        uint32 keyType, 
        bytes calldata key, 
        uint8 metadataType, 
        bytes calldata metadata
    ) external payable returns (uint256 fid);
}

/// Client for interacting with the Key Gateway smart contract
pub struct KeyGateway<'a> {
    eth_client: &'a EthClient,
}

impl<'a> KeyGateway<'a> {
    /// Creates a new instance of the Key Gateway client
    ///
    /// # Arguments
    ///
    /// * `eth_client` - The Ethereum client to use for contract interactions
    pub fn new(eth_client: &'a EthClient) -> Self {
        Self { eth_client }
    }

    /// Adds a new key to the Key Gateway contract
    ///
    /// # Arguments
    ///
    /// * `key` - The key bytes to add
    /// * `metadata` - The metadata bytes associated with the key
    ///
    /// # Returns
    ///
    /// The transaction hash as a B256
    pub async fn add(
        &self, 
        key: Bytes,
        metadata: Bytes,
    ) -> Result<B256> {
        let call = addCall {
            keyType: 1,
            key,
            metadataType: 1,
            metadata,
        };
        let calldata = call.abi_encode();

        tracing::info!("Add calldata: 0x{}", hex::encode(&calldata));

        let tx_hash = self.eth_client
            .send_transaction(KEY_GATEWAY_ADDRESS, calldata.into(), U256::ZERO)
            .await
            .map_err(|e| {
                tracing::error!("Failed to send transaction: {}", e);
                eyre!("Failed to send transaction: {}", e)
            })?;

        tracing::info!("Transaction submitted: 0x{}", hex::encode(&tx_hash));
        Ok(B256::from_slice(&tx_hash[0..32]))
    }
}