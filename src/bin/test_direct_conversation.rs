use std::sync::Arc;
use tokio::sync::Mutex;
use waypoint::core::data_context::DataContextBuilder;
use waypoint::core::types::Fid;
use waypoint::hub::client::Hub;
use waypoint::hub::providers::FarcasterHubClient;
use waypoint::query::WaypointQuery;
use waypoint::services::mcp::NullDb;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Initialize the hub client
    let hub_address = "snapchain.farcaster.xyz:3383";
    println!("Connecting to Farcaster hub at {}", hub_address);

    // Create a new Hub then connect to it
    let mut hub = Hub::new(Arc::new(waypoint::config::HubConfig {
        url: hub_address.to_string(),
        headers: std::collections::HashMap::new(),
        max_concurrent_connections: 5,
        max_requests_per_second: 10,
        retry_max_attempts: 5,
        retry_base_delay_ms: 100,
        retry_max_delay_ms: 30000,
        retry_jitter_factor: 0.25,
        retry_timeout_ms: 60000,
        conn_timeout_ms: 30000,
        shard_indices: vec![0],
        subscribe_to_all_shards: false,
    }))
    .expect("Failed to create hub client");

    hub.connect().await.expect("Failed to connect to hub");
    let hub_client = FarcasterHubClient::new(Arc::new(Mutex::new(hub)));

    // Build the data context with the hub client
    let data_context =
        DataContextBuilder::new().with_hub_client(hub_client).with_database(NullDb).build();

    // Create shared query core
    let query = WaypointQuery::new(data_context);

    // Test parameters
    let fid = Fid::from(4085);
    let hash_str = "9df3dc7d3ff493bbcad5e0b167012b4e1edebced"; // Without 0x prefix

    // Call the get_conversation function
    println!("Testing get_conversation with FID {} and hash {}", fid, hash_str);
    let conversation = query
        .do_get_conversation(
            fid, hash_str, // Without 0x prefix
            true,     // recursive
            5,        // max_depth
            10,       // limit
        )
        .await;

    println!("Conversation Result:");
    println!("{}", conversation);
}
