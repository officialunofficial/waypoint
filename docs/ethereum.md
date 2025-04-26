# Ethereum Module

The Ethereum module provides support for interacting with Ethereum networks.

## Configuration

The Ethereum module is configured through the application's main configuration system:

```toml
# In config.toml
[eth]
# Alchemy API key for provider functionality
alchemy_api_key = "your-alchemy-api-key"  

# Network configuration
default_network = "base"  # Supported: mainnet, base, sepolia, base-sepolia, goerli

# Mnemonic configuration
# Set via environment variable:
# export WAYPOINT_ETH__MNEMONIC="your mnemonic phrase"
# 
# Or in config file:
# mnemonic = "your mnemonic phrase"

# Optional - only specify if you need a custom derivation path
# derivation_path = "m/44'/60'/0'/0/0"
```

Via environment variables (recommended for production):

```sh
# API key for Alchemy provider
WAYPOINT_ETH__ALCHEMY_API_KEY="your-alchemy-api-key"

# Network configuration
WAYPOINT_ETH__DEFAULT_NETWORK="base"

# SECURE: Store mnemonic in environment variable (not in config files)
WAYPOINT_ETH__MNEMONIC="your 12 or 24 word mnemonic phrase here"

# Optional: custom derivation path (if needed)
# WAYPOINT_ETH__DERIVATION_PATH="m/44'/60'/0'/0/0"
```

## Core Components

### Wallet Management

