//! Ethereum wallet implementation focused on mnemonic-based wallets.

use crate::eth::error::{EthError, NetworkKind, Result};
use alloy_primitives::Address;
use alloy_signer::Signer;
use alloy_signer_local::{MnemonicBuilder, PrivateKeySigner};
use coins_bip39::English;
use std::sync::Arc;

/// A wallet trait defining common Ethereum wallet functionality.
pub trait Wallet: Send + Sync {
    /// Get the wallet's address
    fn address(&self) -> Address;

    /// Get the chain ID associated with this wallet
    fn chain_id(&self) -> u64;

    /// Get the network associated with this wallet
    fn network(&self) -> NetworkKind;

    /// Sign a message with this wallet
    fn sign_message(&self, message: &[u8]) -> Result<Vec<u8>>;
}

/// A wallet created from a mnemonic phrase.
#[derive(Clone)]
pub struct MnemonicWallet {
    /// The underlying alloy wallet
    wallet: Arc<PrivateKeySigner>,

    /// The network to use
    network: NetworkKind,

    /// The wallet's address
    address: Address,
}

impl MnemonicWallet {
    /// Create a new wallet from a mnemonic phrase with a given derivation path and network.
    pub fn from_mnemonic(
        mnemonic: &str,
        derivation_path: &str,
        network: NetworkKind,
    ) -> Result<Self> {
        // Create a wallet from the mnemonic
        let wallet = MnemonicBuilder::<English>::default()
            .phrase(mnemonic)
            .derivation_path(derivation_path)
            .map_err(|e| EthError::WalletError(format!("Invalid derivation path: {}", e)))?
            .build()
            .map_err(|e| EthError::WalletError(e.to_string()))?;

        // Get the wallet address
        let address = wallet.address();

        Ok(Self { wallet: Arc::new(wallet), network, address })
    }

    /// Create a new wallet with the default derivation path.
    pub fn new(mnemonic: &str, network: NetworkKind) -> Result<Self> {
        Self::from_mnemonic(mnemonic, "m/44'/60'/0'/0/0", network)
    }

    /// Create a wallet from a configuration.
    pub fn from_config(config: &crate::eth::EthConfig) -> Result<Self> {
        // Get mnemonic from config (which is loaded via Figment from config files or env vars)
        let mnemonic = if let Some(mnemonic_str) = &config.mnemonic {
            mnemonic_str.clone()
        } else {
            return Err(EthError::ConfigError(
                "Mnemonic not provided in configuration (set via WAYPOINT_ETH__MNEMONIC or config file)".to_string()
            ));
        };

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

        // Create the wallet
        Self::from_mnemonic(&mnemonic, &config.derivation_path, network)
    }
}

impl Wallet for MnemonicWallet {
    fn address(&self) -> Address {
        self.address
    }

    fn chain_id(&self) -> u64 {
        self.network.chain_id()
    }

    fn network(&self) -> NetworkKind {
        self.network.clone()
    }

    fn sign_message(&self, message: &[u8]) -> Result<Vec<u8>> {
        // Check if we're already in a tokio runtime
        let signature = if tokio::runtime::Handle::try_current().is_ok() {
            // We're already in a runtime, so spawn a blocking task
            let wallet_clone = self.wallet.clone();
            let message_clone = message.to_vec();

            // Use a blocking task to avoid blocking the current thread
            tokio::task::block_in_place(|| {
                futures::executor::block_on(async {
                    wallet_clone
                        .sign_message(&message_clone)
                        .await
                        .map_err(|e| EthError::WalletError(e.to_string()))
                })
            })?
        } else {
            // Not in a runtime, create a new one
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| EthError::WalletError(e.to_string()))?;

            // Sign the message using the alloy wallet
            runtime.block_on(async {
                self.wallet
                    .sign_message(message)
                    .await
                    .map_err(|e| EthError::WalletError(e.to_string()))
            })?
        };

        // Convert the signature bytes to a vector
        Ok(signature.as_bytes().to_vec())
    }
}
