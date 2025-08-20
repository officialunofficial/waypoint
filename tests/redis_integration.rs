use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use waypoint::config::RedisConfig;
use waypoint::redis::{client::Redis, stream::RedisStream};

/// Helper to check if Redis is available
async fn redis_available() -> bool {
    let config = RedisConfig {
        url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        pool_size: 10,
        batch_size: 100,
        enable_dead_letter: true,
        consumer_rebalance_interval_seconds: 300,
        metrics_collection_interval_seconds: 60,
        connection_timeout_ms: 5000,
        idle_timeout_secs: 300,
        max_connection_lifetime_secs: 300,
    };

    match Redis::new(&config).await {
        Ok(redis) => redis.check_connection().await.unwrap_or(false),
        Err(_) => false,
    }
}

#[tokio::test]
async fn test_stream_create_consume_cycle() {
    if !redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let config = RedisConfig {
        url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        pool_size: 20,
        batch_size: 10,
        enable_dead_letter: true,
        consumer_rebalance_interval_seconds: 300,
        metrics_collection_interval_seconds: 60,
        connection_timeout_ms: 5000,
        idle_timeout_secs: 300,
        max_connection_lifetime_secs: 300,
    };

    let redis = Arc::new(Redis::new(&config).await.expect("Failed to create Redis client"));
    let stream = RedisStream::new(redis.clone());

    let test_stream_key = format!("test:stream:{}", uuid::Uuid::new_v4());
    let test_group = "test-group";
    let test_consumer = "test-consumer";

    // 1. Create consumer group
    let _ = stream.create_group(&test_stream_key, test_group).await;

    // 2. Publish test messages
    let mut message_ids = Vec::new();
    for i in 0..10 {
        let data = format!("test message {}", i).into_bytes();
        match redis.xadd(&test_stream_key, &data).await {
            Ok(id) => message_ids.push(id),
            Err(e) => panic!("Failed to add message: {}", e),
        }
    }

    assert_eq!(message_ids.len(), 10, "Should have published 10 messages");

    // 3. Consume messages
    let entries = stream
        .reserve(&test_stream_key, test_group, 5, Some(test_consumer))
        .await
        .expect("Failed to reserve messages");

    assert_eq!(entries.len(), 5, "Should consume 5 messages in first batch");

    // 4. Acknowledge consumed messages
    let ack_ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
    stream
        .ack(&test_stream_key, test_group, ack_ids)
        .await
        .expect("Failed to acknowledge messages");

    // 5. Consume remaining messages
    let entries = stream
        .reserve(&test_stream_key, test_group, 10, Some(test_consumer))
        .await
        .expect("Failed to reserve remaining messages");

    assert_eq!(entries.len(), 5, "Should consume remaining 5 messages");

    // Clean up
    let _ = redis.xdel(&test_stream_key, "0-0").await;
}

#[tokio::test]
async fn test_pending_message_recovery() {
    if !redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let config = RedisConfig {
        url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        pool_size: 20,
        batch_size: 10,
        enable_dead_letter: true,
        consumer_rebalance_interval_seconds: 300,
        metrics_collection_interval_seconds: 60,
        connection_timeout_ms: 5000,
        idle_timeout_secs: 300,
        max_connection_lifetime_secs: 300,
    };

    let redis = Arc::new(Redis::new(&config).await.expect("Failed to create Redis client"));
    let stream = RedisStream::new(redis.clone());

    let test_stream_key = format!("test:pending:{}", uuid::Uuid::new_v4());
    let test_group = "test-group";
    let consumer1 = "consumer-1";
    let consumer2 = "consumer-2";

    // Create consumer group
    let _ = stream.create_group(&test_stream_key, test_group).await;

    // Publish messages
    for i in 0..5 {
        let data = format!("pending message {}", i).into_bytes();
        redis.xadd(&test_stream_key, &data).await.expect("Failed to add message");
    }

    // Consumer 1 reserves messages but doesn't acknowledge them
    let entries = stream
        .reserve(&test_stream_key, test_group, 5, Some(consumer1))
        .await
        .expect("Failed to reserve messages");

    assert_eq!(entries.len(), 5, "Consumer 1 should reserve 5 messages");

    // Wait briefly to simulate consumer 1 failing
    sleep(Duration::from_millis(100)).await;

    // Consumer 2 claims stale messages
    let claimed = stream
        .claim_stale(&test_stream_key, test_group, Duration::from_millis(50), 10, Some(consumer2))
        .await
        .expect("Failed to claim stale messages");

    assert_eq!(claimed.len(), 5, "Consumer 2 should claim all 5 stale messages");

    // Acknowledge claimed messages
    let ack_ids: Vec<String> = claimed.iter().map(|e| e.id.clone()).collect();
    stream
        .ack(&test_stream_key, test_group, ack_ids)
        .await
        .expect("Failed to acknowledge claimed messages");

    // Verify no pending messages remain
    let pending = redis
        .xpending(&test_stream_key, test_group, Duration::from_millis(0), 10)
        .await
        .expect("Failed to check pending messages");

    assert_eq!(pending.len(), 0, "Should have no pending messages after acknowledgment");
}

