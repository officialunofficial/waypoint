# Farcaster Integration

This document describes how to use Waypoint's Farcaster integration features.

## Farcaster Signup

To register a new Farcaster account, follow these steps:

1. Ensure you have the required environment variables set in your `.env` file:
   ```
    PROVIDER_URL: ${PROVIDER_URL}
    ETH_MNEMONIC: ${ETH_MNEMONIC}
    FARCASTER_HUB_URL: ${FARCASTER_HUB_URL}
   ```

2. Run the Farcaster signup command:

   **Local:**
   ```bash
   make farcaster signup
   ```

   This command will:
   - Generate a new Ethereum wallet from the mnemonic
   - Register a farcaster FID
   - Register a username for the account and add to it
   - Store the ED25519 signer in the `.keys` directory
   - Register the signer for the account
   - Send a hello world message to the Snapchain

3. After successful registration, your keys will be stored in the `.keys` directory.

For more information about the Farcaster protocol, visit the [official Farcaster documentation](https://docs.farcaster.xyz/). 