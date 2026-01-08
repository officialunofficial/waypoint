//! Consumer service for Redis → PostgreSQL streaming
//!
//! This service reads events from Redis streams and writes them to PostgreSQL.
//! It can be run independently of the producer for horizontal scaling.

use crate::{
    app::{
        AppError, ProcessorRegistry, Result, Service, ServiceContext, ServiceError, ServiceHandle,
    },
    core::MessageType,
    redis::stream::RedisStream,
    services::streaming::Consumer,
};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tracing::info;

/// Consumer service that reads from Redis and writes to PostgreSQL
///
/// This service is designed for consumer-only deployments where you want to
/// scale the database writes independently from Hub subscription.
#[derive(Default)]
pub struct ConsumerService {
    /// Batch size for reading from Redis
    batch_size: Option<u64>,
    /// Concurrency for processing messages
    concurrency: Option<usize>,
    /// Processing timeout
    timeout: Option<Duration>,
    /// Event retention duration
    retention: Option<Duration>,
    /// Enable print processor for debugging
    enable_print_processor: bool,
}

impl ConsumerService {
    /// Create a new consumer service with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set batch size for reading from Redis
    pub fn with_batch_size(mut self, size: u64) -> Self {
        self.batch_size = Some(size);
        self
    }

    /// Set concurrency for processing messages
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Set processing timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set event retention duration
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = Some(retention);
        self
    }

    /// Enable or disable the print processor for debugging
    pub fn with_print_processor(mut self, enabled: bool) -> Self {
        self.enable_print_processor = enabled;
        self
    }
}

#[async_trait]
impl Service for ConsumerService {
    fn name(&self) -> &str {
        "consumer"
    }

    async fn start<'a>(&'a self, context: ServiceContext<'a>) -> Result<ServiceHandle> {
        use crate::processor::{AppResources, database::DatabaseProcessor, print::PrintProcessor};

        // Ensure Database is available (required for consumer mode)
        let database = context.state.database.as_ref().ok_or_else(|| {
            AppError::Service(ServiceError::Initialization(
                "Database client not available. Consumer mode requires database configuration."
                    .to_string(),
            ))
        })?;

        // Create app resources for processors
        // In consumer-only mode, hub is not available
        let app_resources = AppResources::with_config_consumer_only(
            Arc::clone(&context.state.redis),
            Arc::clone(database),
            context.config.clone(),
        );

        // Create processor registry
        let mut processor_registry = ProcessorRegistry::new(Arc::clone(&context.state));

        // Register database processor
        {
            info!("Registering database processor for consumer service");

            struct DatabaseWrapper {
                processor: DatabaseProcessor,
            }

            impl DatabaseWrapper {
                fn new(resources: &AppResources) -> Self {
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
            info!("Registering print processor for consumer service");

            struct PrintWrapper {
                processor: PrintProcessor,
            }

            impl PrintWrapper {
                fn new(resources: &AppResources) -> Self {
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

            let wrapper = PrintWrapper::new(&app_resources);
            processor_registry.register(wrapper);
        }

        // Create Redis stream
        let redis_stream = Arc::new(
            RedisStream::new(Arc::clone(&context.state.redis)).with_config(&context.config.stream),
        );

        // For consumer-only mode, we use the hub URL from config as the stream key prefix
        // This assumes the producer wrote to streams using the same hub URL
        let hub_host = context.config.hub.url.clone();

        info!("Consumer service using hub host key: {}", hub_host);

        // Create consumer
        let stream_config = context.config.stream.clone();
        let mut consumer = Consumer::new(
            redis_stream,
            Arc::new(processor_registry),
            hub_host,
            "all".to_string(),
            stream_config,
        );

        // Apply overrides
        if let Some(batch_size) = self.batch_size {
            consumer = consumer.with_batch_size(batch_size);
        }
        if let Some(concurrency) = self.concurrency {
            consumer = consumer.with_concurrency(concurrency);
        }
        if let Some(timeout) = self.timeout {
            consumer = consumer.with_timeout(timeout);
        }
        if let Some(retention) = self.retention {
            consumer = consumer.with_retention(retention);
        }

        // Start consumer
        let consumer_handle = consumer.start().await;

        // Create stop channel
        let (stop_tx, stop_rx) = oneshot::channel();

        // Create join handle for graceful shutdown
        let join_handle = tokio::spawn(async move {
            // Wait for stop signal
            let _ = stop_rx.await;

            info!("Stopping consumer service...");

            // Abort consumer tasks
            consumer_handle.abort();

            info!("Consumer service stopped");
        });

        Ok(ServiceHandle::new(stop_tx, join_handle))
    }
}
