use crate::{
    processor::consumer::EventProcessor,
    proto::HubEvent,
    redis::stream::{RedisStream, StreamEntry},
};
use futures::future::join_all;
use prost::Message;
use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, time};
use tracing::{error, info};

pub struct StreamProcessor {
    pub stream: Arc<RedisStream>,
    pub(crate) stream_key: String,
    pub(crate) group_name: String,
    pub(crate) processors: Vec<Arc<dyn EventProcessor>>,
    pub(crate) shutdown: Arc<RwLock<bool>>,
    pub(crate) max_events_per_fetch: u64,
    pub(crate) processing_concurrency: usize,
    pub(crate) event_processing_timeout: Duration,
}

impl StreamProcessor {
    pub async fn process_stream(&self) -> Result<(), crate::redis::error::Error> {
        while !*self.shutdown.read().await {
            if let Err(e) = self.stream.create_group(&self.stream_key, &self.group_name).await {
                error!("Error creating group for {}: {}", self.stream_key, e);
                continue;
            }

            match self.process_batch().await {
                Ok(true) => continue,
                Ok(false) => {
                    if let Ok(count) = self.process_stale_events().await {
                        if count > 0 {
                            info!("Processed {} stale events for {}", count, self.stream_key);
                            continue;
                        }
                    }
                },
                Err(e) => error!("Processing error for {}: {}", self.stream_key, e),
            }

            time::sleep(Duration::from_millis(10)).await;
        }

        info!("Stream processor terminated for {}", self.stream_key);
        Ok(())
    }

    async fn process_batch(&self) -> Result<bool, crate::redis::error::Error> {
        let entries = match self
            .stream
            .reserve(&self.stream_key, &self.group_name, self.max_events_per_fetch)
            .await
        {
            Ok(entries) => entries,
            Err(e) => return Err(e),
        };

        if entries.is_empty() {
            return Ok(false);
        }

        let mut tasks = Vec::with_capacity(entries.len());
        let mut batch = Vec::new();

        for entry in entries {
            batch.push(entry);
            if batch.len() >= self.processing_concurrency {
                let batch_tasks = self.process_batch_concurrent(batch).await?;
                tasks.extend(batch_tasks);
                batch = Vec::new();
            }
        }

        if !batch.is_empty() {
            let batch_tasks = self.process_batch_concurrent(batch).await?;
            tasks.extend(batch_tasks);
        }

        let process_start_time = std::time::Instant::now();
        let results = join_all(tasks).await;
        let processing_time = process_start_time.elapsed().as_millis() as u64;

        // Update performance metrics
        self.stream.update_success_metrics(processing_time).await;

        let successful_ids: Vec<String> = results.into_iter().filter_map(|r| r.ok()).collect();

        if !successful_ids.is_empty() {
            match self.stream.ack(&self.stream_key, &self.group_name, successful_ids).await {
                Ok(_) => Ok(true),
                Err(e) => {
                    if e.to_string().contains("NOPROTO")
                        || e.to_string().contains("ERR")
                        || e.to_string().contains("NOGROUP")
                        || e.to_string().contains("BUSYGROUP")
                    {
                        Ok(true)
                    } else {
                        // Update error metrics
                        self.stream.update_error_metrics().await;
                        Err(e)
                    }
                },
            }
        } else {
            Ok(true)
        }
    }

    async fn process_batch_concurrent(
        &self,
        batch: Vec<StreamEntry>,
    ) -> Result<Vec<tokio::task::JoinHandle<String>>, crate::redis::error::Error> {
        use tokio::sync::Semaphore;

        // Create semaphore to limit concurrent tasks
        let semaphore = Arc::new(Semaphore::new(self.processing_concurrency));
        let mut tasks = Vec::with_capacity(batch.len());

        // Pre-decode all events in batch to avoid duplicate work
        let entries: Vec<_> = batch
            .into_iter()
            .map(|entry| {
                let event = HubEvent::decode(entry.data.as_slice()).ok();
                (entry.id, entry.data, event)
            })
            .collect();

        for (id, _data, maybe_event) in entries {
            let processors = self.processors.clone();
            let semaphore_clone = Arc::clone(&semaphore);

            let task = tokio::spawn(async move {
                // Acquire semaphore permit to limit concurrency
                let _permit = semaphore_clone.acquire().await.unwrap();

                match maybe_event {
                    Some(event) => {
                        // Process all processors in parallel without special handling
                        let event_clone = event.clone();
                        let results = futures::future::join_all(
                            processors
                                .iter()
                                .map(|processor| processor.process_event(event_clone.clone())),
                        )
                        .await;

                        // Check for any errors
                        for (i, result) in results.into_iter().enumerate() {
                            if let Err(e) = result {
                                error!("Processor[{}] error: {}", i, e);
                                return id;
                            }
                        }

                        id
                    },
                    None => {
                        error!("Decode error for message {}", id);
                        id
                    },
                }
            });

            tasks.push(task);
        }

        Ok(tasks)
    }

    async fn process_stale_events(&self) -> Result<u64, crate::redis::error::Error> {
        let stale_entries = self
            .stream
            .claim_stale(
                &self.stream_key,
                &self.group_name,
                self.event_processing_timeout,
                self.max_events_per_fetch,
            )
            .await?;

        if stale_entries.is_empty() {
            return Ok(0);
        }

        let total_entries = stale_entries.len();
        let mut tasks = Vec::new();
        let mut batch = Vec::new();

        for entry in stale_entries {
            batch.push(entry);
            if batch.len() >= self.processing_concurrency {
                let current_batch = std::mem::take(&mut batch);
                let batch_tasks = self.process_batch_concurrent(current_batch).await?;
                tasks.extend(batch_tasks);
            }
        }

        if !batch.is_empty() {
            let batch_tasks = self.process_batch_concurrent(batch).await?;
            tasks.extend(batch_tasks);
        }

        let process_start_time = std::time::Instant::now();
        let results = join_all(tasks).await;
        let processing_time = process_start_time.elapsed().as_millis() as u64;

        // Update metrics for stale events
        self.stream.update_success_metrics(processing_time).await;
        self.stream.update_retry_metrics().await;

        let successful_ids: Vec<String> = results.into_iter().filter_map(|r| r.ok()).collect();

        if !successful_ids.is_empty() {
            match self.stream.ack(&self.stream_key, &self.group_name, successful_ids).await {
                Ok(_) => {},
                Err(e) => {
                    self.stream.update_error_metrics().await;
                    return Err(e);
                },
            }
        }

        Ok(total_entries as u64)
    }
}
