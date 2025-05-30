use crate::{
    database::{batch::BatchInserter, client::Database},
    processor::database::DatabaseProcessor,
    proto::{self, Message, MessageData},
};
use chrono::Utc;
use rand::Rng;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{error, info};

// Helper function to generate a random message of given type
fn generate_random_message(message_type: i32, fid: u64) -> Message {
    let mut rng = rand::rng();
    let hash = (0..32).map(|_| rng.random::<u8>()).collect::<Vec<u8>>();
    let signer = (0..32).map(|_| rng.random::<u8>()).collect::<Vec<u8>>();

    let mut message = Message {
        data: Some(MessageData {
            r#type: message_type,
            fid,
            timestamp: Utc::now().timestamp() as u32,
            network: 1,
            ..Default::default()
        }),
        hash_scheme: 1,
        signature_scheme: 1,
        hash: hash.clone(),
        signature: (0..64).map(|_| rng.random::<u8>()).collect(),
        signer: signer.clone(),
        data_bytes: None,
    };

    // Add appropriate body based on message type
    match message_type {
        1 => {
            // CastAdd
            message.data.as_mut().unwrap().body =
                Some(proto::message_data::Body::CastAddBody(proto::CastAddBody {
                    text: format!("Test cast {}", rng.random::<u32>()),
                    mentions: vec![],
                    mentions_positions: vec![],
                    embeds: vec![],
                    parent: None,
                    embeds_deprecated: vec![],
                    r#type: 0, // Regular cast
                }));
        },
        3 => {
            // ReactionAdd
            message.data.as_mut().unwrap().body =
                Some(proto::message_data::Body::ReactionBody(proto::ReactionBody {
                    r#type: 1, // Like
                    target: Some(proto::reaction_body::Target::TargetCastId(proto::CastId {
                        fid: rng.random_range(1..10000),
                        hash: (0..32).map(|_| rng.random::<u8>()).collect(),
                    })),
                }));
        },
        5 => {
            // LinkAdd
            message.data.as_mut().unwrap().body =
                Some(proto::message_data::Body::LinkBody(proto::LinkBody {
                    r#type: "follow".to_string(),
                    target: Some(proto::link_body::Target::TargetFid(rng.random_range(1..10000))),
                    display_timestamp: Some(Utc::now().timestamp() as u32),
                }));
        },
        11 => {
            // UserDataAdd
            message.data.as_mut().unwrap().body =
                Some(proto::message_data::Body::UserDataBody(proto::UserDataBody {
                    r#type: 2, // Display name
                    value: format!("User {}", rng.random::<u32>()),
                }));
        },
        _ => {},
    }

    message
}

// Generate a set of random messages for benchmarking
fn generate_test_data(count: usize) -> Vec<Message> {
    let mut messages = Vec::with_capacity(count);
    let mut rng = rand::rng();

    for _ in 0..count {
        // Random FID between 1 and 10000
        let fid = rng.random_range(1..10000);

        // Random message type from cast, reaction, link, user_data
        let message_type = match rng.random_range(0..4) {
            0 => 1,  // CastAdd
            1 => 3,  // ReactionAdd
            2 => 5,  // LinkAdd
            3 => 11, // UserDataAdd
            _ => 1,
        };

        messages.push(generate_random_message(message_type, fid));
    }

    messages
}

// Benchmark function for individual inserts
pub async fn benchmark_individual_inserts(
    processor: &DatabaseProcessor,
    messages: &[Message],
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();

    for msg in messages {
        processor.process_message(msg, "merge").await?;
    }

    Ok(start.elapsed())
}

// Benchmark function for batch inserts
pub async fn benchmark_batch_inserts(
    processor: &DatabaseProcessor,
    messages: &[Message],
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();

    processor.process_message_batch(messages, "merge").await?;

    Ok(start.elapsed())
}

// Run the full benchmark with different batch sizes
pub async fn run_benchmark(
    db: Arc<Database>,
    processor: Arc<DatabaseProcessor>,
    total_messages: usize,
) -> Result<(), String> {
    info!("Generating test data with {} messages", total_messages);
    let messages = generate_test_data(total_messages);

    // Test with individual inserts (small sample)
    let small_sample = messages.iter().take(100).cloned().collect::<Vec<_>>();
    info!("Testing individual inserts with 100 messages");
    match benchmark_individual_inserts(&processor, &small_sample).await {
        Ok(duration) => {
            info!(
                "Individual insert: {} messages in {:?} ({:?} per message)",
                small_sample.len(),
                duration,
                duration / small_sample.len() as u32
            );
        },
        Err(e) => {
            error!("Error during individual insert benchmark: {}", e);
            return Err(format!("Benchmark error: {}", e));
        },
    }

    // Wait a bit to let the database catch up
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Test with batch inserts at different batch sizes
    for batch_size in [50, 100, 200, 500, 1000].iter() {
        info!("Testing batch inserts with batch size {}", batch_size);
        let batch_inserter = BatchInserter::new(&db.pool, *batch_size);

        let start = Instant::now();
        match batch_inserter.process_message_batch(&messages).await {
            Ok(_) => {
                let duration = start.elapsed();
                info!(
                    "Batch insert (size {}): {} messages in {:?} ({:?} per message)",
                    batch_size,
                    messages.len(),
                    duration,
                    duration / messages.len() as u32
                );
            },
            Err(e) => {
                error!("Error during batch insert benchmark: {}", e);
                return Err(format!("Batch insert error: {}", e));
            },
        }

        // Wait a bit to let the database catch up
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Test with the processor's process_message_batch function
    info!("Testing processor batch processing");
    match benchmark_batch_inserts(&processor, &messages).await {
        Ok(duration) => {
            info!(
                "Processor batch insert: {} messages in {:?} ({:?} per message)",
                messages.len(),
                duration,
                duration / messages.len() as u32
            );
        },
        Err(e) => {
            error!("Error during processor batch insert benchmark: {}", e);
            return Err(format!("Processor batch insert error: {}", e));
        },
    }

    Ok(())
}
