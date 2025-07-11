use crate::{
    core::root_parent_hub::{find_root_parent_hub_with_retry, CAST_RETRY_STREAM, CAST_RETRY_DEAD, MAX_RETRY_ATTEMPTS},
    database::client::Database,
    hub::providers::FarcasterHubClient,
    redis::client::Redis,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn, error, debug};

/// Retry worker for processing failed cast insertions
pub struct CastRetryWorker {
    redis: Arc<Redis>,
    database: Arc<Database>,
    hub_client: FarcasterHubClient,
}

impl CastRetryWorker {
    pub fn new(
        redis: Arc<Redis>,
        database: Arc<Database>,
        hub: Arc<Mutex<crate::hub::client::Hub>>,
    ) -> Self {
        let hub_client = FarcasterHubClient::new(hub);
        Self {
            redis,
            database,
            hub_client,
        }
    }

    /// Run the retry worker
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting cast retry worker");

        loop {
            if let Err(e) = self.process_retry_batch().await {
                error!("Error processing retry batch: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
            }

            // Sleep between batches
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }

    /// Process a batch of retry messages
    async fn process_retry_batch(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Read messages from retry stream
        let messages = self.redis.xreadgroup(
            "retry_workers",
            "worker_1", 
            CAST_RETRY_STREAM,
            10  // Process 10 at a time
        ).await?;

        if messages.is_empty() {
            return Ok(());
        }

        info!("Processing {} retry messages", messages.len());

        for (msg_id, msg_data) in messages {
            if let Err(e) = self.process_retry_message(&msg_id, &msg_data).await {
                error!("Failed to process retry message {}: {}", msg_id, e);
                // Message will remain in pending and can be reclaimed later
            } else {
                // Acknowledge successful processing
                let _ = self.redis.xack(CAST_RETRY_STREAM, "retry_workers", &msg_id).await;
                debug!("Successfully processed and acked message {}", msg_id);
            }
        }

        Ok(())
    }

    /// Process a single retry message
    async fn process_retry_message(
        &self,
        msg_id: &str,
        msg_data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Parse the message data (in real implementation, you'd use proper serialization)
        let msg_str = String::from_utf8_lossy(msg_data);
        debug!("Processing retry message {}: {}", msg_id, msg_str);

        // For now, just parse the basic fields from the Redis stream
        // In a real implementation, you'd use proper JSON or protobuf serialization
        let fields: Vec<&str> = msg_str.split('|').collect();
        if fields.len() < 6 {
            return Err("Invalid message format".into());
        }

        let cast_fid: Option<i64> = fields[0].parse().ok();
        let cast_hash = if fields[1].is_empty() { None } else { Some(hex::decode(fields[1])?) };
        let parent_fid: Option<i64> = fields[2].parse().ok();
        let parent_hash = if fields[3].is_empty() { None } else { Some(hex::decode(fields[3])?) };
        let parent_url = if fields[4].is_empty() { None } else { Some(fields[4]) };
        let attempt: u64 = fields[5].parse().unwrap_or(1);

        // Try to resolve the root parent again
        let result = find_root_parent_hub_with_retry(
            &self.hub_client,
            &*self.redis,
            parent_fid,
            parent_hash.as_ref().map(|h| h.as_slice()),
            parent_url,
            cast_fid,
            cast_hash.as_ref().map(|h| h.as_slice()),
        ).await;

        match result {
            Ok(Some(_root_info)) => {
                info!("Successfully resolved root parent for cast on retry attempt {}", attempt);
                // In a real implementation, you'd update the database with the resolved parent info
                Ok(())
            },
            Ok(None) => {
                // No root parent needed, this is fine
                info!("No root parent needed for cast, retry successful");
                Ok(())
            },
            Err(e) => {
                let new_attempt = attempt + 1;
                warn!("Retry attempt {} failed: {}", new_attempt, e);
                
                if new_attempt >= MAX_RETRY_ATTEMPTS {
                    // Move to dead letter queue
                    warn!("Moving cast to dead letter queue after {} attempts", new_attempt);
                    self.move_to_dead_letter(cast_fid, &cast_hash, parent_fid, &parent_hash, parent_url, &e.to_string()).await?;
                } else {
                    // Add back to retry queue with incremented attempt
                    self.add_back_to_retry(cast_fid, &cast_hash, parent_fid, &parent_hash, parent_url, new_attempt).await?;
                }
                
                Err(e)
            }
        }
    }

    /// Move failed cast to dead letter queue
    async fn move_to_dead_letter(
        &self,
        cast_fid: Option<i64>,
        cast_hash: &Option<Vec<u8>>,
        parent_fid: Option<i64>,
        parent_hash: &Option<Vec<u8>>,
        parent_url: Option<&str>,
        error: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cast_hash_hex = cast_hash.as_ref().map(hex::encode).unwrap_or_default();
        let parent_hash_hex = parent_hash.as_ref().map(hex::encode).unwrap_or_default();
        
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
        
        let _: Result<String, _> = bb8_redis::redis::cmd("XADD")
            .arg(CAST_RETRY_DEAD)
            .arg("*")
            .arg("cast_fid").arg(cast_fid.unwrap_or(0))
            .arg("cast_hash").arg(&cast_hash_hex)
            .arg("parent_fid").arg(parent_fid.unwrap_or(0))
            .arg("parent_hash").arg(&parent_hash_hex)
            .arg("parent_url").arg(parent_url.unwrap_or(""))
            .arg("final_error").arg(error)
            .arg("timestamp").arg(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
            .query_async(&mut *conn)
            .await;
        
        Ok(())
    }

    /// Add cast back to retry queue with incremented attempt count
    async fn add_back_to_retry(
        &self,
        cast_fid: Option<i64>,
        cast_hash: &Option<Vec<u8>>,
        parent_fid: Option<i64>,
        parent_hash: &Option<Vec<u8>>,
        parent_url: Option<&str>,
        attempt: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cast_hash_hex = cast_hash.as_ref().map(hex::encode).unwrap_or_default();
        let parent_hash_hex = parent_hash.as_ref().map(hex::encode).unwrap_or_default();
        
        // Add exponential backoff delay
        let delay_seconds = 2_u64.pow((attempt - 1) as u32).min(300); // Max 5 minutes
        let retry_after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + delay_seconds;
        
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
        
        let _: Result<String, _> = bb8_redis::redis::cmd("XADD")
            .arg(CAST_RETRY_STREAM)
            .arg("*")
            .arg("cast_fid").arg(cast_fid.unwrap_or(0))
            .arg("cast_hash").arg(&cast_hash_hex)
            .arg("parent_fid").arg(parent_fid.unwrap_or(0))
            .arg("parent_hash").arg(&parent_hash_hex)
            .arg("parent_url").arg(parent_url.unwrap_or(""))
            .arg("attempt").arg(attempt)
            .arg("retry_after").arg(retry_after)
            .query_async(&mut *conn)
            .await;
        
        debug!("Added cast back to retry queue: attempt={}, retry_after={}", attempt, retry_after);
        Ok(())
    }

    /// Create consumer group if it doesn't exist
    pub async fn ensure_consumer_group(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
        
        // Try to create consumer group, ignore error if it already exists
        let _: Result<String, _> = bb8_redis::redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(CAST_RETRY_STREAM)
            .arg("retry_workers")
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut *conn)
            .await;
        
        info!("Consumer group 'retry_workers' ensured for stream '{}'", CAST_RETRY_STREAM);
        Ok(())
    }
}

/// Admin functions for managing the retry system
pub struct CastRetryAdmin {
    redis: Arc<Redis>,
}

impl CastRetryAdmin {
    pub fn new(redis: Arc<Redis>) -> Self {
        Self { redis }
    }

    /// Get retry queue statistics
    pub async fn get_retry_stats(&self) -> Result<RetryStats, Box<dyn std::error::Error + Send + Sync>> {
        let retry_count = self.redis.xlen(CAST_RETRY_STREAM).await.unwrap_or(0);
        let dead_count = self.redis.xlen(CAST_RETRY_DEAD).await.unwrap_or(0);
        
        Ok(RetryStats {
            retry_queue_length: retry_count,
            dead_letter_length: dead_count,
        })
    }

    /// Reprocess all dead letter messages (move them back to retry queue)
    pub async fn reprocess_dead_letters(&self) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        info!("Reprocessing all dead letter messages");
        
        // Read all messages from dead letter queue
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
        
        let messages: Result<Vec<(String, Vec<(String, Vec<u8>)>)>, _> = bb8_redis::redis::cmd("XRANGE")
            .arg(CAST_RETRY_DEAD)
            .arg("-")
            .arg("+")
            .query_async(&mut *conn)
            .await;
        
        let mut moved_count = 0;
        if let Ok(messages) = messages {
            for (msg_id, _fields) in messages {
                // Move message to retry queue (simplified)
                let _: Result<String, _> = bb8_redis::redis::cmd("XADD")
                    .arg(CAST_RETRY_STREAM)
                    .arg("*")
                    .arg("reprocessed_from").arg(&msg_id)
                    .arg("attempt").arg(1)
                    .query_async(&mut *conn)
                    .await;
                
                // Delete from dead letter queue
                let _: Result<u64, _> = bb8_redis::redis::cmd("XDEL")
                    .arg(CAST_RETRY_DEAD)
                    .arg(&msg_id)
                    .query_async(&mut *conn)
                    .await;
                
                moved_count += 1;
            }
        }
        
        info!("Moved {} messages from dead letter to retry queue", moved_count);
        Ok(moved_count)
    }

    /// Clear negative cache (force retry of all cached failures)
    pub async fn clear_negative_cache(&self) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.redis.pool.get().await.map_err(|e| format!("Redis error: {}", e))?;
        
        // Use SCAN to find all failed_lookup keys
        let keys: Result<Vec<String>, _> = bb8_redis::redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg("failed_lookup:*")
            .arg("COUNT")
            .arg("1000")
            .query_async(&mut *conn)
            .await;
        
        let mut deleted_count = 0;
        if let Ok(scan_result) = keys {
            // scan_result is [cursor, [key1, key2, ...]]
            // For simplicity, just delete what we found in this scan
            if scan_result.len() > 1 {
                let keys_to_delete: Vec<&str> = scan_result[1..].iter().map(|s| s.as_str()).collect();
                if !keys_to_delete.is_empty() {
                    let result: Result<u64, _> = bb8_redis::redis::cmd("DEL")
                        .arg(&keys_to_delete)
                        .query_async(&mut *conn)
                        .await;
                    deleted_count = result.unwrap_or(0);
                }
            }
        }
        
        info!("Cleared {} negative cache entries", deleted_count);
        Ok(deleted_count)
    }
}

#[derive(Debug)]
pub struct RetryStats {
    pub retry_queue_length: u64,
    pub dead_letter_length: u64,
}