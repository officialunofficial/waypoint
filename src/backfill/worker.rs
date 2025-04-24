use crate::{
    backfill::reconciler::MessageReconciler, hub::filter::SpamFilter, metrics,
    processor::consumer::EventProcessor, redis::client::Redis,
};
use bb8_redis::redis::RedisResult;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{RwLock, mpsc},
    time,
};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackfillJob {
    pub fids: Vec<u64>,
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

pub struct BackfillQueue {
    redis: Arc<Redis>,
    queue_key: String,
    high_priority_queue_key: String,
    low_priority_queue_key: String,
    in_progress_queue_key: String,
    metrics: Arc<tokio::sync::RwLock<QueueMetrics>>,
}

#[derive(Debug, Default, Clone)]
pub struct QueueMetrics {
    pub jobs_queued: u64,
    pub jobs_processed: u64,
    pub jobs_failed: u64,
    pub fids_processed: u64,
    pub avg_job_time_ms: f64,
    pub high_priority_queue_size: u64,
    pub normal_priority_queue_size: u64,
    pub low_priority_queue_size: u64,
    pub in_progress_queue_size: u64,
}

impl BackfillQueue {
    pub fn new(redis: Arc<Redis>, queue_key: String) -> Self {
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
            metrics: Arc::new(tokio::sync::RwLock::new(QueueMetrics::default())),
        }
    }

    /// Get current queue metrics
    /// Uses more efficient pattern without unnecessary cloning
    pub async fn get_metrics(&self) -> QueueMetrics {
        // First fetch the queue sizes without holding the lock
        let high = self.get_queue_length_for_key(&self.high_priority_queue_key).await.unwrap_or(0);
        let normal = self.get_queue_length_for_key(&self.queue_key).await.unwrap_or(0);
        let low = self.get_queue_length_for_key(&self.low_priority_queue_key).await.unwrap_or(0);
        let in_progress =
            self.get_queue_length_for_key(&self.in_progress_queue_key).await.unwrap_or(0);

        // Then briefly acquire the lock to read other metrics
        let metrics_guard = self.metrics.read().await;

        // Create a new metrics object with copied primitive values (no deep cloning)
        QueueMetrics {
            jobs_queued: metrics_guard.jobs_queued,
            jobs_processed: metrics_guard.jobs_processed,
            jobs_failed: metrics_guard.jobs_failed,
            fids_processed: metrics_guard.fids_processed,
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

        let result: RedisResult<usize> =
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

    pub async fn add_job(&self, mut job: BackfillJob) -> Result<(), crate::redis::error::Error> {
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

        let fid_count = job.fids.len();
        let job_id = job.id.clone();
        // Use a reference to avoid copying non-Copy type
        let priority = &job.priority;

        info!(
            "Adding backfill job {} with {} FIDs to queue: {} (priority: {:?})",
            job_id, fid_count, queue_key, priority
        );

        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        let result: RedisResult<()> = bb8_redis::redis::cmd("LPUSH")
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

                // Update StatsD metrics
                metrics::set_jobs_in_queue(fid_count as u64);
            },
            Err(e) => error!("Failed to add job {} to queue {}: {:?}", job_id, queue_key, e),
        }

        result.map_err(crate::redis::error::Error::RedisError)
    }

    pub async fn get_job(&self) -> Result<Option<BackfillJob>, crate::redis::error::Error> {
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
            let result: RedisResult<Option<Vec<String>>> = bb8_redis::redis::cmd("BRPOP")
                .arg(queue_key)
                .arg(1) // Short timeout to try next queue if empty
                .query_async(&mut *conn)
                .await;

            match result {
                Ok(Some(values)) if values.len() >= 2 => {
                    // BRPOP returns [key, value]
                    let job_data = &values[1];
                    let mut job: BackfillJob = serde_json::from_str(job_data).map_err(|e| {
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
                    let _: RedisResult<()> = bb8_redis::redis::cmd("LPUSH")
                        .arg(&self.in_progress_queue_key)
                        .arg(&in_progress_data)
                        .query_async(&mut *conn)
                        .await;

                    if let Some(timeout) = job.visibility_timeout {
                        // Set expiration on the job - we'll get it back after timeout if not completed
                        let job_key = format!("backfill:job:{}", job.id);
                        let _: RedisResult<()> = bb8_redis::redis::cmd("SET")
                            .arg(&job_key)
                            .arg(&in_progress_data)
                            .arg("EX")
                            .arg(timeout)
                            .query_async(&mut *conn)
                            .await;
                    }

                    info!(
                        "Retrieved backfill job {} with {} FIDs from queue {} (priority: {:?}, attempt: {})",
                        job.id,
                        job.fids.len(),
                        queue_key,
                        job.priority,
                        job.attempts
                    );
                    debug!("FIDs in retrieved job: {:?}", job.fids);

                    return Ok(Some(job));
                },
                Ok(_) => {
                    // Continue to next queue without logging
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
    pub async fn complete_job(&self, job_id: &str) -> Result<(), crate::redis::error::Error> {
        let mut conn = self
            .redis
            .pool
            .get()
            .await
            .map_err(|e| crate::redis::error::Error::PoolError(e.to_string()))?;

        // Remove job from in-progress list
        let _: RedisResult<()> = bb8_redis::redis::cmd("DEL")
            .arg(format!("backfill:job:{}", job_id))
            .query_async(&mut *conn)
            .await;

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.jobs_processed += 1;

        // Update StatsD metrics
        metrics::increment_jobs_processed();

        Ok(())
    }

    /// Move job back to queue after a failure
    pub async fn retry_job(&self, mut job: BackfillJob) -> Result<(), crate::redis::error::Error> {
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

        let result: RedisResult<usize> =
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

// Message types for worker stats updates
enum StatsUpdate {
    JobCompleted { fid_count: usize, spam_count: usize },
    Error,
}

#[derive(Debug, Default, Clone)]
struct WorkerStats {
    jobs_processed: usize,
    fids_processed: usize,
    spam_fids_skipped: usize,
    errors: usize,
    start_time: Option<std::time::Instant>,
}

pub struct Worker {
    reconciler: Arc<MessageReconciler>,
    queue: Arc<BackfillQueue>,
    processors: Vec<Arc<dyn EventProcessor>>,
    concurrency: usize,
    shutdown: Arc<RwLock<bool>>,
    stats: WorkerStats,
    spam_filter: Arc<SpamFilter>,
}

impl Worker {
    pub fn new(
        reconciler: Arc<MessageReconciler>,
        queue: Arc<BackfillQueue>,
        concurrency: usize,
    ) -> Self {
        let spam_filter = Arc::new(SpamFilter::new());

        // Start the update loop for the spam filter
        {
            let filter_clone = Arc::clone(&spam_filter);
            tokio::spawn(async move {
                if let Err(e) = filter_clone.start_updater().await {
                    error!("Failed to initialize spam filter: {}", e);
                }
            });
        }

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
            spam_filter,
        }
    }

    pub fn add_processor<P: EventProcessor + 'static>(&mut self, processor: Arc<P>) {
        info!("Adding processor to backfill worker: {}", std::any::type_name::<P>());
        self.processors.push(processor);
    }

    async fn log_stats(&self, queue_length: usize) {
        let elapsed = if let Some(start_time) = self.stats.start_time {
            start_time.elapsed()
        } else {
            Duration::from_secs(0)
        };

        let fids_per_second = if elapsed.as_secs() > 0 {
            self.stats.fids_processed as f64 / elapsed.as_secs() as f64
        } else {
            0.0
        };

        // Track backfill rate
        metrics::set_backfill_fids_per_second(fids_per_second);

        // Only log non-zero progress
        if self.stats.jobs_processed > 0
            || self.stats.fids_processed > 0
            || self.stats.errors > 0
            || queue_length > 0
        {
            info!(
                "Backfill progress: {} jobs, {} FIDs processed ({:.2} FIDs/sec), {} spam FIDs skipped, {} errors, {} jobs remaining in queue",
                self.stats.jobs_processed,
                self.stats.fids_processed,
                fids_per_second,
                self.stats.spam_fids_skipped,
                self.stats.errors,
                queue_length
            );
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Starting backfill worker with concurrency {} and spam filtering enabled",
            self.concurrency
        );

        // Channel for stats updates from tasks
        let (tx, mut rx) = mpsc::channel::<StatsUpdate>(100);

        // Create a dedicated task for processing stats updates
        let stats = Arc::new(RwLock::new(self.stats.clone()));
        let stats_task = {
            let stats = stats.clone();
            tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    let mut stats_guard = stats.write().await;
                    match update {
                        StatsUpdate::JobCompleted { fid_count, spam_count } => {
                            stats_guard.jobs_processed += 1;
                            stats_guard.fids_processed += fid_count;
                            stats_guard.spam_fids_skipped += spam_count;
                            info!(
                                "Stats update: {} jobs, {} FIDs processed, {} spam FIDs skipped, {} errors",
                                stats_guard.jobs_processed,
                                stats_guard.fids_processed,
                                stats_guard.spam_fids_skipped,
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
                        let fids = job.fids.clone();
                        let fid_count = job.fids.len();
                        let spam_filter = Arc::clone(&self.spam_filter);

                        info!("Starting backfill job with {} FIDs", fid_count);

                        let handle = tokio::spawn(async move {
                            let job_start_time = std::time::Instant::now();
                            let mut job_success_count = 0;
                            let mut job_error_count = 0;
                            let mut job_spam_count = 0;

                            // First filter out spam FIDs
                            let mut non_spam_fids = Vec::new();
                            for fid in fids {
                                if spam_filter.is_spam(fid).await {
                                    info!("Skipping spam FID {}", fid);
                                    job_spam_count += 1;
                                } else {
                                    non_spam_fids.push(fid);
                                }
                            }

                            // Set up a semaphore to limit concurrent Hub connections
                            // Use a smaller value to prevent database connection pool saturation
                            let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
                            let mut tasks = Vec::new();

                            // Process FIDs in parallel using Tokio tasks with semaphore
                            for fid in non_spam_fids {
                                let reconciler_clone = Arc::clone(&reconciler);
                                let processors_clone = processors.clone();
                                let tx_clone2 = tx_clone.clone();
                                let semaphore_clone = Arc::clone(&semaphore);

                                let task = tokio::spawn(async move {
                                    // Acquire permit to limit concurrency of Hub calls
                                    let _permit = semaphore_clone.acquire().await.unwrap();

                                    let fid_start = std::time::Instant::now();
                                    info!("Starting processing FID {}", fid);

                                    let mut success = false;
                                    let mut error = false;

                                    for (idx, processor) in processors_clone.iter().enumerate() {
                                        let processor_name = format!("Processor {}", idx + 1);
                                        info!("Using {} for FID {}", processor_name, fid);

                                        match reconciler_clone
                                            .reconcile_fid(fid, processor.clone())
                                            .await
                                        {
                                            Ok(_) => {
                                                info!(
                                                    "Successfully processed FID {} with {}",
                                                    fid, processor_name
                                                );
                                                success = true;
                                                metrics::increment_fids_processed(1);
                                            },
                                            Err(e) => {
                                                error!(
                                                    "Error reconciling FID {} with {}: {:?}",
                                                    fid, processor_name, e
                                                );
                                                error = true;
                                                if let Err(e) =
                                                    tx_clone2.send(StatsUpdate::Error).await
                                                {
                                                    error!("Failed to send error update: {}", e);
                                                }
                                                metrics::increment_job_errors();
                                            },
                                        }
                                    }

                                    let fid_elapsed = fid_start.elapsed();
                                    info!(
                                        "Completed processing FID {} in {:.2?}",
                                        fid, fid_elapsed
                                    );
                                    (success, error)
                                });

                                tasks.push(task);
                            }

                            // Wait for all tasks to complete
                            for task in tasks {
                                match task.await {
                                    Ok((success, error)) => {
                                        if success {
                                            job_success_count += 1;
                                        }
                                        if error {
                                            job_error_count += 1;
                                        }
                                    },
                                    Err(e) => {
                                        error!("Task failed: {:?}", e);
                                        job_error_count += 1;
                                    },
                                }
                            }

                            let elapsed = job_start_time.elapsed();
                            info!(
                                "Completed backfill job with {} FIDs ({} succeeded, {} failed, {} spam) in {:.2?}",
                                fid_count,
                                job_success_count,
                                job_error_count,
                                job_spam_count,
                                elapsed
                            );

                            // Send update to main task with both regular and spam counts
                            if let Err(e) = tx_clone
                                .send(StatsUpdate::JobCompleted {
                                    fid_count: job_success_count,
                                    spam_count: job_spam_count,
                                })
                                .await
                            {
                                error!("Failed to send job completion update: {}", e);
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

        info!("Backfill worker shutting down");

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
        info!("Requesting backfill worker to stop");
        *self.shutdown.write().await = true;
    }
}
