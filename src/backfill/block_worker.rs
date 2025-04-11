use crate::{
    backfill::block_reconciler::BlockReconciler,
    processor::consumer::EventProcessor,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::RwLock,
    time,
};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    High,
    Normal,
    Low,
}

impl Default for JobPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl Default for JobState {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockBackfillJob {
    pub shard_id: u32,
    pub start_block: u64,
    pub end_block: u64,
    #[serde(default)]
    pub priority: JobPriority,
    #[serde(default)]
    pub state: JobState,
    #[serde(default)]
    pub visibility_timeout: Option<u64>, // Timeout in seconds
    #[serde(default)]
    pub attempts: u32,
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Default, Clone)]
pub struct BlockQueueMetrics {
    pub jobs_queued: u64,
    pub jobs_processed: u64,
    pub jobs_failed: u64,
    pub blocks_processed: u64,
    pub avg_job_time_ms: f64,
    pub high_priority_queue_size: u64,
    pub normal_priority_queue_size: u64,
    pub low_priority_queue_size: u64,
    pub in_progress_queue_size: u64,
}

pub struct BlockBackfillQueue {
    redis: Arc<crate::redis::client::Redis>,
    queue_key: String,
    high_priority_queue_key: String,
    low_priority_queue_key: String,
    in_progress_queue_key: String,
    metrics: Arc<tokio::sync::RwLock<BlockQueueMetrics>>,
}

impl BlockBackfillQueue {
    pub fn new(redis: Arc<crate::redis::client::Redis>, queue_key: String) -> Self {
        let base_key = if queue_key.ends_with(":queue") {
            queue_key.trim_end_matches(":queue").to_string()
        } else {
            queue_key.clone()
        };

        Self {
            redis,
            queue_key: format!("{}:normal", base_key),
            high_priority_queue_key: format!("{}:high", base_key),
            low_priority_queue_key: format!("{}:low", base_key),
            in_progress_queue_key: format!("{}:inprogress", base_key),
            metrics: Arc::new(tokio::sync::RwLock::new(BlockQueueMetrics::default())),
        }
    }

    /// Get current queue metrics
    pub async fn get_metrics(&self) -> BlockQueueMetrics {
        // First fetch the queue sizes without holding the lock
        let high = self.get_queue_length_for_key(&self.high_priority_queue_key).await.unwrap_or(0);
        let normal = self.get_queue_length_for_key(&self.queue_key).await.unwrap_or(0);
        let low = self.get_queue_length_for_key(&self.low_priority_queue_key).await.unwrap_or(0);
        let in_progress =
            self.get_queue_length_for_key(&self.in_progress_queue_key).await.unwrap_or(0);

        // Then briefly acquire the lock to read other metrics
        let metrics_guard = self.metrics.read().await;

        // Create a new metrics object with copied primitive values (no deep cloning)
        BlockQueueMetrics {
            jobs_queued: metrics_guard.jobs_queued,
            jobs_processed: metrics_guard.jobs_processed,
            jobs_failed: metrics_guard.jobs_failed,
            blocks_processed: metrics_guard.blocks_processed,
            avg_job_time_ms: metrics_guard.avg_job_time_ms,
            high_priority_queue_size: high as u64,
            normal_priority_queue_size: normal as u64,
            low_priority_queue_size: low as u64,
            in_progress_queue_size: in_progress as u64,
        }
    }

    async fn get_queue_length_for_key(
        &self,
        key: &str,
    ) -> Result<usize, crate::redis::error::Error> {
        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        let result: bb8_redis::redis::RedisResult<usize> =
            bb8_redis::redis::cmd("LLEN").arg(key).query_async(&mut *conn).await;

        match result {
            Ok(len) => {
                debug!("Queue {} has {} jobs remaining", key, len);
                Ok(len)
            },
            Err(e) => {
                error!("Error getting queue length for {}: {:?}", key, e);
                Err(crate::redis::error::Error::RedisError(e))
            },
        }
    }

