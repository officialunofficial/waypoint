use serde::{Deserialize, Serialize};

/// Configuration for Ethereum functionality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthConfig {
    /// Mnemonic phrase for wallet generation
    pub mnemonic: Option<String>,

    /// Alchemy API key
    pub alchemy_api_key: Option<String>,

    /// Default network to use
    #[serde(default = "default_network")]
    pub default_network: String,

    /// Account derivation path
    #[serde(default = "default_derivation_path")]
    pub derivation_path: String,
}

/// Default network
fn default_network() -> String {
    "base".to_string()
}

/// Default derivation path for Ethereum wallets
fn default_derivation_path() -> String {
    "m/44'/60'/0'/0/0".to_string()
}

impl Default for EthConfig {
    fn default() -> Self {
        Self {
            mnemonic: None,
            alchemy_api_key: None,
            default_network: default_network(),
            derivation_path: default_derivation_path(),
        }
    }
}
