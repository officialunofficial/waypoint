//! Streaming service for processing real-time Hub events
use crate::{
    app::{
        AppError, ProcessorRegistry, Result, Service, ServiceContext, ServiceError, ServiceHandle,
    },
    core::MessageType,
    hub::subscriber::{HubSubscriber, SubscriberOptions},
    proto::HubEvent,
    redis::stream::RedisStream,
};
use async_trait::async_trait;
use prost::Message as ProstMessage;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock, oneshot},
    task::JoinHandle,
};
use tracing::{error, info, trace, warn};

const DEFAULT_BATCH_SIZE: u64 = 10;
const DEFAULT_CONCURRENCY: usize = 200;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_EVENT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Consumer state used across tasks
#[derive(Default)]
struct ConsumerState {
    shutdown: bool,
    startup_complete: bool,
}

/// Consumer for processing message batches
pub struct Consumer {
    stream: Arc<RedisStream>,
    processors: Arc<ProcessorRegistry>,
    hub_host: String,
    group_name: String,
    state: Arc<RwLock<ConsumerState>>,
    batch_size: u64,
    concurrency: usize,
    timeout: Duration,
    retention: Duration,
}

impl Consumer {
    /// Create a new consumer
    pub fn new(
        stream: Arc<RedisStream>,
        processors: Arc<ProcessorRegistry>,
        hub_host: String,
        group_name: String,
    ) -> Self {
        Self {
            stream,
            processors,
            hub_host,
            group_name,
            state: Arc::new(RwLock::new(ConsumerState::default())),
            batch_size: DEFAULT_BATCH_SIZE,
            concurrency: DEFAULT_CONCURRENCY,
            timeout: DEFAULT_TIMEOUT,
            retention: DEFAULT_EVENT_RETENTION,
        }
    }

    /// Set batch size
    pub fn with_batch_size(mut self, size: u64) -> Self {
        self.batch_size = size;
        self
    }

    /// Set processing concurrency
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Set processing timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set event retention
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    /// Start the consumer
    pub async fn start(self) -> JoinHandle<()> {
        let consumer = Arc::new(self);

        // Set initial state
        {
            let mut state = consumer.state.write().await;
            state.shutdown = false;
            state.startup_complete = false;
        }

        // Start stream tasks for each message type
        let mut handles = Vec::new();

        // Create a barrier for synchronizing startup across all tasks
        // Count message types for calculating channel capacity
        let message_types_count = MessageType::all().collect::<Vec<_>>().len();
        let total_tasks = message_types_count * 2 + 1; // *2 for processor and cleanup tasks + 1 for consumer cleanup
        let (startup_tx, mut startup_rx) = tokio::sync::mpsc::channel(total_tasks);

        // Start consumer cleanup task to periodically clean up idle consumers
        let consumer_clone = Arc::clone(&consumer);
        let startup_tx_clone = startup_tx.clone();
        let consumer_cleanup_handle = tokio::spawn(async move {
            // Signal that initialization is starting
            let _ = startup_tx_clone.send(()).await;

            // Wait briefly to avoid contention
            tokio::time::sleep(Duration::from_millis(10)).await;

            // Run the consumer cleanup task periodically
            consumer_clone.run_consumer_cleanup().await;
            info!("Consumer cleanup task shut down");
        });
        handles.push(consumer_cleanup_handle);

        for (index, message_type) in MessageType::all().enumerate() {
            let consumer_clone = Arc::clone(&consumer);
            let startup_tx_clone = startup_tx.clone();

            let handle = tokio::spawn(async move {
                // Signal that initialization is about to start
                let _ = startup_tx_clone.send(()).await;

                // Stagger startup to prevent thundering herd on Redis pool
                // Each processor waits an additional 50ms to spread out the initial connections
                tokio::time::sleep(Duration::from_millis(10 + (50 * index as u64))).await;

                if let Err(e) = consumer_clone.process_stream(message_type).await {
                    error!("Stream processor error for {:?}: {}", message_type, e);
                }
                info!("Stream processor for {:?} shut down", message_type);
            });
            handles.push(handle);
        }

        // Start cleanup task for old events and track their handles
        let mut cleanup_handles = Vec::new();
        for (index, message_type) in MessageType::all().enumerate() {
            let consumer_clone = Arc::clone(&consumer);
            let startup_tx_clone = startup_tx.clone();

            let cleanup_handle = tokio::spawn(async move {
                // Signal that initialization is about to start
                let _ = startup_tx_clone.send(()).await;

                // Stagger cleanup tasks even more to avoid contention
                // Cleanup is less critical, so we can wait longer
                tokio::time::sleep(Duration::from_millis(10 + (100 * index as u64))).await;

                consumer_clone.cleanup_old_events(message_type).await;
                info!("Cleanup task for {:?} shut down", message_type);
            });
            cleanup_handles.push(cleanup_handle);
        }

        // Drop our reference to the sender so the receiver can complete
        drop(startup_tx);

        // Wait for all tasks to signal they're ready to start
        let consumer_clone = Arc::clone(&consumer);

        // Spawn a dedicated task to track initialization and set the startup_complete flag
        tokio::spawn(async move {
            // Wait for all tasks to signal they've started initializing
            let mut count = 0;
            while let Some(()) = startup_rx.recv().await {
                count += 1;
                // For debugging purposes
                trace!("Task initialization signal received ({}/{})", count, total_tasks);
            }

            info!("All {} tasks initialized. Marking startup as complete.", count);

            // Now mark startup as complete - IMPORTANT: This is what unblocks the processing!
            {
                let mut state = consumer_clone.state.write().await;
                state.startup_complete = true;

                // Add this to explicitly log the state change
                info!("Startup phase complete - processors will now begin processing events");
            }
        });

        // Add an immediate initialization for the state to mark it as started
        {
            let mut state = consumer.state.write().await;
            state.startup_complete = true;
            info!("Preemptively marking startup as complete to allow immediate processing");
        }

        // Return a handle that waits for all tasks and reports progress
        tokio::spawn(async move {
            // Use join_set to manage both types of tasks
            let mut remaining_tasks = handles.len() + cleanup_handles.len();
            info!("Consumer monitoring {} tasks for graceful shutdown", remaining_tasks);

            // Wait for stream processors
            for handle in handles {
                if let Err(e) = handle.await {
                    error!("Stream processor task error: {}", e);
                }
                remaining_tasks -= 1;
                info!("Consumer shutdown progress: {} tasks remaining", remaining_tasks);
            }

            // Wait for cleanup tasks
            for handle in cleanup_handles {
                if let Err(e) = handle.await {
                    error!("Cleanup task error: {}", e);
                }
                remaining_tasks -= 1;
                info!("Consumer shutdown progress: {} tasks remaining", remaining_tasks);
            }

            info!("All consumer tasks shut down successfully");
        })
    }