The Ethereum module includes a `MnemonicWallet` implementation that can generate wallets from BIP-39 mnemonic phrases.
The wallet implementation uses [alloy-rs](https://github.com/alloy-rs/alloy) for cryptographic operations.

```rust
use waypoint::eth::{MnemonicWallet, SecureMnemonic, NetworkKind};

// In a real app, this would be loaded from your configuration
// Set via: export WAYPOINT_ETH__MNEMONIC="your mnemonic phrase" 
let mnemonic_phrase = "test test test test test test test test test test test junk";
let mnemonic = SecureMnemonic::new(mnemonic_phrase.to_string());

// Create wallets for different networks
let base_wallet = MnemonicWallet::new(mnemonic.phrase(), NetworkKind::Base)?;
let mainnet_wallet = MnemonicWallet::new(mnemonic.phrase(), NetworkKind::Mainnet)?;
println!("Base wallet address: {}", base_wallet.address());
println!("Mainnet wallet address: {}", mainnet_wallet.address());

// Sign a message
let message = b"Hello, Ethereum!";
let signature = base_wallet.sign_message(message)?;
println!("Signature: {}", hex::encode(signature));
```

### Provider Interface

The Ethereum module includes an `AlchemyProvider` implementation for interacting with Ethereum networks via
Alchemy's API. This provider can be used to query blockchain data, such as gas prices and account balances.

```rust
use waypoint::eth::{AlchemyProvider, NetworkKind};

// Create providers for different networks
let mainnet = AlchemyProvider::new(NetworkKind::Mainnet, "your-alchemy-api-key")?;
let base = AlchemyProvider::new(NetworkKind::Base, "your-alchemy-api-key")?;
let sepolia = AlchemyProvider::new(NetworkKind::Sepolia, "your-alchemy-api-key")?;

// Get current gas prices across networks
let mainnet_gas = mainnet.gas_price().await?;
let base_gas = base.gas_price().await?;
println!("Mainnet gas price: {} gwei", mainnet_gas / 1_000_000_000u64);
println!("Base gas price: {} gwei", base_gas / 1_000_000_000u64);

// Get an account balance
let balance = mainnet.get_balance(mainnet_wallet.address()).await?;
println!("Balance: {} ETH", balance.to_f64_lossy() / 1e18);
```

### Network Support

The module includes built-in support for various Ethereum networks, including Base Chain and its testnet:

- `NetworkKind::Mainnet` - Ethereum mainnet (chain ID: 1)
- `NetworkKind::Base` - Base mainnet (chain ID: 8453)
- `NetworkKind::Sepolia` - Sepolia testnet (chain ID: 11155111)
- `NetworkKind::BaseSepolia` - Base Sepolia testnet (chain ID: 84532)
- `NetworkKind::Goerli` - Goerli testnet (chain ID: 5)
- `NetworkKind::Custom` - Custom networks with user-defined name and chain ID

## Security Considerations

The Ethereum module implements multiple security best practices:

1. **Environment Variable Storage**: The recommended way to provide mnemonics is through environment 
   variables, not in configuration files which could be accidentally committed to version control.

2. **24-Word Mnemonic Support**: Support for 24-word mnemonics provides enhanced security compared to
   the standard 12-word phrases (256-bit entropy vs 128-bit).

3. **Secure Mnemonic Handling**: The `SecureMnemonic` structure uses the `zeroize` crate to securely 
   erase sensitive data from memory when it's no longer needed.

4. **Memory Zeroization**: All sensitive cryptographic material uses `ZeroizeOnDrop`, ensuring private
   keys and mnemonics are wiped from memory when they're no longer needed.

5. **Minimal Attack Surface**: The module exposes only the necessary functionality for wallet and provider
   operations, minimizing the attack surface.

6. **No Private Key Export**: The module does not provide functionality to export private keys, reducing
   the risk of key exposure.

7. **Runtime Management**: The module carefully manages the async/sync boundary, ensuring that cryptographic
   operations can be performed efficiently without blocking.

8. **No File Storage**: We do not provide functionality to save mnemonics or keys to the filesystem,
   to prevent potential security vulnerabilities.

## Example Usage

### Creating and Using a Wallet

```rust
use waypoint::eth::{MnemonicWallet, SecureMnemonic, NetworkKind, Wallet};

// In a real app, this would be loaded from your configuration
// Set via: export WAYPOINT_ETH__MNEMONIC="your mnemonic phrase"
let mnemonic_phrase = "test test test test test test test test test test test junk";
let mnemonic = SecureMnemonic::new(mnemonic_phrase.to_string());

// Create wallets for different networks
let mainnet_wallet = MnemonicWallet::new(mnemonic.phrase(), NetworkKind::Mainnet)?;
let base_wallet = MnemonicWallet::new(mnemonic.phrase(), NetworkKind::BaseSepolia)?;

// Display wallet information
println!("Mainnet Address: {}", mainnet_wallet.address());
println!("Mainnet Chain ID: {}", mainnet_wallet.chain_id());
println!("Network: {}", mainnet_wallet.network().name());

println!("Base Sepolia Address: {}", base_wallet.address());
println!("Base Sepolia Chain ID: {}", base_wallet.chain_id());

// Sign a message with mainnet wallet
let message = b"Hello, Ethereum!";
let signature = mainnet_wallet.sign_message(message)?;
println!("Signature: {}", hex::encode(signature));

```

### Interacting with the Blockchain

```rust
use waypoint::eth::{AlchemyProvider, NetworkKind, types::Address};

// Create a provider for Ethereum testnets
let sepolia = AlchemyProvider::new(NetworkKind::Sepolia, "your-alchemy-api-key")?;
let base_sepolia = AlchemyProvider::new(NetworkKind::BaseSepolia, "your-alchemy-api-key")?;

// Use the Sepolia provider for this example
let provider = sepolia;

// Get the current gas price
let gas_price = provider.gas_price().await?;
println!("Current gas price: {} gwei", gas_price / 1_000_000_000u64);

// Get transaction receipt
let tx_hash = "0x...".parse()?;
if let Some(receipt) = provider.get_transaction_receipt(tx_hash).await? {
    println!("Transaction status: {}", receipt.status);
    println!("Block number: {}", receipt.block_number);
    println!("Gas used: {}", receipt.gas_used);
}
```

## Testing

The Ethereum module includes unit tests that can be run with:

```bash
cargo test --package waypoint --lib eth
```

For tests that interact with Ethereum networks, you'll need to set the `WAYPOINT_ETH__ALCHEMY_API_KEY` environment
variable to a valid Alchemy API key. Tests that require this environment variable are skipped if it's
not set.