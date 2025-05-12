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
   - Registers a farcaster FID
   - Register a username for the account and add to it
   - Store the ED25519 signer in the `.keys` directory
   - Registers the signer for the account
   - Sends a hello world message to the snapchain

3. The process will prompt you for:
   - A username to register (must be available)
   - Confirmation of the registration

4. After successful registration, your keys will be stored in the `.keys` directory.

For more information about the Farcaster protocol, visit the [official Farcaster documentation](https://docs.farcaster.xyz/). 