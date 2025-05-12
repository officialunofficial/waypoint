use alloy_primitives::Bytes;
use ed25519_dalek::{SigningKey, VerifyingKey, Signer};
use rand_core::OsRng;
use std::fs::{read, write, create_dir_all};
use std::path::Path;
use color_eyre::eyre::{Result, eyre};
use hex;

/// Default directory for storing keys
const KEY_DIR: &str = ".keys";
/// Default filename for storing the Ed25519 signing key
const KEY_FILE_NAME: &str = "ed25519_signer";
/// Expected size of the key file in bytes (only the 32-byte private key is saved)
const KEY_FILE_SIZE: usize = 32;

/// Manages Ed25519 keypair for Farcaster signing operations
#[derive(Clone)]
pub struct KeyManager {
    /// The private key used for signing
    signing_key: SigningKey,
    /// The public key used for verification
    verifying_key: VerifyingKey,
}

impl KeyManager {
    /// Creates a new Ed25519 keypair and saves the private key to disk
    pub fn new() -> Result<Self> {
        create_dir_all(KEY_DIR)
            .map_err(|e| eyre!("Failed to create keys directory: {}", e))?;

        // Generate new key pair
        let signing_key = SigningKey::generate(&mut OsRng);
        let sk_bytes: [u8; 32] = signing_key.to_bytes();

        // Save only the private key
        let key_path = Path::new(KEY_DIR).join(KEY_FILE_NAME);
        write(&key_path, &sk_bytes)
            .map_err(|e| eyre!("Failed to write key file: {}", e))?;

        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Loads an existing Ed25519 signing key from disk and derives the public key
    pub fn load_from_file() -> Result<Self> {
        let key_path = Path::new(KEY_DIR).join(KEY_FILE_NAME);
        println!("Loading keys from: {:?}", key_path);

        let data = read(&key_path)
            .map_err(|e| eyre!("Failed to read key file: {}", e))?;

        if data.len() != KEY_FILE_SIZE {
            return Err(eyre!(
                "Invalid key file size: expected {} bytes, got {} bytes",
                KEY_FILE_SIZE,
                data.len()
            ));
        }

        let sk_bytes: [u8; 32] = data[..32].try_into()
            .map_err(|_| eyre!("Failed to parse private key bytes"))?;

        let signing_key = SigningKey::from_bytes(&sk_bytes);
        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Signs the given data using the private key
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        println!("Signing data: {:?}", hex::encode(data));
        println!("Signing key: {:?}", hex::encode(&self.signing_key.to_bytes()));
        self.signing_key.sign(data).to_vec()
    }

    /// Returns the public key as `Bytes`
    pub fn public_key_bytes(&self) -> Bytes {
        Bytes::from(self.verifying_key.to_bytes().to_vec())
    }

    /// Returns the verifying key as a byte vector
    pub fn verifying_key_bytes(&self) -> Vec<u8> {
        self.verifying_key.to_bytes().to_vec()
    }

    /// Checks if the key file exists
    pub fn key_file_exists() -> bool {
        Path::new(KEY_DIR).join(KEY_FILE_NAME).exists()
    }
}