    /// Stop the consumer
    pub async fn stop(&self) {
        // Mark the shutdown flag
        let mut state = self.state.write().await;
        state.shutdown = true;
    }

    /// Run a periodic task to clean up idle consumers from Redis streams
    /// and force reclaim any stuck messages that have been in the pending state too long
    pub async fn run_consumer_cleanup(&self) {
        // Constants for cleanup behavior
        const CLEANUP_INTERVAL_SECS: u64 = 30 * 60; // 30 minutes
        // Threshold for considering consumers/messages as extremely idle (1 hour)
        const EXTREME_IDLE_THRESHOLD_MS: u64 = 3600000; // 1 hour in milliseconds

        // Create a cleanup interval with skipped tick behavior to avoid queuing
        let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Wait for the first tick to occur immediately
        interval.tick().await;

        while !self.should_shutdown().await {
            // Wait for the next interval or shutdown using select for responsiveness
            tokio::select! {
                _ = interval.tick() => {},
                _ = self.wait_for_shutdown() => {
                    info!("Shutdown signal detected in consumer cleanup task");
                    break;
                }
            }

            // Run consumer cleanup for all streams with the default group
            info!("Starting scheduled consumer cleanup for all streams");
            self.cleanup_all_consumer_groups(EXTREME_IDLE_THRESHOLD_MS).await;
            info!("Scheduled consumer cleanup completed");
        }
    }

    /// Clean up idle consumers from all streams
    ///
    /// Finds all Redis stream keys matching the hub pattern and cleans up idle consumers
    /// from each stream that has the specified consumer group.
    ///
    /// # Arguments
    /// * `idle_threshold` - Milliseconds threshold for considering a consumer extremely idle
    async fn cleanup_all_consumer_groups(&self, idle_threshold: u64) {
        // Find all stream keys matching our pattern using the Redis client
        let keys_result = self.stream.redis.keys("hub:*:stream:*").await;

        let mut total_deleted = 0;
        let mut total_reclaimed = 0;

        match keys_result {
            Ok(keys) => {
                info!("Found {} stream keys to clean up", keys.len());

                for stream_key in keys {
                    let stream_key_str = stream_key.as_str(); // Convert to &str
                    // Check if this stream has the specified consumer group (default)
                    let has_group =
                        self.check_stream_has_group(stream_key_str, &self.group_name).await;

                    if has_group {
                        // First try to reclaim any extremely stale messages
                        if let Ok(reclaimed) = self
                            .force_reclaim_stale_messages(
                                stream_key_str,
                                &self.group_name,
                                idle_threshold,
                            )
                            .await
                        {
                            total_reclaimed += reclaimed;
                        }

                        // Then cleanup idle consumers
                        let deleted = self
                            .cleanup_consumer_group(
                                stream_key_str,
                                &self.group_name,
                                idle_threshold,
                            )
                            .await;
                        total_deleted += deleted;
                    }
                }

                info!(
                    "Consumer cleanup complete: reclaimed {} stale messages, deleted {} idle consumers",
                    total_reclaimed, total_deleted
                );
            },
            Err(e) => {
                error!("Error getting stream keys for cleanup: {}", e);
            },
        }
    }