    pub async fn add_job(&self, mut job: BlockBackfillJob) -> Result<(), crate::redis::error::Error> {
        // Set job ID if not provided
        if job.id.is_empty() {
            job.id = uuid::Uuid::new_v4().to_string();
        }

        // Select appropriate queue based on priority
        let queue_key = match job.priority {
            JobPriority::High => &self.high_priority_queue_key,
            JobPriority::Normal => &self.queue_key,
            JobPriority::Low => &self.low_priority_queue_key,
        };

        // Serialize job once to avoid cloning
        let job_data = serde_json::to_string(&job)
            .map_err(|e| crate::redis::error::Error::DeserializationError(e.to_string()))?;

        let block_count = job.end_block - job.start_block + 1;
        let job_id = job.id.clone();
        
        info!(
            "Adding backfill job {} with blocks {}-{} ({} blocks) for shard {} to queue: {} (priority: {:?})",
            job_id, job.start_block, job.end_block, block_count, job.shard_id, queue_key, job.priority
        );

        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        let result: bb8_redis::redis::RedisResult<()> = bb8_redis::redis::cmd("LPUSH")
            .arg(queue_key)
            .arg(job_data)
            .query_async(&mut *conn)
            .await;

        match &result {
            Ok(_) => {
                info!("Successfully added job {} to queue {}", job_id, queue_key);
                // Update metrics with minimal lock time
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.jobs_queued += 1;
                }
            },
            Err(e) => error!("Failed to add job {} to queue {}: {:?}", job_id, queue_key, e),
        }

        result.map_err(crate::redis::error::Error::RedisError)
    }

    pub async fn get_job(&self) -> Result<Option<BlockBackfillJob>, crate::redis::error::Error> {
        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        // Try queues in priority order: high, normal, low
        let queue_keys =
            [&self.high_priority_queue_key, &self.queue_key, &self.low_priority_queue_key];

        for queue_key in &queue_keys {
            let result: bb8_redis::redis::RedisResult<Option<Vec<String>>> = bb8_redis::redis::cmd("BRPOP")
                .arg(queue_key)
                .arg(1) // Short timeout to try next queue if empty
                .query_async(&mut *conn)
                .await;

            match result {
                Ok(Some(values)) if values.len() >= 2 => {
                    // BRPOP returns [key, value]
                    let job_data = &values[1];
                    let mut job: BlockBackfillJob = serde_json::from_str(job_data).map_err(|e| {
                        crate::redis::error::Error::DeserializationError(e.to_string())
                    })?;

                    // Update job state
                    job.state = JobState::InProgress;
                    job.attempts += 1;

                    // Set visibility timeout if not set
                    if job.visibility_timeout.is_none() {
                        job.visibility_timeout = Some(300); // Default 5 minute timeout
                    }

                    // Add to in-progress queue with TTL for visibility timeout
                    let in_progress_data = serde_json::to_string(&job).unwrap();
                    let _: bb8_redis::redis::RedisResult<()> = bb8_redis::redis::cmd("LPUSH")
                        .arg(&self.in_progress_queue_key)
                        .arg(&in_progress_data)
                        .query_async(&mut *conn)
                        .await;

                    if let Some(timeout) = job.visibility_timeout {
                        // Set expiration on the job - we'll get it back after timeout if not completed
                        let job_key = format!("block_backfill:job:{}", job.id);
                        let _: bb8_redis::redis::RedisResult<()> = bb8_redis::redis::cmd("SET")
                            .arg(&job_key)
                            .arg(&in_progress_data)
                            .arg("EX")
                            .arg(timeout)
                            .query_async(&mut *conn)
                            .await;
                    }

                    info!(
                        "Retrieved block backfill job {} with blocks {}-{} from queue {} (priority: {:?}, attempt: {})",
                        job.id,
                        job.start_block,
                        job.end_block,
                        queue_key,
                        job.priority,
                        job.attempts
                    );

                    return Ok(Some(job));
                },
                Ok(_) => {
                    debug!("No jobs available in queue {}", queue_key);
                    // Continue to next queue
                },
                Err(e) => {
                    error!("Error retrieving job from queue {}: {:?}", queue_key, e);
                    return Err(crate::redis::error::Error::RedisError(e));
                },
            }
        }

        // No jobs found in any queue
        Ok(None)
    }

    /// Mark a job as completed
    pub async fn complete_job(&self, job_id: &str, blocks_processed: u64) -> Result<(), crate::redis::error::Error> {
        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        // Remove job from in-progress list
        let _: bb8_redis::redis::RedisResult<()> = bb8_redis::redis::cmd("DEL")
            .arg(format!("block_backfill:job:{}", job_id))
            .query_async(&mut *conn)
            .await;

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.jobs_processed += 1;
        metrics.blocks_processed += blocks_processed;

        Ok(())
    }

    /// Move job back to queue after a failure
    pub async fn retry_job(&self, mut job: BlockBackfillJob) -> Result<(), crate::redis::error::Error> {
        // Reset state and increment attempts in place (no cloning)
        job.state = JobState::Pending;
        job.attempts += 1;

        // Reduce priority if job has been retried multiple times
        if job.attempts > 3 {
            job.priority = JobPriority::Low;
        }

        // Add job back to queue (reusing the job object)
        self.add_job(job).await?;

        // Update metrics with minimal lock time
        {
            let mut metrics = self.metrics.write().await;
            metrics.jobs_failed += 1;
        }

        Ok(())
    }

    pub async fn get_queue_length(&self) -> Result<usize, crate::redis::error::Error> {
        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        let result: bb8_redis::redis::RedisResult<usize> =
            bb8_redis::redis::cmd("LLEN").arg(&self.queue_key).query_async(&mut *conn).await;

        match result {
            Ok(len) => {
                debug!("Queue {} has {} jobs remaining", self.queue_key, len);
                Ok(len)
            },
            Err(e) => {
                error!("Error getting queue length for {}: {:?}", self.queue_key, e);
                Err(crate::redis::error::Error::RedisError(e))
            },
        }
    }
}

