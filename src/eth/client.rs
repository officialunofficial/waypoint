use std::{sync::Arc, time::Duration};
use alloy_primitives::{Address, Bytes, TxHash, U256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::{TransactionReceipt, TransactionRequest};
use alloy_signer_local::PrivateKeySigner;
use tokio::time::sleep;
use crate::eth::EthError;
use super::error::Result;

/// Ethereum client for interacting with the blockchain
pub struct EthClient {
    /// The provider for making RPC calls
    provider: Arc<dyn Provider>,
    /// The wallet for signing transactions
    wallet: Arc<PrivateKeySigner>,
}

impl EthClient {
    /// Creates a new Ethereum client instance
    ///
    /// # Arguments
    ///
    /// * `provider` - The provider for making RPC calls
    /// * `wallet` - The wallet for signing transactions
    pub fn new<P: Provider + Send + Sync + 'static>(provider: P, wallet: PrivateKeySigner) -> Self {
        Self { 
            provider: Arc::new(provider), 
            wallet: Arc::new(wallet) 
        }
    }

    /// Makes a read-only call to a smart contract
    ///
    /// # Arguments
    ///
    /// * `to` - The address of the contract to call
    /// * `data` - The calldata for the contract call
    ///
    /// # Returns
    ///
    /// The bytes returned from the contract call
    pub async fn call_contract(&self, to: Address, data: Vec<u8>) -> Result<Bytes> {
        let tx = TransactionRequest::default()
            .from(self.wallet.address())
            .to(to)
            .input(data.into());
    
        self.provider
            .call(tx)
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))
    }

    /// Sends a transaction to the blockchain
    ///
    /// # Arguments
    ///
    /// * `to` - The address to send the transaction to
    /// * `data` - The calldata for the transaction
    /// * `value` - The amount of ETH to send with the transaction
    ///
    /// # Returns
    ///
    /// The transaction hash
    pub async fn send_transaction(&self, to: Address, data: Vec<u8>, value: U256) -> Result<TxHash> {
        let nonce = self.provider
            .get_transaction_count(self.wallet.address())
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))?;

        tracing::debug!("Using nonce: {}", nonce);

        let tx = TransactionRequest::default()
            .from(self.wallet.address())
            .to(to)
            .value(value)
            .nonce(nonce)
            .input(data.into());

        let transaction = self.provider
            .send_transaction(tx)
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))?;

        Ok(transaction.tx_hash().clone())
    }

    /// Waits for a transaction receipt with a timeout
    ///
    /// # Arguments
    ///
    /// * `hash` - The transaction hash to wait for
    ///
    /// # Returns
    ///
    /// The transaction receipt if found within the timeout period
    pub async fn wait_for_transaction_receipt(&self, hash: TxHash) -> Result<TransactionReceipt> {
        const MAX_RETRIES: u32 = 10;
        const RETRY_DELAY: Duration = Duration::from_secs(1);

        for attempt in 1..=MAX_RETRIES {
            if let Some(receipt) = self.provider
                .get_transaction_receipt(hash)
                .await
                .map_err(|e| EthError::ProviderError(e.to_string()))? 
            {
                return Ok(receipt);
            }

            if attempt < MAX_RETRIES {
                tracing::debug!("Waiting for transaction receipt, attempt {}/{}", attempt, MAX_RETRIES);
                sleep(RETRY_DELAY).await;
            }
        }
    
        Err(EthError::ProviderError(
            format!("Timeout waiting for receipt of tx: {:?}", hash)
        ))
    }

    /// Gets the ETH balance of an address
    ///
    /// # Arguments
    ///
    /// * `address` - The address to check the balance of
    ///
    /// # Returns
    ///
    /// The balance in wei
    pub async fn get_balance(&self, address: Address) -> Result<U256> {
        self.provider
            .get_balance(address)
            .await
            .map_err(|e| EthError::ProviderError(e.to_string()))
    }
}