    /// Check if a stream has a specific consumer group
    ///
    /// # Arguments
    /// * `conn` - Redis connection pool
    /// * `stream_key` - The Redis stream key to check
    /// * `group_name` - The consumer group name to look for
    ///
    /// # Returns
    /// * `bool` - true if the stream has the specified consumer group, false otherwise
    async fn check_stream_has_group(&self, stream_key: &str, group_name: &str) -> bool {
        // Use the Redis client to get group info
        match self.stream.redis.xinfo_groups(stream_key).await {
            Ok(groups) => {
                for group in groups {
                    if let Some(name) = group.get("name") {
                        if name == group_name {
                            return true;
                        }
                    }
                }
                false
            },
            Err(_) => false,
        }
    }

    /// Clean up idle consumers for a specific stream and group
    ///
    /// This function identifies extremely idle consumers and removes them after ensuring
    /// any pending messages have been claimed by the waypoint consumer.
    ///
    /// # Arguments
    /// * `stream_key` - The Redis stream key
    /// * `group_name` - The consumer group name
    /// * `idle_threshold` - Milliseconds threshold for considering a consumer extremely idle
    ///
    /// # Returns
    /// * `usize` - Number of consumers deleted
    async fn cleanup_consumer_group(
        &self,
        stream_key: &str,
        group_name: &str,
        idle_threshold: u64,
    ) -> usize {
        // Get list of consumers in the group
        match self.stream.redis.xinfo_consumers(stream_key, group_name).await {
            Ok(consumers) => {
                let mut deleted_count = 0;
                let current_consumer = crate::redis::stream::RedisStream::get_stable_consumer_id();

                for consumer_info in consumers {
                    // Get consumer name and idle time
                    let consumer_name = consumer_info.get("name").map(|s| s.as_str()).unwrap_or("");
                    let idle_time: u64 =
                        consumer_info.get("idle").and_then(|s| s.parse().ok()).unwrap_or(0);

                    // Skip current consumer and recently active consumers
                    if consumer_name == current_consumer || consumer_name.is_empty() {
                        continue;
                    }

                    // Check if consumer is extremely idle
                    if idle_time > idle_threshold {
                        // Check pending count for this consumer
                        let pending_count: u64 =
                            consumer_info.get("pending").and_then(|s| s.parse().ok()).unwrap_or(0);

                        // If consumer has pending messages, try to claim them first
                        if pending_count > 0 {
                            trace!(
                                "Consumer {} has {} pending messages, attempting to claim before deletion",
                                consumer_name, pending_count
                            );

                            // Try to claim any pending messages from this consumer
                            match self
                                .stream
                                .claim_stale(
                                    stream_key,
                                    group_name,
                                    Duration::from_millis(1), // Immediate claim since we're deleting
                                    pending_count as usize,
                                    Some(&current_consumer),
                                )
                                .await
                            {
                                Ok(claimed) => {
                                    trace!(
                                        "Claimed {} messages from idle consumer {}",
                                        claimed.len(),
                                        consumer_name
                                    );
                                    // Acknowledge the claimed messages to clear them
                                    let ids: Vec<String> =
                                        claimed.into_iter().map(|e| e.id).collect();
                                    if !ids.is_empty() {
                                        let _ = self.stream.ack(stream_key, group_name, ids).await;
                                    }
                                },
                                Err(e) => {
                                    warn!(
                                        "Failed to claim messages from idle consumer {}: {}",
                                        consumer_name, e
                                    );
                                },
                            }
                        }

                        // Delete the idle consumer
                        match self
                            .stream
                            .delete_consumer(stream_key, group_name, consumer_name)
                            .await
                        {
                            Ok(_) => {
                                deleted_count += 1;
                                trace!(
                                    "Deleted idle consumer {} (idle for {} ms)",
                                    consumer_name, idle_time
                                );
                            },
                            Err(e) => {
                                warn!("Failed to delete idle consumer {}: {}", consumer_name, e);
                            },
                        }
                    }
                }

                deleted_count
            },
            Err(e) => {
                warn!("Failed to get consumer info for cleanup: {}", e);
                0
            },
        }
    }

