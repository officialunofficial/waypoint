//! Secure storage for Ethereum mnemonics.

use crate::eth::error::{EthError, Result};
use coins_bip39::{English, Mnemonic};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Mnemonic phrase wrapper that implements zeroize for secure memory handling
#[derive(Debug, ZeroizeOnDrop)]
pub struct SecureMnemonic {
    /// The mnemonic phrase
    phrase: String,
}

impl Zeroize for SecureMnemonic {
    fn zeroize(&mut self) {
        self.phrase.zeroize();
    }
}

impl SecureMnemonic {
    /// Create a new secure mnemonic
    pub fn new(phrase: String) -> Self {
        Self { phrase }
    }

    /// Get the mnemonic phrase
    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// Validate that the mnemonic phrase is valid
    pub fn validate(&self) -> Result<()> {
        // Parse the mnemonic to validate it
        Mnemonic::<English>::new_from_phrase(self.phrase())
            .map_err(|e| EthError::MnemonicError(e.to_string()))?;

        Ok(())
    }
}
