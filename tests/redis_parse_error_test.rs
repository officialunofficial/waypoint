//! Test cases to reproduce and verify the Redis parse error fix
//!
//! These tests specifically target the "Parse Error: Could not convert to string"
//! issue that was occurring with XREADGROUP operations on binary data.

#[cfg(test)]
mod tests {
    use fred::prelude::*;
    use waypoint::config::RedisConfig;
    use waypoint::redis::client::Redis;

    // Helper to create test configuration
    fn test_config() -> RedisConfig {
        RedisConfig {
            url: "redis://localhost:6379".to_string(),
            max_pool_size: 5,
            batch_size: 10,
            enable_dead_letter: false,
            consumer_rebalance_interval_seconds: 300,
            metrics_collection_interval_seconds: 60,
            connection_timeout_ms: 5000,
            circuit_breaker: waypoint::config::CircuitBreakerConfig::default(),
        }
    }

    #[tokio::test]
    #[ignore] // Only run when Redis is available
    async fn test_xreadgroup_with_binary_data() {
        let config = test_config();
        let redis = Redis::new(&config).await.expect("Failed to connect to Redis");

        let stream_key = "test:binary_stream";
        let group_name = "test_group";
        let consumer_name = "test_consumer";

        // Cleanup any existing test data
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;

        // Create some binary data similar to what we store in production
        let binary_data = vec![
            0x08, 0x01, 0x12, 0x14, 0x68, 0x75, 0x62, 0x3a, 0x73, 0x6e, 0x61, 0x70, 0x2e, 0x75,
            0x6e, 0x6f, 0x2e, 0x66, 0x75, 0x6e, 0x1a, 0x10, 0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xff, 0x62, 0x60, 0x60, 0x60,
        ];

        // Add binary data to stream using our xadd method
        let message_id =
            redis.xadd(stream_key, &binary_data).await.expect("Failed to add message to stream");

        println!("Added message with ID: {}", message_id);

        // Create consumer group - ignore errors if group already exists
        let _: Result<String, _> =
            redis.pool.xgroup_create(stream_key, group_name, "$", true).await;

        // Now try to read the binary data using our xreadgroup method
        // This should not produce a "Parse Error: Could not convert to string"
        let result = redis.xreadgroup(group_name, consumer_name, stream_key, 10).await;

        match result {
            Ok(messages) => {
                println!("Successfully read {} messages", messages.len());
                assert!(!messages.is_empty(), "Should have read at least one message");

                // Verify the binary data was read correctly
                let (read_id, read_data) = &messages[0];
                println!("Read message ID: {}, data length: {}", read_id, read_data.len());
                assert_eq!(read_data, &binary_data, "Binary data should match what was stored");

                // Acknowledge the message
                let _ = redis.xack(stream_key, group_name, [read_id.clone()]).await;
            },
            Err(e) => {
                panic!("XREADGROUP failed with binary data: {}", e);
            },
        }

        // Cleanup
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;
    }

    #[tokio::test]
    #[ignore] // Only run when Redis is available  
    async fn test_xreadgroup_with_mixed_data_types() {
        let config = test_config();
        let redis = Redis::new(&config).await.expect("Failed to connect to Redis");

        let stream_key = "test:mixed_stream";
        let group_name = "test_group";
        let consumer_name = "test_consumer";

        // Cleanup any existing test data
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;

        // Add different types of data to the stream
        let string_data = "hello world".as_bytes().to_vec();
        let binary_data = vec![0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD];
        let json_like_data = r#"{"type":"cast","data":"test"}"#.as_bytes().to_vec();

        // Add messages with different data types using our xadd method
        let _ = redis.xadd(stream_key, &string_data).await.expect("Failed to add string message");
        let _ = redis.xadd(stream_key, &binary_data).await.expect("Failed to add binary message");
        let _ = redis.xadd(stream_key, &json_like_data).await.expect("Failed to add JSON message");

        // Create consumer group - ignore errors if group already exists
        let _: Result<String, _> =
            redis.pool.xgroup_create(stream_key, group_name, "$", true).await;

        // Read all messages - should handle mixed data types without parse errors
        let result = redis.xreadgroup(group_name, consumer_name, stream_key, 10).await;

        match result {
            Ok(messages) => {
                println!("Successfully read {} messages with mixed data types", messages.len());
                assert_eq!(messages.len(), 3, "Should have read 3 messages");

                // Verify each message was read correctly
                assert_eq!(messages[0].1, string_data);
                assert_eq!(messages[1].1, binary_data);
                assert_eq!(messages[2].1, json_like_data);

                // Acknowledge all messages in batch
                let ids: Vec<String> = messages.iter().map(|(id, _)| id.clone()).collect();
                let _ = redis.xack(stream_key, group_name, ids).await;
            },
            Err(e) => {
                panic!("XREADGROUP failed with mixed data types: {}", e);
            },
        }

        // Cleanup
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;
    }

