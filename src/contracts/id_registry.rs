use alloy_primitives::{address, Address, U256};
use alloy_sol_types::{sol, SolCall};
use color_eyre::eyre::{Result, eyre};
use crate::eth::EthClient;

/// The ID Registry contract address on the optimism network
pub const ID_REGISTRY_ADDRESS: Address = address!("0x00000000fc6c5f01fc30151999387bb99a9f489b");

sol! {
    function idOf(address owner) external view returns (uint256);
}

/// Client for interacting with the ID Registry smart contract
pub struct IdRegistry<'a> {
    eth_client: &'a EthClient
}

impl<'a> IdRegistry<'a> {
    /// Creates a new instance of the ID Registry client
    ///
    /// # Arguments
    ///
    /// * `eth_client` - The Ethereum client to use for contract interactions
    pub fn new(eth_client: &'a EthClient) -> Self {
        Self { eth_client }
    }

    /// Gets the FID (Farcaster ID) for a given owner address
    ///
    /// # Arguments
    ///
    /// * `owner` - The address to look up the FID for
    ///
    /// # Returns
    ///
    /// The FID as a U256
    pub async fn id_of(&self, owner: Address) -> Result<U256> {
        let id_of_call = idOfCall { owner };
        let data = id_of_call.abi_encode();

        let result = self.eth_client
            .call_contract(ID_REGISTRY_ADDRESS, data)
            .await
            .map_err(|e| eyre!("Failed to call idOf function: {}", e))?;

        if result.len() < 32 {
            return Err(eyre!("Invalid response from fid call: too short"));
        }

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result[0..32]);
        let fid = U256::from_be_bytes(bytes);
        
        tracing::info!("FID returned for the address {}: {}", owner, fid);
        Ok(fid)
    }
}
