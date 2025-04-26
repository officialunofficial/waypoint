mod config;
mod error;
mod keystore;
mod provider;
mod wallet;

pub use config::EthConfig;
pub use error::{EthError, NetworkKind};
pub use keystore::SecureMnemonic;
pub use provider::AlchemyProvider;
pub use wallet::{MnemonicWallet, Wallet};

/// Re-export essential types from alloy-primitives and alloy for convenience
pub mod types {
    pub use super::NetworkKind;
    pub use alloy_primitives::{Address, B256 as TxHash, U256};
    pub use alloy_rpc_types_eth::TransactionReceipt;
}
