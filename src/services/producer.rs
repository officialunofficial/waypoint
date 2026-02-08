//! Producer service for Hub → Redis streaming
//!
//! This service subscribes to the Snapchain Hub and writes events to Redis streams.
//! It can be run independently of the consumer for horizontal scaling.

use crate::{
    app::{AppError, Result, Service, ServiceContext, ServiceError, ServiceHandle},
    hub::subscriber::{HubSubscriber, SubscriberOptions},
    redis::stream::RedisStream,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{error, info, warn};

/// Producer service that reads from Hub and writes to Redis streams
///
/// This service is designed for producer-only deployments where you want to
/// scale the Hub subscription independently from database writes.
#[derive(Default)]
pub struct ProducerService {
    /// Enable spam filtering on ingested events
    enable_spam_filter: bool,
    /// Subscriber options
    subscriber_options: Option<SubscriberOptions>,
}

impl ProducerService {
    /// Create a new producer service with default settings
    pub fn new() -> Self {
        Self { enable_spam_filter: true, subscriber_options: None }
    }

    /// Enable or disable spam filtering
    pub fn with_spam_filter(mut self, enabled: bool) -> Self {
        self.enable_spam_filter = enabled;
        self
    }

    /// Set subscriber options
    pub fn with_subscriber_options(mut self, options: SubscriberOptions) -> Self {
        self.subscriber_options = Some(options);
        self
    }
}

#[async_trait]
impl Service for ProducerService {
    fn name(&self) -> &str {
        "producer"
    }

    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> Result<ServiceHandle> {
        // Ensure Hub is available (required for producer mode)
        let hub = context.state.hub.as_ref().ok_or_else(|| {
            AppError::Service(ServiceError::Initialization(
                "Hub client not available. Producer mode requires hub configuration.".to_string(),
            ))
        })?;

        // Get hub info to understand available shards
        let hub_info = {
            let hub_guard = hub.lock().await;
            hub_guard.get_hub_info().await.map_err(|e| {
                AppError::Service(ServiceError::Initialization(format!(
                    "Failed to get hub info: {}",
                    e
                )))
            })?
        };

        // Determine available shards
        let available_shards = if !hub_info.shard_infos.is_empty() {
            hub_info.shard_infos.len() as u32
        } else {
            hub_info.num_shards + 1
        };

        info!(
            "Hub reports num_shards: {}, total shards: {} (including metadata shard 0)",
            hub_info.num_shards, available_shards
        );

        // Determine which shards to subscribe to
        let shard_indices = if !context.config.hub.shard_indices.is_empty() {
            // Use explicitly configured shards
            info!("Using configured shard indices: {:?}", context.config.hub.shard_indices);

            // Validate configured shards
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
                    warn!("Shard 0 is configured but is typically used only for metadata.");
                }
            }

            context.config.hub.shard_indices.clone()
        } else if context.config.hub.subscribe_to_all_shards {
            // Subscribe to all shards except metadata shard 0
            info!(
                "subscribe_to_all_shards is set - subscribing to shards 1-{}",
                available_shards - 1
            );

            let shards: Vec<u32> = (1..available_shards).collect();

            if shards.is_empty() && available_shards == 1 {
                return Err(AppError::Service(ServiceError::Initialization(
                    "Hub only has shard 0 (metadata shard) available. No event shards to subscribe to.".into()
                )));
            }

            shards
        } else {
            return Err(AppError::Service(ServiceError::Initialization(
                "No shard indices configured. Set hub.shard_indices or hub.subscribe_to_all_shards"
                    .into(),
            )));
        };

        info!("Starting producer subscriptions for shards: {:?}", shard_indices);

        // Create subscribers for each shard
        let mut subscriber_handles = Vec::new();
        let mut subscriber_arcs = Vec::new();

        for shard_index in shard_indices {
            let shard_key = format!("shard_{}", shard_index);

            let subscriber = {
                let mut hub_guard = hub.lock().await;
                let client = hub_guard.client().ok_or_else(|| {
                    ServiceError::Initialization("No hub client available".to_string())
                })?;

                let mut options = self.subscriber_options.clone().unwrap_or_default();
                options.spam_filter_enabled = Some(self.enable_spam_filter);
                options.hub_config = Some(Arc::new(context.config.hub.clone()));
                options.shard_index = Some(shard_index as u64);

                HubSubscriber::new(
                    client.clone(),
                    Arc::clone(&context.state.redis),
                    RedisStream::new(Arc::clone(&context.state.redis))
                        .with_config(&context.config.stream),
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
                info!("Starting producer subscriber for shard {}", shard_index);
                if let Err(e) = subscriber.start().await {
                    error!("Producer subscriber error for shard {}: {}", shard_index, e);
                }
            });

            subscriber_handles.push(handle);
        }

        // Create stop channel
        let (stop_tx, stop_rx) = oneshot::channel();

        // Create join handle for graceful shutdown
        let join_handle = tokio::spawn(async move {
            // Wait for stop signal
            let _ = stop_rx.await;

            info!("Stopping producer service...");

            // Stop all subscribers
            for (i, subscriber_arc) in subscriber_arcs.iter().enumerate() {
                let subscriber = subscriber_arc.lock().await;
                if let Err(e) = subscriber.stop().await {
                    error!("Error stopping producer subscriber {}: {}", i, e);
                } else {
                    info!("Producer subscriber {} stopped", i);
                }
            }

            // Give subscribers a moment to complete in-flight operations
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Abort all subscriber tasks
            for handle in subscriber_handles {
                handle.abort();
            }

            info!("Producer service stopped");
        });

        Ok(ServiceHandle::new(stop_tx, join_handle))
    }
}