#[tokio::test]
async fn test_concurrent_consumers() {
    if !redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let config = RedisConfig {
        url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        pool_size: 50,
        batch_size: 10,
        enable_dead_letter: true,
        consumer_rebalance_interval_seconds: 300,
        metrics_collection_interval_seconds: 60,
        connection_timeout_ms: 5000,
        idle_timeout_secs: 300,
        max_connection_lifetime_secs: 300,
    };

    let redis = Arc::new(Redis::new(&config).await.expect("Failed to create Redis client"));
    let stream = RedisStream::new(redis.clone());

    let test_stream_key = format!("test:concurrent:{}", uuid::Uuid::new_v4());
    let test_group = "test-group";

    // Create consumer group
    let _ = stream.create_group(&test_stream_key, test_group).await;

    // Publish 100 messages
    for i in 0..100 {
        let data = format!("concurrent message {}", i).into_bytes();
        redis.xadd(&test_stream_key, &data).await.expect("Failed to add message");
    }

    // Spawn multiple concurrent consumers
    let mut handles = Vec::new();
    for consumer_id in 0..5 {
        let stream = stream.clone();
        let stream_key = test_stream_key.clone();
        let group = test_group.to_string();
        let consumer_name = format!("consumer-{}", consumer_id);

        let handle = tokio::spawn(async move {
            let mut consumed = 0;
            for _ in 0..5 {
                let entries = stream
                    .reserve(&stream_key, &group, 5, Some(&consumer_name))
                    .await
                    .expect("Failed to reserve messages");

                if !entries.is_empty() {
                    consumed += entries.len();
                    let ack_ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
                    stream
                        .ack(&stream_key, &group, ack_ids)
                        .await
                        .expect("Failed to acknowledge messages");
                }

                sleep(Duration::from_millis(10)).await;
            }
            consumed
        });

        handles.push(handle);
    }

    // Wait for all consumers to finish
    let mut total_consumed = 0;
    for handle in handles {
        let consumed = handle.await.expect("Consumer task failed");
        total_consumed += consumed;
    }

    assert_eq!(total_consumed, 100, "All 100 messages should be consumed exactly once");
}

#[tokio::test]
async fn test_pool_stress() {
    if !redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let config = RedisConfig {
        url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string()),
        pool_size: 20, // Limited pool size to test contention
        batch_size: 10,
        enable_dead_letter: true,
        consumer_rebalance_interval_seconds: 300,
        metrics_collection_interval_seconds: 60,
        connection_timeout_ms: 2000, // Shorter timeout to detect issues faster
        idle_timeout_secs: 300,
        max_connection_lifetime_secs: 300,
    };

    let redis = Arc::new(Redis::new(&config).await.expect("Failed to create Redis client"));

    // Spawn many concurrent operations to stress the pool
    let mut handles = Vec::new();
    for i in 0..50 {
        let redis = redis.clone();
        let handle = tokio::spawn(async move {
            let key = format!("stress:test:{}", i);
            let mut successes = 0;
            let mut timeouts = 0;

            for j in 0..20 {
                let data = format!("stress message {}-{}", i, j).into_bytes();
                match redis.xadd(&key, &data).await {
                    Ok(_) => successes += 1,
                    Err(e) if e.to_string().contains("timeout") => timeouts += 1,
                    Err(e) => panic!("Unexpected error: {}", e),
                }
            }

            (successes, timeouts)
        });
        handles.push(handle);
    }

    // Collect results
    let mut total_successes = 0;
    let mut total_timeouts = 0;
    for handle in handles {
        let (successes, timeouts) = handle.await.expect("Task failed");
        total_successes += successes;
        total_timeouts += timeouts;
    }

    println!("Pool stress test: {} successes, {} timeouts", total_successes, total_timeouts);

    // With proper pool management, we should have mostly successes
    assert!(total_successes > 900, "Should have >90% success rate under stress");
    assert!(total_timeouts < 100, "Should have <10% timeout rate under stress");
}
