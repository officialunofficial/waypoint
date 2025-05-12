use alloy_provider::ProviderBuilder;
use alloy_signer_local::MnemonicBuilder;
use clap::{ArgMatches, Command};
use coins_bip39::English;
use color_eyre::eyre::{Result, eyre};
use tracing::info;
use waypoint::{
    eth::{EthClient, EthError}, 
    farcaster::{
        get_current_farcaster_timestamp, 
        KeyManager, 
        RegisterFID, 
        RegisterFname, 
        SendMessageClient
    }, 
    proto::{
        hub_service_client::HubServiceClient, 
        message_data::Body, 
        CastAddBody, 
        FarcasterNetwork, 
        MessageData, 
        MessageType
    }
};

/// Register Farcaster-related commands
pub fn register_commands(app: Command) -> Command {
    app.about("Farcaster operations")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("signup")
                .about("Register a new Farcaster account")
        )
}

/// Handle Farcaster commands
pub async fn handle_command(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("signup", _)) => handle_signup().await,
        _ => {
            info!("Please specify a valid Farcaster command. Available commands:");
            info!("  signup    - Register a new Farcaster account");
            Ok(())
        },
    }
}

/// Handle the signup command
async fn handle_signup() -> Result<()> {
    // Load environment variables
    let provider_url = std::env::var("PROVIDER_URL")
        .map_err(|_| eyre!("PROVIDER_URL environment variable not set"))?;
    
    let mnemonic = std::env::var("ETH_MNEMONIC")
        .map_err(|_| eyre!("ETH_MNEMONIC environment variable not set"))?;

    let farcaster_hub_url = std::env::var("FARCASTER_HUB_URL")
        .map_err(|_| eyre!("FARCASTER_HUB_URL environment variable not set"))?;

    // Initialize wallet
    let wallet = MnemonicBuilder::<English>::default()
        .phrase(mnemonic)
        .build()
        .map_err(|e| EthError::WalletError(e.to_string()))?;

    // Initialize provider
    let provider = ProviderBuilder::new()
        .wallet(wallet.clone())
        .connect(&provider_url)
        .await?;

    // Initialize key manager
    let key_manager = KeyManager::new()?;

    // Initialize eth client
    let eth_client = EthClient::new(provider, wallet.clone());

    // Register FID
    let register_fid_client = RegisterFID::new(
        eth_client,
        key_manager.clone(),
        wallet.clone(),
    );

    let fid = register_fid_client.register_fid(wallet.address()).await?;
    info!("Successfully registered FID: {:?}", fid);

    // Initialize hub client
    let hub_client = HubServiceClient::connect(farcaster_hub_url)
        .await?;

    // Initialize send message client
    let mut send_message_client = SendMessageClient::new(
        key_manager,
        hub_client,
    );

    // Register fname
    let register_fname_client = RegisterFname::new(
        wallet,
        send_message_client.clone(),
    );

    let fname = format!("waypoint{:?}", fid);
    register_fname_client.register_fname(fid, fname.clone()).await?;
    info!("Successfully registered fname: {}", fname);

    // Send a hello world message
    let message_data = create_hello_world_message(fid)?;
    send_message_client.send_message(message_data).await?;
    info!("Successfully sent message");

    Ok(())
}

/// Creates a hello world message
fn create_hello_world_message(fid: alloy_primitives::U256) -> Result<MessageData> {
    let fid_u64 = fid.to_string().parse::<u64>()
        .map_err(|e| eyre!("Failed to parse FID: {}", e))?;

    Ok(MessageData {    
        r#type: MessageType::CastAdd as i32,
        fid: fid_u64,
        timestamp: get_current_farcaster_timestamp() as u32,
        network: FarcasterNetwork::Mainnet as i32,
        body: Some(Body::CastAddBody(CastAddBody {
            text: "Hello world!".to_string(),
            embeds: vec![],
            embeds_deprecated: vec![],
            mentions: vec![],
            mentions_positions: vec![],
            r#type: 0,
            parent: None
        }))
    })
} 