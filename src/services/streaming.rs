//! Streaming service for processing real-time Hub events
use crate::{
    app::{ProcessorRegistry, Result, Service, ServiceContext, ServiceError, ServiceHandle},
    core::MessageType,
    hub::subscriber::{HubSubscriber, SubscriberOptions},
    proto::HubEvent,
    redis::{error::Error as RedisError, stream::RedisStream},
};
use async_trait::async_trait;
use prost::Message as ProstMessage;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, RwLock, oneshot},
    task::JoinHandle,
};
use tracing::{error, info, trace};

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
        let total_tasks = message_types_count * 2; // *2 for both processor and cleanup tasks
        let (startup_tx, mut startup_rx) = tokio::sync::mpsc::channel(total_tasks);

        for message_type in MessageType::all() {
            let consumer_clone = Arc::clone(&consumer);
            let startup_tx_clone = startup_tx.clone();

            let handle = tokio::spawn(async move {
                // Signal that initialization is about to start
                let _ = startup_tx_clone.send(()).await;

                // Wait briefly to ensure we don't cause contention during initialization
                tokio::time::sleep(Duration::from_millis(10)).await;

                if let Err(e) = consumer_clone.process_stream(message_type).await {
                    error!("Stream processor error for {:?}: {}", message_type, e);
                }
                info!("Stream processor for {:?} shut down", message_type);
            });
            handles.push(handle);
        }

        // Start cleanup task for old events and track their handles
        let mut cleanup_handles = Vec::new();
        for message_type in MessageType::all() {
            let consumer_clone = Arc::clone(&consumer);
            let startup_tx_clone = startup_tx.clone();

            let cleanup_handle = tokio::spawn(async move {
                // Signal that initialization is about to start
                let _ = startup_tx_clone.send(()).await;

                // Wait briefly to ensure we don't cause contention during initialization
                tokio::time::sleep(Duration::from_millis(10)).await;

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

    /// Process events from a specific stream
    async fn process_stream(
        &self,
        message_type: MessageType,
    ) -> std::result::Result<(), RedisError> {
        // Use the same key format that publisher uses
        let clean_host = self.hub_host.split(':').next().unwrap_or(&self.hub_host);
        let stream_key =
            crate::types::get_stream_key(clean_host, message_type.to_stream_key(), Some("evt"));
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

                if let Err(e) = self.stream.ack(&stream_key, &group_name, successful_ids).await {
                    error!("Error acknowledging messages from {}: {}", stream_key, e);
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
    ) -> std::result::Result<Option<Vec<crate::redis::stream::StreamEntry>>, RedisError> {
        // Check shutdown before starting the operation
        if self.should_shutdown().await {
            info!("Shutdown signal detected before stream reservation for {:?}", message_type);
            return Ok(None);
        }

        // Try to reserve messages from the stream
        match self.stream.reserve(stream_key, group_name, self.batch_size, None).await {
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
                    .claim_stale(stream_key, group_name, self.timeout, self.batch_size, None)
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
                error!("Error reserving messages from {}: {}", stream_key, e);

                // Check shutdown before sleeping
                if self.should_shutdown().await {
                    info!("Shutdown signal detected after reserve error for {:?}", message_type);
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
                    match self.processors.process_event(event).await {
                        Ok(_) => successful_ids.push(entry.id),
                        Err(e) => {
                            error!("Error processing event {}: {}", entry.id, e);
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
        let stream_key =
            crate::types::get_stream_key(clean_host, message_type.to_stream_key(), Some("evt"));

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
            match self.stream.trim(&stream_key, self.retention).await {
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

        let subscriber = {
            let mut hub_guard = context.state.hub.lock().await;
            let client = hub_guard.client().ok_or_else(|| {
                ServiceError::Initialization("No hub client available".to_string())
            })?;

            let mut options = self.options.subscriber.clone().unwrap_or_default();

            // Configure the subscriber to use spam filter if enabled
            if !self.enable_spam_filter {
                info!("Spam filter disabled - all messages will be processed");
                options.spam_filter_enabled = Some(false);
            } else {
                info!("Spam filter enabled - spam messages will be filtered");
                options.spam_filter_enabled = Some(true);
            }

            HubSubscriber::new(
                client.clone(),
                Arc::clone(&context.state.redis),
                RedisStream::new(Arc::clone(&context.state.redis)), // Can't use Arc<RedisStream> here
                hub_guard.host().to_string(),
                "default".to_string(),
                options,
            )
            .await
        };

        // Create consumer - reuse the same Redis stream
        let consumer = Consumer::new(
            redis_stream, // Reuse the stream - no need to create a new one
            Arc::new(processor_registry),
            hub_host,
            "default".to_string(),
        );

        // Apply options from config
        let consumer =
            consumer.with_batch_size(self.options.batch_size.unwrap_or(DEFAULT_BATCH_SIZE));
        let consumer =
            consumer.with_concurrency(self.options.concurrency.unwrap_or(DEFAULT_CONCURRENCY));
        let consumer = consumer.with_timeout(self.options.timeout.unwrap_or(DEFAULT_TIMEOUT));
        let consumer =
            consumer.with_retention(self.options.retention.unwrap_or(DEFAULT_EVENT_RETENTION));

        // Start consumer and subscriber
        let consumer_handle = consumer.start().await;
        let subscriber_arc = Arc::new(Mutex::new(subscriber));

        let subscriber_clone = Arc::clone(&subscriber_arc);
        let subscriber_handle = tokio::spawn(async move {
            let subscriber = subscriber_clone.lock().await;
            if let Err(e) = subscriber.start().await {
                error!("Subscriber error: {}", e);
            }
        });

        // Create stop channel
        let (stop_tx, stop_rx) = oneshot::channel();

        // Create join handle with more graceful shutdown sequence
        let join_handle = tokio::spawn(async move {
            // Wait for stop signal
            let _ = stop_rx.await;

            info!("Stopping streaming service (subscriber first, then consumer)...");

            // First stop the subscriber to prevent new events from entering the system
            {
                let subscriber = subscriber_arc.lock().await;
                if let Err(e) = subscriber.stop().await {
                    error!("Error stopping subscriber: {}", e);
                } else {
                    info!("Subscriber shutdown signal sent successfully");
                }
            }

            // Give the subscriber a moment to complete any in-flight operations
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Then stop the consumer (which will handle remaining events already in Redis)
            info!("Aborting consumer tasks...");
            consumer_handle.abort();

            // Finally stop the subscriber background task
            info!("Aborting subscriber task...");
            subscriber_handle.abort();

            info!("Streaming service stopped completely");
        });

        Ok(ServiceHandle::new(stop_tx, join_handle))
    }
}
