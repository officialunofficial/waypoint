//! Error types for the Ethereum module.

use thiserror::Error;

/// Errors that can occur when working with Ethereum wallets and providers.
#[derive(Debug, Error)]
pub enum EthError {
    /// Error when loading or decrypting a keystore
    #[error("Keystore error: {0}")]
    KeystoreError(String),

    /// Error when creating or using a wallet
    #[error("Wallet error: {0}")]
    WalletError(String),

    /// Error when interacting with a provider
    #[error("Provider error: {0}")]
    ProviderError(String),

    /// Error with a mnemonic phrase
    #[error("Mnemonic error: {0}")]
    MnemonicError(String),

    /// Encryption/Decryption error
    #[error("Encryption error: {0}")]
    EncryptionError(String),

    /// Invalid configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Invalid private key
    #[error("Invalid private key")]
    InvalidPrivateKey,

    /// IO Error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type for Ethereum operations
pub type Result<T> = std::result::Result<T, EthError>;

/// Ethereum network types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkKind {
    /// Ethereum mainnet
    Mainnet,
    /// Base mainnet
    Base,
    /// Sepolia testnet
    Sepolia,
    /// Base Sepolia testnet
    BaseSepolia,
    /// Goerli testnet
    Goerli,
    /// Custom network with name and chain ID
    Custom {
        /// Network name
        name: String,
        /// Chain ID
        chain_id: u64,
    },
}

impl NetworkKind {
    /// Get the chain ID for this network
    pub fn chain_id(&self) -> u64 {
        match self {
            NetworkKind::Mainnet => 1,
            NetworkKind::Base => 8453,
            NetworkKind::Sepolia => 11155111,
            NetworkKind::BaseSepolia => 84532,
            NetworkKind::Goerli => 5,
            NetworkKind::Custom { chain_id, .. } => *chain_id,
        }
    }

    /// Get the network name
    pub fn name(&self) -> &str {
        match self {
            NetworkKind::Mainnet => "mainnet",
            NetworkKind::Base => "base",
            NetworkKind::Sepolia => "sepolia",
            NetworkKind::BaseSepolia => "base-sepolia",
            NetworkKind::Goerli => "goerli",
            NetworkKind::Custom { name, .. } => name,
        }
    }

    /// Get the Alchemy RPC URL for this network
    pub fn alchemy_url(&self, api_key: &str) -> String {
        match self {
            NetworkKind::Mainnet => format!("https://eth-mainnet.g.alchemy.com/v2/{}", api_key),
            NetworkKind::Base => format!("https://base-mainnet.g.alchemy.com/v2/{}", api_key),
            NetworkKind::Sepolia => format!("https://eth-sepolia.g.alchemy.com/v2/{}", api_key),
            NetworkKind::BaseSepolia => {
                format!("https://base-sepolia.g.alchemy.com/v2/{}", api_key)
            },
            NetworkKind::Goerli => format!("https://eth-goerli.g.alchemy.com/v2/{}", api_key),
            NetworkKind::Custom { .. } => {
                format!("https://eth-mainnet.g.alchemy.com/v2/{}", api_key)
            }, // Default to mainnet for custom networks
        }
    }
}
