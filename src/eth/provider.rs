//! Ethereum provider implementation using Alchemy.

use crate::eth::{
    config::EthConfig,
    error::{EthError, NetworkKind, Result},
};
use alloy_primitives::{Address, B256 as TxHash, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types_eth::TransactionReceipt;
use std::sync::Arc;
use url::Url;

#[derive(Clone)]
pub struct AlchemyProvider {
    /// The underlying alloy provider - using type erasure for simplicity
    provider: Arc<dyn Provider + Send + Sync>,

    /// The network this provider is connected to
    network: NetworkKind,

    #[allow(dead_code)]
    api_key: String,
}

impl AlchemyProvider {
    /// Create a new provider for the given network with an Alchemy API key.
    pub fn new(network: NetworkKind, api_key: &str) -> Result<Self> {
        // Get the Alchemy URL for this network
        let url_str = network.alchemy_url(api_key);

        // Parse the URL
        let url = Url::parse(&url_str)
            .map_err(|e| EthError::ProviderError(format!("Invalid URL: {}", e)))?;

        // Create the provider using the builder
        // on_http() returns a provider directly, not a Result
        let provider = ProviderBuilder::new().on_http(url);

        Ok(Self { provider: Arc::new(provider), network, api_key: api_key.to_string() })
    }

    /// Create a provider from a configuration.
    pub fn from_config(config: &EthConfig) -> Result<Self> {
        // Get the API key
        let api_key = config
            .alchemy_api_key
            .as_ref()
            .ok_or_else(|| EthError::ConfigError("Alchemy API key is required".to_string()))?;

        // Get the network
        let network = match config.default_network.as_str() {
            "mainnet" => NetworkKind::Mainnet,
            "base" => NetworkKind::Base,
            "sepolia" => NetworkKind::Sepolia,
            "base-sepolia" => NetworkKind::BaseSepolia,
            "goerli" => NetworkKind::Goerli,
            _ => NetworkKind::Custom {
                name: config.default_network.clone(),
                chain_id: 0, // Default to 0 for unknown networks
            },
        };

        Self::new(network, api_key)
    }

    /// Get the current network.
    pub fn network(&self) -> &NetworkKind {
        &self.network
    }

    /// Get the chain ID.
    pub fn chain_id(&self) -> u64 {
        self.network.chain_id()
    }

    /// Get the current gas price.
    pub async fn gas_price(&self) -> Result<U256> {
        let price = self
            .provider
            .get_gas_price()
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))?;

        // Convert the returned u128 to U256
        Ok(U256::from(price))
    }

    /// Get the balance of an address.
    pub async fn get_balance(&self, address: Address) -> Result<U256> {
        self.provider.get_balance(address).await.map_err(|e| EthError::ProviderError(e.to_string()))
    }

    /// Get a transaction receipt.
    pub async fn get_transaction_receipt(
        &self,
        tx_hash: TxHash,
    ) -> Result<Option<TransactionReceipt>> {
        self.provider
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))
    }

    /// Get the underlying alloy provider.
    pub fn provider(&self) -> &Arc<dyn Provider + Send + Sync> {
        &self.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn test_provider_api_calls() {
        // Load config to get API key from environment
        let config = match Config::load() {
            Ok(config) => config,
            Err(_) => return, // Skip test if config loading fails
        };
        
        // Skip this test if no API key is provided
        let api_key = match &config.eth.alchemy_api_key {
            Some(key) => key,
            None => return, // Skip test if no API key
        };

        // Create provider for Base Sepolia (testnet)
        let provider = AlchemyProvider::new(NetworkKind::BaseSepolia, api_key).unwrap();

        // Check network
        assert_eq!(provider.network(), &NetworkKind::BaseSepolia);
        assert_eq!(provider.chain_id(), 84532);

        // Get gas price (should work on any network)
        let gas_price = provider.gas_price().await.unwrap();
        assert!(gas_price > U256::ZERO);
    }
}