    /// Force reclaim any messages stuck in the pending state for too long
    ///
    /// This function finds messages that have been pending for longer than the specified
    /// idle threshold and forcibly reclaims them using the FORCE option with XCLAIM.
    /// It also attempts to acknowledge them to clear them from the pending queue.
    ///
    /// # Arguments
    /// * `stream_key` - The Redis stream key
    /// * `group_name` - The consumer group name
    /// * `idle_threshold` - Milliseconds threshold for considering a message extremely stale
    ///
    /// # Returns
    /// * `std::result::Result<usize, crate::redis::error::Error>` - Number of messages reclaimed or error
    async fn force_reclaim_stale_messages(
        &self,
        stream_key: &str,
        group_name: &str,
        idle_threshold: u64,
    ) -> std::result::Result<usize, crate::redis::error::Error> {
        // Constants for batch processing
        const BATCH_SIZE: usize = 100;

        // No need to get connection with fred - it handles this internally

        // Get pending messages count using simplified approach
        let pending_items =
            self.stream.redis.xpending(stream_key, group_name, Duration::from_millis(1), 1).await?;

        let pending_count = if pending_items.is_empty() {
            0
        } else {
            // Since we can't get exact count easily, we'll process in batches
            // and keep going until no more pending messages
            1000 // Assume max 1000 pending messages to process
        };

        // Track total reclaimed messages
        let mut total_reclaimed = 0;

        if pending_count > 0 {
            info!(
                "[{}] Found {} pending messages to check for reclamation",
                stream_key, pending_count
            );

            // Process in batches for better performance and to avoid timeout issues
            let mut processed = 0;
            while processed < pending_count {
                // Get pending messages with minimal idle time
                let pending_msgs = self
                    .stream
                    .redis
                    .xpending(
                        stream_key,
                        group_name,
                        Duration::from_millis(idle_threshold),
                        BATCH_SIZE as u64,
                    )
                    .await;

                // Process the pending messages

                match pending_msgs {
                    Ok(pending_items) => {
                        let pending_msgs = pending_items;
                        if pending_msgs.is_empty() {
                            break; // No more messages to process
                        }

                        processed += pending_msgs.len() as u64;

                        if !pending_msgs.is_empty() {
                            let reclaim_count = pending_msgs.len();
                            info!(
                                "[{}] Found {} extremely stale messages to force reclaim",
                                stream_key, reclaim_count
                            );

                            // Get our stable consumer ID
                            let waypoint_consumer =
                                crate::redis::stream::RedisStream::get_stable_consumer_id();

                            // Extract message IDs
                            let msg_ids: Vec<String> =
                                pending_msgs.iter().map(|item| item.id.clone()).collect();

                            // Force claim with XCLAIM
                            let claim_result = self
                                .stream
                                .redis
                                .xclaim(
                                    stream_key,
                                    group_name,
                                    &waypoint_consumer,
                                    Duration::from_millis(0),
                                    &msg_ids,
                                )
                                .await;

                            match claim_result {
                                Ok(_) => {
                                    info!(
                                        "[{}] Successfully force reclaimed {} stale messages",
                                        stream_key, reclaim_count
                                    );
                                    total_reclaimed += reclaim_count;

                                    // Try to acknowledge them to clear the backlog
                                    for id in &msg_ids {
                                        if let Err(e) =
                                            self.stream.redis.xack(stream_key, group_name, id).await
                                        {
                                            error!(
                                                "[{}] Error acknowledging stale message {}: {}",
                                                stream_key, id, e
                                            );
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!(
                                        "[{}] Error force reclaiming stale messages: {}",
                                        stream_key, e
                                    );
                                },
                            }
                        }
                    },
                    Err(e) => {
                        error!(
                            "[{}] Error getting pending message details: {} - Response type not vector compatible",
                            stream_key, e
                        );
                        // Continue processing instead of breaking to handle transient errors
                        processed = pending_count; // Exit the loop
                    },
                }
            }
        }

        Ok(total_reclaimed)
    }

    /// Process events from a specific stream
    async fn process_stream(
        &self,
        message_type: MessageType,
    ) -> std::result::Result<(), crate::redis::error::Error> {
        // Use the same key format that publisher uses
        let clean_host = self.hub_host.split(':').next().unwrap_or(&self.hub_host);

        let stream_key = crate::types::get_stream_key(clean_host, message_type.to_stream_key());
        let group_name = self.group_name.clone();

        // Create the consumer group if it doesn't exist
        if let Err(e) = self.stream.create_group(&stream_key, &group_name).await {
            error!("Error creating group for {}: {}", stream_key, e);
            return Err(e);
        }

        trace!("Starting stream processor for {:?}", message_type);

        // Main processing loop
        while !self.should_shutdown().await {
            // 1. Reserve messages from stream
            let entries =
                match self.try_reserve_messages(&stream_key, &group_name, message_type).await {
                    Ok(Some(entries)) => entries,
                    Ok(None) => continue, // No entries or shutdown requested
                    Err(e) => return Err(e),
                };

            // 2. Process messages
            let successful_ids = self.process_message_batch(entries, message_type).await;

            // 3. Acknowledge processed messages
            if !successful_ids.is_empty() {
                if self.should_shutdown().await {
                    info!("Shutdown signal detected before acknowledgment for {:?}", message_type);
                    break;
                }

                if let Err(e) =
                    self.stream.ack(&stream_key, &group_name, successful_ids.clone()).await
                {
                    error!("Failed to acknowledge messages: {}", e);
                }
            }

            // Check shutdown before next iteration
            if self.should_shutdown().await {
                info!("Shutdown signal detected at end of processing loop for {:?}", message_type);
                break;
            }
        }

        info!("Stream processor for {:?} shutting down cleanly", message_type);
        Ok(())
    }

    /// Try to reserve messages from the stream with proper shutdown handling
    async fn try_reserve_messages(
        &self,
        stream_key: &str,
        group_name: &str,
        message_type: MessageType,
    ) -> std::result::Result<
        Option<Vec<crate::redis::stream::StreamEntry>>,
        crate::redis::error::Error,
    > {
        // Check shutdown before starting the operation
        if self.should_shutdown().await {
            info!("Shutdown signal detected before stream reservation for {:?}", message_type);
            return Ok(None);
        }

        // Try to reserve messages from the stream
        match self.stream.reserve(stream_key, group_name, self.batch_size as usize, None).await {
            Ok(entries) => {
                if !entries.is_empty() {
                    return Ok(Some(entries));
                }

                // No entries reserved, try to claim stale messages
                if self.should_shutdown().await {
                    info!(
                        "Shutdown signal detected before claiming stale messages for {:?}",
                        message_type
                    );
                    return Ok(None);
                }

                match self
                    .stream
                    .claim_stale(
                        stream_key,
                        group_name,
                        self.timeout,
                        self.batch_size as usize,
                        None,
                    )
                    .await
                {
                    Ok(count) if !count.is_empty() => {
                        trace!("Processed {} stale events for {}", count.len(), stream_key);
                    },
                    Ok(_) => {
                        // No stale messages either, wait briefly before next poll
                        if self.should_shutdown().await {
                            info!(
                                "Shutdown signal detected before idle wait for {:?}",
                                message_type
                            );
                            return Ok(None);
                        }

                        // Use a more responsive wait with shutdown checking
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
                            _ = self.wait_for_shutdown() => {
                                info!("Shutdown signal detected during idle wait for {:?}", message_type);
                                return Ok(None);
                            }
                        }
                    },
                    Err(e) => {
                        error!("Error claiming stale messages from {}: {}", stream_key, e);
                    },
                }

                // Continue with next iteration of the main loop
                Ok(None)
            },
            Err(e) => {
                // Check if this is a connection pool timeout
                let is_pool_timeout = e.to_string().contains("Connection timeout")
                    || e.to_string().contains("Pool error");

                if is_pool_timeout {
                    warn!("Redis pool timeout for {} stream, will retry with backoff", stream_key);
                    // Use exponential backoff for pool timeouts
                    let retry_delay = Duration::from_millis(500);
                    tokio::select! {
                        _ = tokio::time::sleep(retry_delay) => {},
                        _ = self.wait_for_shutdown() => {
                            info!("Shutdown signal detected during pool timeout retry for {:?}", message_type);
                            return Ok(None);
                        }
                    }
                } else {
                    error!("Error reserving messages from {}: {}", stream_key, e);

                    // Check shutdown before sleeping
                    if self.should_shutdown().await {
                        info!(
                            "Shutdown signal detected after reserve error for {:?}",
                            message_type
                        );
                        return Ok(None);
                    }

                    // Wait a brief moment before retrying
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {},
                        _ = self.wait_for_shutdown() => {
                            info!("Shutdown signal detected during error wait for {:?}", message_type);
                            return Ok(None);
                        }
                    }
                }

                Ok(None)
            },
        }
    }