    #[tokio::test]
    #[ignore] // Only run when Redis is available
    async fn test_xclaim_with_binary_data() {
        let config = test_config();
        let redis = Redis::new(&config).await.expect("Failed to connect to Redis");

        let stream_key = "test:xclaim_stream";
        let group_name = "test_group";
        let consumer1 = "consumer1";
        let consumer2 = "consumer2";

        // Cleanup any existing test data
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;

        // Create binary data similar to production
        let binary_data = vec![
            0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0xF9, 0xF8, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
            0x06, 0x07,
        ];

        // Add binary data to stream
        let message_id = redis.xadd(stream_key, &binary_data).await.expect("Failed to add message");
        println!("Added message with ID: {}", message_id);

        // Create consumer group at the beginning of the stream
        let _: Result<String, _> =
            redis.pool.xgroup_create(stream_key, group_name, "0", true).await;

        // Read with consumer1 to make it pending
        let _: Result<fred::types::RedisValue, _> = redis
            .pool
            .xreadgroup(group_name, consumer1, Some(1), Some(0), false, stream_key, ">")
            .await;

        // Wait a bit to make the message stale
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Now try to claim the message with consumer2 - this should not produce a parse error
        let result = redis
            .xclaim(
                stream_key,
                group_name,
                consumer2,
                std::time::Duration::from_millis(50),
                std::slice::from_ref(&message_id),
            )
            .await;

        match result {
            Ok(claimed) => {
                println!("Successfully claimed {} messages", claimed.len());
                assert!(!claimed.is_empty(), "Should have claimed at least one message");

                // Verify the binary data was claimed correctly
                let (claimed_id, claimed_data) = &claimed[0];
                println!("Claimed message ID: {}, data length: {}", claimed_id, claimed_data.len());
                assert_eq!(claimed_data, &binary_data, "Binary data should match what was stored");

                // Acknowledge the message
                let _ = redis.xack(stream_key, group_name, [claimed_id.clone()]).await;
            },
            Err(e) => {
                panic!("XCLAIM failed with binary data: {}", e);
            },
        }

        // Cleanup
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;
    }

    #[tokio::test]
    #[ignore] // Only run when Redis is available
    async fn test_xreadgroup_empty_stream() {
        let config = test_config();
        let redis = Redis::new(&config).await.expect("Failed to connect to Redis");

        let stream_key = "test:empty_stream";
        let group_name = "test_group";
        let consumer_name = "test_consumer";

        // Cleanup any existing test data
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;

        // Create an empty stream by adding and then deleting a message
        let temp_data = b"temp".to_vec();
        let msg_id = redis.xadd(stream_key, &temp_data).await.expect("Failed to create stream");
        let _: u64 =
            redis.pool.xdel(stream_key, msg_id).await.expect("Failed to delete temp message");

        // Create consumer group on empty stream - ignore errors if group already exists
        let _: Result<String, _> =
            redis.pool.xgroup_create(stream_key, group_name, "0", true).await;

        // Reading from empty stream should return empty vec, not parse error
        let result = redis.xreadgroup(group_name, consumer_name, stream_key, 10).await;

        match result {
            Ok(messages) => {
                println!("Successfully read from empty stream: {} messages", messages.len());
                assert!(messages.is_empty(), "Empty stream should return empty vec");
            },
            Err(e) => {
                panic!("XREADGROUP failed on empty stream: {}", e);
            },
        }

        // Cleanup
        let _: Result<u64, _> = redis.pool.del(stream_key).await;
        let _: Result<u64, _> = redis.pool.xgroup_destroy(stream_key, group_name).await;
    }
}
