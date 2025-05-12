use alloy_primitives::{address, Address, B256, U256};
use alloy_sol_types::{sol, SolCall};
use crate::eth::EthClient;
use color_eyre::eyre::{Result, eyre};

/// The ID Gateway contract address on the optimism network
pub const ID_GATEWAY_ADDRESS: Address = address!("0x00000000fc25870c6ed6b6c7e41fb078b7656f69");

sol! {
    function price(uint256 extraStorage) external view returns (uint256);
    function register(address recovery) external payable returns (uint256 fid);
    function nonces(address owner) external view returns (uint256);
}

/// Client for interacting with the ID Gateway smart contract
pub struct IdGateway<'a> {
    eth_client: &'a EthClient
}

impl<'a> IdGateway<'a> {
    /// Creates a new instance of the ID Gateway client
    ///
    /// # Arguments
    ///
    /// * `eth_client` - The Ethereum client to use for contract interactions
    /// * `contract_address` - The address of the ID Gateway contract
    pub fn new(eth_client: &'a EthClient) -> Self {
        Self { eth_client }
    }

    /// Gets the price for registration with optional extra storage
    ///
    /// # Arguments
    ///
    /// * `extra_storage` - Amount of extra storage to allocate
    ///
    /// # Returns
    ///
    /// The price in wei as a U256
    #[allow(non_snake_case)]
    pub async fn price(&self, extraStorage: U256) -> Result<U256> {
        let call = priceCall { extraStorage };
        let data = call.abi_encode();

        let result = self.eth_client
            .call_contract(ID_GATEWAY_ADDRESS, data)
            .await
            .map_err(|e| eyre!("Failed to call price function: {}", e))?;

        if result.len() < 32 {
            return Err(eyre!("Invalid response from price call: too short"));
        }

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result[0..32]);
        let price = U256::from_be_bytes(bytes);
        
        tracing::info!("Price returned from contract: {} wei", price);
        Ok(price)
    }

    /// Registers a new FID with the specified recovery address
    ///
    /// # Arguments
    ///
    /// * `recovery` - The recovery address for the new FID
    /// * `value` - Optional transaction value in wei. If None, the price will be fetched from the contract
    ///
    /// # Returns
    ///
    /// The transaction hash as a B256
    pub async fn register(
        &self, 
        recovery: Address,
        value: Option<U256>,
    ) -> Result<B256> {
        let call = registerCall { recovery };
        let calldata = call.abi_encode();

        tracing::info!("Register calldata: 0x{}", hex::encode(&calldata));

        let transaction_value = match value {
            Some(v) => v,
            None => {
                tracing::info!("No value provided, calling price() function");
                self.price(U256::ZERO).await.map_err(|e| {
                    tracing::error!("Failed to get price from contract: {}", e);
                    eyre!("Failed to get price from contract: {}", e)
                })?
            }
        };

        tracing::info!(
            "Sending transaction to contract {} with value: {} wei",
            ID_GATEWAY_ADDRESS,
            transaction_value
        );

        let tx_hash = self.eth_client
            .send_transaction(ID_GATEWAY_ADDRESS, calldata.into(), transaction_value)
            .await
            .map_err(|e| {
                tracing::error!("Failed to send transaction: {}", e);
                eyre!("Failed to send transaction: {}", e)
            })?;

        tracing::info!("Transaction submitted: 0x{}", hex::encode(&tx_hash));
        Ok(B256::from_slice(&tx_hash[0..32]))
    }
}