    /// Process a batch of messages and return IDs of successfully processed ones
    async fn process_message_batch(
        &self,
        entries: Vec<crate::redis::stream::StreamEntry>,
        message_type: MessageType,
    ) -> Vec<String> {
        let mut successful_ids = Vec::with_capacity(entries.len());

        for entry in entries {
            // Check shutdown periodically during processing
            if self.should_shutdown().await {
                info!("Shutdown signal detected during message processing for {:?}", message_type);
                break;
            }

            match HubEvent::decode(entry.data.as_slice()) {
                Ok(event) => {
                    trace!("Decoded event {} (type={})", entry.id, event.r#type);
                    crate::metrics::increment_events_received();

                    let start_time = Instant::now();
                    match self.processors.process_event(event).await {
                        Ok(_) => {
                            trace!("Processed event {}", entry.id);
                            crate::metrics::increment_events_processed();
                            crate::metrics::record_event_processing_time(start_time.elapsed());
                            successful_ids.push(entry.id)
                        },
                        Err(e) => {
                            error!("Error processing event {}: {}", entry.id, e);
                            crate::metrics::record_event_processing_time(start_time.elapsed());
                            successful_ids.push(entry.id); // Ack anyway to avoid reprocessing
                        },
                    }
                },
                Err(e) => {
                    error!("Error decoding event {}: {}", entry.id, e);
                    successful_ids.push(entry.id); // Ack anyway to avoid reprocessing
                },
            }
        }

        successful_ids
    }

    /// Helper method to check if the consumer should shut down
    async fn should_shutdown(&self) -> bool {
        let state = self.state.read().await;
        state.shutdown
    }

    /// Clean up old events from the stream
    async fn cleanup_old_events(&self, message_type: MessageType) {
        // Use the same key format that publisher uses
        let clean_host = self.hub_host.split(':').next().unwrap_or(&self.hub_host);
        let stream_key = crate::types::get_stream_key(clean_host, message_type.to_stream_key());

        trace!("Starting cleanup task for {:?}", message_type);

        // Create an interval that will fire every minute
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Initial tick happens immediately, so consume it first
        interval.tick().await;

        while !self.should_shutdown().await {
            // Wait for the next interval tick with shutdown checking
            tokio::select! {
                _ = interval.tick() => {},
                _ = self.wait_for_shutdown() => {
                    info!("Shutdown signal detected during cleanup interval wait for {:?}", message_type);
                    break;
                }
            }

            // Check shutdown flag before trimming
            if self.should_shutdown().await {
                info!("Shutdown signal detected before trim operation for {:?}", message_type);
                break;
            }

            // Perform trim operation
            // Keep events from the last 24 hours
            match self.stream.trim(&stream_key, Duration::from_secs(24 * 60 * 60)).await {
                Ok(_) => {
                    // Successfully trimmed old events (or no events to trim)
                },
                Err(e) => {
                    error!("Error trimming old events from {}: {}", stream_key, e);

                    // Check shutdown before sleeping on error
                    if self.should_shutdown().await {
                        info!("Shutdown signal detected after trim error for {:?}", message_type);
                        break;
                    }

                    // Brief sleep to avoid rapid retries on persistent errors
                    tokio::time::sleep(Duration::from_millis(100)).await;
                },
            }
        }

        info!("Cleanup task for {:?} shutting down cleanly", message_type);
    }

    /// Helper function that resolves when shutdown is requested
    async fn wait_for_shutdown(&self) -> bool {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            if self.should_shutdown().await {
                return true;
            }
        }
    }
}