enum StatsUpdate {
    JobCompleted { blocks_processed: usize },
    Error,
}

#[derive(Debug, Default, Clone)]
struct WorkerStats {
    jobs_processed: usize,
    blocks_processed: usize,
    errors: usize,
    start_time: Option<std::time::Instant>,
}

pub struct BlockWorker {
    reconciler: Arc<BlockReconciler>,
    queue: Arc<BlockBackfillQueue>,
    processors: Vec<Arc<dyn EventProcessor>>,
    concurrency: usize,
    shutdown: Arc<RwLock<bool>>,
    stats: WorkerStats,
}

impl BlockWorker {
    pub fn new(
        reconciler: Arc<BlockReconciler>,
        queue: Arc<BlockBackfillQueue>,
        concurrency: usize,
    ) -> Self {
        Self {
            reconciler,
            queue,
            processors: Vec::new(),
            concurrency,
            shutdown: Arc::new(RwLock::new(false)),
            stats: WorkerStats {
                start_time: Some(std::time::Instant::now()),
                ..Default::default()
            },
        }
    }

    pub fn add_processor<P: EventProcessor + 'static>(&mut self, processor: Arc<P>) {
        info!("Adding processor to block backfill worker: {}", std::any::type_name::<P>());
        self.processors.push(processor);
    }

    async fn log_stats(&self, queue_length: usize) {
        let elapsed = if let Some(start_time) = self.stats.start_time {
            start_time.elapsed()
        } else {
            Duration::from_secs(0)
        };

        let blocks_per_second = if elapsed.as_secs() > 0 {
            self.stats.blocks_processed as f64 / elapsed.as_secs() as f64
        } else {
            0.0
        };

        info!(
            "Block backfill progress: {} jobs, {} blocks processed ({:.2} blocks/sec), {} errors, {} jobs remaining in queue",
            self.stats.jobs_processed,
            self.stats.blocks_processed,
            blocks_per_second,
            self.stats.errors,
            queue_length
        );
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Starting block backfill worker with concurrency {}",
            self.concurrency
        );

        // Channel for stats updates from tasks
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StatsUpdate>(100);

        // Create a dedicated task for processing stats updates
        let stats = Arc::new(RwLock::new(self.stats.clone()));
        let stats_task = {
            let stats = stats.clone();
            tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    let mut stats_guard = stats.write().await;
                    match update {
                        StatsUpdate::JobCompleted { blocks_processed } => {
                            stats_guard.jobs_processed += 1;
                            stats_guard.blocks_processed += blocks_processed;
                            info!(
                                "Stats update: {} jobs, {} blocks processed, {} errors",
                                stats_guard.jobs_processed,
                                stats_guard.blocks_processed,
                                stats_guard.errors
                            );
                        },
                        StatsUpdate::Error => {
                            stats_guard.errors += 1;
                        },
                    }
                }
            })
        };

        let mut handles = Vec::new();
        let mut last_progress_log = std::time::Instant::now();

        loop {
            // Check if we should shutdown
            if *self.shutdown.read().await {
                break;
            }

            // Update local stats from the shared stats
            self.stats = stats.read().await.clone();

            // Clean up completed tasks
            handles.retain(
                |h: &tokio::task::JoinHandle<
                    Result<(), Box<dyn std::error::Error + Send + Sync>>,
                >| !h.is_finished(),
            );
            let active_tasks = handles.len();

            // Log progress every 10 seconds
            if last_progress_log.elapsed() > Duration::from_secs(10) {
                let queue_length = self.queue.get_queue_length().await.unwrap_or(0);
                self.log_stats(queue_length).await;
                last_progress_log = std::time::Instant::now();
            }

            // If we have capacity, try to get more jobs
            if active_tasks < self.concurrency {
                match self.queue.get_job().await {
                    Ok(Some(job)) => {
                        let reconciler = Arc::clone(&self.reconciler);
                        let processors = self.processors.clone();
                        let tx_clone = tx.clone();
                        let queue = Arc::clone(&self.queue);
                        
                        let shard_id = job.shard_id;
                        let start_block = job.start_block;
                        let end_block = job.end_block;
                        let job_id = job.id.clone();
                        let block_count = end_block - start_block + 1;

                        info!("Starting block backfill job with blocks {}-{} for shard {}", 
                             start_block, end_block, shard_id);

                        let handle = tokio::spawn(async move {
                            let job_start_time = std::time::Instant::now();
                            let mut job_success_count = 0;
                            let mut job_error_count = 0;

                            // Set up a semaphore to limit concurrent reconciliations
                            let semaphore = Arc::new(tokio::sync::Semaphore::new(10));

                            // Process all blocks in the job
                            for processor in &processors {
                                let reconciler_clone = Arc::clone(&reconciler);
                                let processor_clone = Arc::clone(processor);
                                let _semaphore_clone = Arc::clone(&semaphore); // Prefix with underscore since it's currently unused
                                
                                match reconciler_clone
                                    .reconcile_block_range(shard_id, start_block, end_block, processor_clone)
                                    .await
                                {
                                    Ok(_) => {
                                        job_success_count += 1;
                                        info!("Successfully processed blocks {}-{} for shard {}", 
                                              start_block, end_block, shard_id);
                                    },
                                    Err(e) => {
                                        error!("Error reconciling blocks {}-{} for shard {}: {:?}", 
                                              start_block, end_block, shard_id, e);
                                        job_error_count += 1;
                                        if let Err(e) = tx_clone.send(StatsUpdate::Error).await {
                                            error!("Failed to send error update: {}", e);
                                        }
                                    },
                                }
                            }

                            let elapsed = job_start_time.elapsed();
                            info!(
                                "Completed block backfill job with {} blocks (processed: {}, errors: {}) in {:.2?}",
                                block_count,
                                job_success_count,
                                job_error_count,
                                elapsed
                            );

                            // Mark the job as completed
                            if job_error_count == 0 {
                                if let Err(e) = queue.complete_job(&job_id, block_count).await {
                                    error!("Failed to mark job as completed: {}", e);
                                }
                                
                                // Send update to main task
                                if let Err(e) = tx_clone
                                    .send(StatsUpdate::JobCompleted { blocks_processed: block_count as usize })
                                    .await
                                {
                                    error!("Failed to send job completion update: {}", e);
                                }
                            } else {
                                // Retry the job
                                if let Err(e) = queue.retry_job(job).await {
                                    error!("Failed to retry job: {}", e);
                                }
                            }

                            Ok(())
                        });

                        handles.push(handle);
                    },
                    Ok(None) => {
                        // No jobs available, wait a bit
                        time::sleep(Duration::from_secs(1)).await;
                    },
                    Err(e) => {
                        error!("Error getting job: {:?}", e);
                        self.stats.errors += 1;
                        time::sleep(Duration::from_secs(1)).await;
                    },
                }
            } else {
                // All workers busy, wait a bit
                time::sleep(Duration::from_millis(100)).await;
            }
        }

        info!("Block backfill worker shutting down");

        // Shutdown the stats task
        stats_task.abort();

        // Final stats sync
        self.stats = stats.read().await.clone();

        // Wait for in-progress tasks to complete
        for handle in handles {
            if !handle.is_finished() {
                match handle.await {
                    Ok(_) => {},
                    Err(e) => error!("Error joining task during shutdown: {:?}", e),
                }
            }
        }

        // Log final statistics
        let queue_length = self.queue.get_queue_length().await.unwrap_or(0);
        self.log_stats(queue_length).await;

        Ok(())
    }

    pub async fn stop(&self) {
        info!("Requesting block backfill worker to stop");
        *self.shutdown.write().await = true;
    }
}