/// Streaming service for processing Hub events
#[derive(Default)]
pub struct StreamingService {
    /// Service options
    options: StreamingOptions,
    /// Enabled spam filter
    enable_spam_filter: bool,
    /// Enabled print processor
    enable_print_processor: bool,
}

/// Processor type for selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorType {
    /// Database processor
    Database,
    /// Print processor (for debugging)
    Print,
    /// Spam filter
    SpamFilter,
}

/// Options for the streaming service
#[derive(Default)]
pub struct StreamingOptions {
    /// Subscriber options
    pub subscriber: Option<SubscriberOptions>,
    /// Consumer batch size
    pub batch_size: Option<u64>,
    /// Consumer concurrency
    pub concurrency: Option<usize>,
    /// Consumer timeout
    pub timeout: Option<Duration>,
    /// Event retention
    pub retention: Option<Duration>,
    /// Processors to enable (empty means no processors)
    pub processors: Vec<ProcessorType>,
}

impl StreamingOptions {
    /// Create a new default options instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Set subscriber options
    pub fn with_subscriber(mut self, subscriber: SubscriberOptions) -> Self {
        self.subscriber = Some(subscriber);
        self
    }

    /// Set batch size
    pub fn with_batch_size(mut self, batch_size: u64) -> Self {
        self.batch_size = Some(batch_size);
        self
    }

    /// Set concurrency
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set retention
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = Some(retention);
        self
    }

    /// Add a processor type
    pub fn with_processor(mut self, processor: ProcessorType) -> Self {
        self.processors.push(processor);
        self
    }

    /// Add multiple processor types
    pub fn with_processors(mut self, processors: Vec<ProcessorType>) -> Self {
        self.processors.extend(processors);
        self
    }
}

impl StreamingService {
    /// Create a new streaming service
    pub fn new() -> Self {
        Self {
            enable_spam_filter: true, // Enable spam filter by default
            ..Default::default()
        }
    }

    /// Set options for the service
    pub fn with_options(mut self, options: StreamingOptions) -> Self {
        self.options = options;
        self
    }

    /// Configure the service with a builder pattern
    pub fn configure<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StreamingOptions) -> StreamingOptions,
    {
        self.options = f(StreamingOptions::new());
        self
    }

    /// Enable or disable the spam filter
    pub fn with_spam_filter(mut self, enabled: bool) -> Self {
        self.enable_spam_filter = enabled;
        self
    }

    /// Enable or disable the print processor
    pub fn with_print_processor(mut self, enabled: bool) -> Self {
        self.enable_print_processor = enabled;
        self
    }
}

#[async_trait]
impl Service for StreamingService {
    fn name(&self) -> &str {
        "streaming"
    }

    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> Result<ServiceHandle> {
        // Import necessary modules
        use crate::core::MessageType;
        use crate::processor::{AppResources, database::DatabaseProcessor, print::PrintProcessor};

        // Create a single set of app resources with config - this will be wrapped in Arc only once
        // when needed by processor implementations
        let app_resources = AppResources::with_config(
            Arc::clone(&context.state.hub),
            Arc::clone(&context.state.redis),
            Arc::clone(&context.state.database),
            context.config.clone(),
        );

        // Create processor registry
        let mut processor_registry = ProcessorRegistry::new(Arc::clone(&context.state));

        // Always register the database processor since it's core functionality
        {
            info!("Registering database processor");

            // Create a wrapper that implements the app's EventProcessor trait
            struct DatabaseWrapper {
                processor: DatabaseProcessor,
            }

            impl DatabaseWrapper {
                fn new(resources: &AppResources) -> Self {
                    // Only wrap in Arc at the point of creation - no unnecessary Arcs
                    let resources_arc = Arc::new(resources.clone());
                    Self { processor: DatabaseProcessor::new(resources_arc) }
                }
            }

            #[async_trait::async_trait]
            impl crate::app::EventProcessor for DatabaseWrapper {
                fn name(&self) -> &str {
                    "database"
                }

                async fn process(
                    &self,
                    event: crate::proto::HubEvent,
                ) -> crate::app::ProcessorResult<()> {
                    use crate::processor::consumer::EventProcessor;
                    self.processor
                        .process_event(event)
                        .await
                        .map_err(|e| crate::app::ProcessorError::Processing(e.to_string()))
                }

                fn supported_types(&self) -> Vec<MessageType> {
                    MessageType::all().collect()
                }
            }

            let wrapper = DatabaseWrapper::new(&app_resources);
            processor_registry.register(wrapper);
        }

        // Register print processor if enabled
        if self.enable_print_processor {
            info!("Registering print processor");

            // Create a wrapper that implements the app's EventProcessor trait
            struct PrintWrapper {
                processor: PrintProcessor,
            }

            impl PrintWrapper {
                fn new(resources: &AppResources) -> Self {
                    // Only wrap in Arc at the point of creation
                    let resources_arc = Arc::new(resources.clone());
                    Self { processor: PrintProcessor::new(resources_arc) }
                }
            }

            #[async_trait::async_trait]
            impl crate::app::EventProcessor for PrintWrapper {
                fn name(&self) -> &str {
                    "print"
                }

                async fn process(
                    &self,
                    event: crate::proto::HubEvent,
                ) -> crate::app::ProcessorResult<()> {
                    // Use the processor trait directly
                    use crate::processor::consumer::EventProcessor;
                    self.processor
                        .process_event(event)
                        .await
                        .map_err(|e| crate::app::ProcessorError::Processing(e.to_string()))
                }

                fn supported_types(&self) -> Vec<MessageType> {
                    // Support all message types
                    MessageType::all().collect()
                }
            }

            let wrapper = PrintWrapper::new(&app_resources);
            processor_registry.register(wrapper);
        }

        // Register any other processors from config for backward compatibility
        for processor_type in &self.options.processors {
            match processor_type {
                ProcessorType::Database => {},   // Already registered
                ProcessorType::Print => {},      // Handled by enable_print_processor
                ProcessorType::SpamFilter => {}, // Handled by enable_spam_filter
            }
        }

        // Set up subscriber - minimize cloning by reusing resources
        let hub_host = {
            let hub = context.state.hub.lock().await;
            hub.host().to_string()
        };

        // Create a single Redis stream instance to share
        // We need to wrap in Arc because it will be shared across threads
        let redis_stream = Arc::new(RedisStream::new(Arc::clone(&context.state.redis)));

        // First, get hub info to understand available shards
        let hub_info = {
            let mut hub_guard = context.state.hub.lock().await;
            hub_guard.get_hub_info().await.map_err(|e| {
                AppError::Service(ServiceError::Initialization(format!(
                    "Failed to get hub info: {}",
                    e
                )))
            })?
        };

        // Use the actual number of shards from shard_infos, not num_shards
        // num_shards appears to count only data shards (excluding metadata shard 0)
        let available_shards = if !hub_info.shard_infos.is_empty() {
            hub_info.shard_infos.len() as u32
        } else {
            // Fallback to num_shards + 1 if shard_infos is empty
            hub_info.num_shards + 1
        };

        info!(
            "Hub reports num_shards: {}, but has {} total shards (including metadata shard 0)",
            hub_info.num_shards, available_shards
        );

        // Debug configuration values
        info!(
            "Shard configuration - shard_indices: {:?}, subscribe_to_all_shards: {}",
            context.config.hub.shard_indices, context.config.hub.subscribe_to_all_shards
        );

        // Determine which shards to subscribe to
        let shard_indices = if !context.config.hub.shard_indices.is_empty() {
            // Use explicitly configured shards, but validate them
            info!("Using configured shard indices: {:?}", context.config.hub.shard_indices);

            // Validate that all configured shards exist
            for &shard_idx in &context.config.hub.shard_indices {
                if shard_idx >= available_shards {
                    return Err(AppError::Service(ServiceError::Initialization(format!(
                        "Configured shard {} does not exist (hub has {} shards, valid indices are 0-{})",
                        shard_idx,
                        available_shards,
                        available_shards - 1
                    ))));
                }

                if shard_idx == 0 {
                    warn!(
                        "Shard 0 is configured but is typically used only for metadata. Consider removing it from shard_indices unless you specifically need metadata events."
                    );
                }
            }

            context.config.hub.shard_indices.clone()
        } else if context.config.hub.subscribe_to_all_shards {
            // Use all available shards except shard 0 (metadata shard)
            info!(
                "subscribe_to_all_shards is set - subscribing to shards 1-{} (skipping metadata shard 0)",
                available_shards - 1
            );

            // Create list of shard indices from 1 to num_shards-1 (skip shard 0)
            let shards: Vec<u32> = (1..available_shards).collect();

            info!(
                "Available shards: {}, subscribing to shards: {:?} (range 1..{})",
                available_shards, shards, available_shards
            );

            if shards.is_empty() && available_shards == 1 {
                return Err(AppError::Service(ServiceError::Initialization(
                    "Hub only has shard 0 (metadata shard) available. No event shards to subscribe to.".into()
                )));
            }

            // Log detailed shard information if available
            if !hub_info.shard_infos.is_empty() {
                for shard_info in &hub_info.shard_infos {
                    info!(
                        "Shard {}: {} messages, max_height: {}, mempool_size: {}",
                        shard_info.shard_id,
                        shard_info.num_messages,
                        shard_info.max_height,
                        shard_info.mempool_size
                    );
                }
            }

            shards
        } else {
            return Err(AppError::Service(ServiceError::Initialization(
                "No shard indices configured. Set hub.shard_indices or hub.subscribe_to_all_shards"
                    .into(),
            )));
        };

        info!("Starting subscriptions for shards: {:?}", shard_indices);

        // Create multiple subscribers, one per shard
        let mut subscriber_handles = Vec::new();
        let mut subscriber_arcs = Vec::new();

        for shard_index in shard_indices {
            let shard_key = format!("shard_{}", shard_index);

            let subscriber = {
                let mut hub_guard = context.state.hub.lock().await;
                let client = hub_guard.client().ok_or_else(|| {
                    ServiceError::Initialization("No hub client available".to_string())
                })?;

                let mut options = self.options.subscriber.clone().unwrap_or_default();

                // Configure the subscriber to use spam filter if enabled
                if !self.enable_spam_filter {
                    options.spam_filter_enabled = Some(false);
                } else {
                    options.spam_filter_enabled = Some(true);
                }

                options.hub_config = Some(Arc::new(context.config.hub.clone()));
                options.shard_index = Some(shard_index as u64);

                HubSubscriber::new(
                    client.clone(),
                    Arc::clone(&context.state.redis),
                    RedisStream::new(Arc::clone(&context.state.redis)),
                    hub_guard.host().to_string(),
                    shard_key.clone(),
                    options,
                )
                .await
            };

            let subscriber_arc = Arc::new(Mutex::new(subscriber));
            subscriber_arcs.push(Arc::clone(&subscriber_arc));

            let subscriber_clone = Arc::clone(&subscriber_arc);
            let handle = tokio::spawn(async move {
                let subscriber = subscriber_clone.lock().await;
                info!("Starting subscriber for shard {}", shard_index);
                if let Err(e) = subscriber.start().await {
                    error!("Subscriber error for shard {}: {}", shard_index, e);
                }
            });

            subscriber_handles.push(handle);
        }

        // Create a single consumer that processes events from all shards
        // The consumer reads from Redis streams that are populated by all subscribers
        let consumer = Consumer::new(
            redis_stream,
            Arc::new(processor_registry),
            hub_host,
            "all".to_string(), // Consumer processes events from all shards
        );

        // Apply options from config
        let consumer =
            consumer.with_batch_size(self.options.batch_size.unwrap_or(DEFAULT_BATCH_SIZE));
        let consumer =
            consumer.with_concurrency(self.options.concurrency.unwrap_or(DEFAULT_CONCURRENCY));
        let consumer = consumer.with_timeout(self.options.timeout.unwrap_or(DEFAULT_TIMEOUT));
        let consumer =
            consumer.with_retention(self.options.retention.unwrap_or(DEFAULT_EVENT_RETENTION));

        // Start consumer
        let consumer_handle = consumer.start().await;

        // Create stop channel
        let (stop_tx, stop_rx) = oneshot::channel();

        // Create join handle with more graceful shutdown sequence
        let join_handle = tokio::spawn(async move {
            // Wait for stop signal
            let _ = stop_rx.await;

            info!("Stopping streaming service (subscribers first, then consumer)...");

            // First stop all subscribers to prevent new events from entering the system
            for (i, subscriber_arc) in subscriber_arcs.iter().enumerate() {
                let subscriber = subscriber_arc.lock().await;
                if let Err(e) = subscriber.stop().await {
                    error!("Error stopping subscriber {}: {}", i, e);
                } else {
                    info!("Subscriber {} shutdown signal sent successfully", i);
                }
            }

            // Give the subscribers a moment to complete any in-flight operations
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Then stop the consumer (which will handle remaining events already in Redis)
            info!("Aborting consumer tasks...");
            consumer_handle.abort();

            // Finally stop all subscriber background tasks
            info!("Aborting {} subscriber tasks...", subscriber_handles.len());
            for handle in subscriber_handles {
                handle.abort();
            }

            info!("Streaming service stopped completely");
        });

        Ok(ServiceHandle::new(stop_tx, join_handle))
    }
}
