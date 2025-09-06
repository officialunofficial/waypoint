use crate::{
    backfill::reconciler::MessageReconciler, hub::filter::SpamFilter, metrics,
    processor::consumer::EventProcessor, redis::client::Redis,
};
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
    #[serde(default)]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>, // Track when job processing started
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
    pub normal_priority_queue_size: u64,
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
            in_progress_queue_key: format!("{}:inprogress", base_key),
            metrics: Arc::new(tokio::sync::RwLock::new(QueueMetrics::default())),
        }
    }

    /// Get current queue metrics
    /// Uses more efficient pattern without unnecessary cloning
    pub async fn get_metrics(&self) -> QueueMetrics {
        // First fetch the queue sizes without holding the lock
        let normal = self.get_queue_length_for_key(&self.queue_key).await.unwrap_or(0);
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
            normal_priority_queue_size: normal as u64,
            in_progress_queue_size: in_progress as u64,
        }
    }

    async fn get_queue_length_for_key(
        &self,
        key: &str,
    ) -> Result<usize, crate::redis::error::Error> {
        match self.redis.llen(key).await {
            Ok(len) => {
                info!("Queue {} has {} jobs remaining", key, len as usize);
                Ok(len as usize)
            },
            Err(e) => {
                error!("Error getting queue length for {}: {:?}", key, e);
                Err(e)
            },
        }
    }

    pub async fn add_job(&self, mut job: BackfillJob) -> Result<(), crate::redis::error::Error> {
        // Set job ID if not provided
        if job.id.is_empty() {
            job.id = uuid::Uuid::new_v4().to_string();
        }

        // Always use the normal queue now
        let queue_key = &self.queue_key;

        // Serialize job once to avoid cloning
        let job_data = serde_json::to_string(&job)
            .map_err(|e| crate::redis::error::Error::DeserializationError(e.to_string()))?;

        let fid_count = job.fids.len();
        let job_id = job.id.clone();

        info!(
            "Adding backfill job {} with {} FIDs to queue: {}, FID range: {:?}..{:?}",
            job_id,
            fid_count,
            queue_key,
            job.fids.first(),
            job.fids.last()
        );

        let result = self.redis.lpush(queue_key, vec![job_data.to_string()]).await;

        match &result {
            Ok(_) => {
                info!(
                    "Successfully added job {} to queue {} with {} FIDs",
                    job_id, queue_key, fid_count
                );
                // Update metrics with minimal lock time
                {
                    let mut metrics = self.metrics.write().await;
                    metrics.jobs_queued += 1;
                }

                // Update StatsD metrics
                metrics::set_jobs_in_queue(fid_count as u64);

                // Log current queue length after adding
                if let Ok(queue_len) = self.get_queue_length_for_key(queue_key).await {
                    info!(
                        "Queue {} now has {} jobs after adding job {}",
                        queue_key, queue_len, job_id
                    );
                }
            },
            Err(e) => error!("Failed to add job {} to queue {}: {:?}", job_id, queue_key, e),
        }

        result.map(|_| ())
    }

    pub async fn get_job(&self) -> Result<Option<BackfillJob>, crate::redis::error::Error> {
        if self.redis.is_pool_under_pressure() {
            let (total, available) = self.redis.get_pool_health();
            debug!(
                "Redis pool under pressure: {}/{} connections available, delaying job fetch",
                available, total
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Only check the normal queue now
        // Use longer timeout for BRPOP in containerized environments
        let result = self.redis.brpop(vec![self.queue_key.clone()], 5).await?; // 5 second timeout

        match result {
            Some((_, job_data)) => {
                let mut job: BackfillJob = serde_json::from_str(&job_data)
                    .map_err(|e| crate::redis::error::Error::DeserializationError(e.to_string()))?;

                job.state = JobState::InProgress;
                job.attempts += 1;
                job.started_at = Some(chrono::Utc::now()); // Set the start time

                if job.visibility_timeout.is_none() {
                    job.visibility_timeout = Some(300);
                }

                let in_progress_data = serde_json::to_string(&job).unwrap();
                let _ = self.redis.lpush(&self.in_progress_queue_key, vec![in_progress_data]).await;

                // Don't use Redis expiration - we'll handle timeout in cleanup_expired_jobs
                // This prevents jobs from being lost when Redis keys expire

                info!(
                    "Retrieved backfill job {} with {} FIDs from queue {} (attempt: {}), FID range: {:?}..{:?}",
                    job.id,
                    job.fids.len(),
                    &self.queue_key,
                    job.attempts,
                    job.fids.first(),
                    job.fids.last()
                );
                debug!("FIDs in retrieved job: {:?}", job.fids);

                Ok(Some(job))
            },
            None => {
                debug!("No jobs found in queue");
                Ok(None)
            },
        }
    }

    /// Mark a job as completed
    pub async fn complete_job(&self, job_id: &str) -> Result<(), crate::redis::error::Error> {
        // Remove job from in-progress queue by value
        // We need to find and remove the job with matching ID from the in-progress queue
        // First, get all jobs from in-progress queue
        let jobs = self.redis.lrange(&self.in_progress_queue_key, 0, -1).await;

        if let Ok(job_list) = jobs {
            // Find and remove the job with matching ID
            for job_data in job_list {
                if let Ok(job) = serde_json::from_str::<BackfillJob>(&job_data) {
                    if job.id == job_id {
                        // Remove this specific job from the in-progress queue
                        let _ = self.redis.lrem(&self.in_progress_queue_key, 0, &job_data).await;
                        debug!("Removed job {} from in-progress queue", job_id);
                        break;
                    }
                }
            }
        }

        // No need to remove job key since we're not using Redis expiration anymore

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.jobs_processed += 1;

        // Update StatsD metrics
        metrics::increment_jobs_processed();

        info!("Marked job {} as complete and removed from in-progress queue", job_id);

        Ok(())
    }

    /// Move job back to queue after a failure
    pub async fn retry_job(&self, mut job: BackfillJob) -> Result<(), crate::redis::error::Error> {
        // Reset state (attempts already incremented when job was picked up)
        job.state = JobState::Pending;
        // Don't increment attempts here - it was already incremented in get_job

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
        let normal = self.get_queue_length_for_key(&self.queue_key).await?;
        info!("Total queue length: {}", normal);
        Ok(normal)
    }

    /// Clean up expired jobs from the in-progress queue
    /// This helps recover from crashes or other issues where jobs weren't properly completed
    pub async fn cleanup_expired_jobs(&self) -> Result<(), crate::redis::error::Error> {
        // Get all jobs from in-progress queue
        let jobs = self.redis.lrange(&self.in_progress_queue_key, 0, -1).await;

        if let Ok(job_list) = jobs {
            let mut expired_count = 0;
            for job_data in job_list {
                if let Ok(mut job) = serde_json::from_str::<BackfillJob>(&job_data) {
                    // Check if job has exceeded its visibility timeout
                    if let Some(timeout) = job.visibility_timeout {
                        // Use started_at if available, otherwise fall back to created_at
                        let reference_time = job.started_at.unwrap_or(job.created_at);
                        let elapsed = chrono::Utc::now()
                            .signed_duration_since(reference_time)
                            .num_seconds() as u64;

                        if elapsed > timeout {
                            // Job has expired, move it back to queue for retry
                            info!(
                                "Found expired job {} (elapsed: {}s > timeout: {}s), moving back to queue for retry (attempt {})",
                                job.id, elapsed, timeout, job.attempts
                            );

                            // Remove from in-progress queue
                            let _ =
                                self.redis.lrem(&self.in_progress_queue_key, 0, &job_data).await;

                            // Reset state and retry
                            job.state = JobState::Pending;
                            job.started_at = None; // Clear the started_at timestamp
                            self.retry_job(job).await?;
                            expired_count += 1;
                        }
                    }
                }
            }

            if expired_count > 0 {
                info!("Cleaned up {} expired jobs from in-progress queue", expired_count);
            }
        }

        Ok(())
    }
}

// Message types for worker stats updates
enum StatsUpdate {
    JobCompleted { fid_count: usize, spam_count: usize },
    Error,
    HighestFidUpdate(u64),
}

#[derive(Debug, Default, Clone)]
struct WorkerStats {
    jobs_processed: usize,
    fids_processed: usize,
    spam_fids_skipped: usize,
    errors: usize,
    start_time: Option<std::time::Instant>,
    highest_fid_processed: u64,
}

pub struct Worker {
    reconciler: Arc<MessageReconciler>,
    queue: Arc<BackfillQueue>,
    processors: Vec<Arc<dyn EventProcessor>>,
    concurrency: usize,
    shutdown: Arc<RwLock<bool>>,
    stats: WorkerStats,
    spam_filter: Arc<SpamFilter>,
    // Hub connection rate limiter to avoid overwhelming the hub
    hub_connection_limiter: Arc<tokio::sync::Semaphore>,
    // Track if we should auto-queue more FIDs when queue is empty
    auto_queue_enabled: bool,
    max_fid_to_process: Option<u64>,
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

        let hub_connection_limiter = Arc::new(tokio::sync::Semaphore::new(10));

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
            hub_connection_limiter,
            auto_queue_enabled: false,
            max_fid_to_process: None,
        }
    }

    pub fn add_processor<P: EventProcessor + 'static>(&mut self, processor: Arc<P>) {
        info!("Adding processor to backfill worker: {}", std::any::type_name::<P>());
        self.processors.push(processor);
    }

    /// Enable auto-queueing of FIDs when the queue is empty
    pub fn enable_auto_queue(&mut self, max_fid: u64) {
        self.auto_queue_enabled = true;
        self.max_fid_to_process = Some(max_fid);
        info!("Auto-queueing enabled with max FID: {}", max_fid);
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
        // If configured concurrency is too high, reduce it to avoid overwhelming the Hub
        // Database connections are managed by the pool, so we only need to limit hub connections
        let hub_limit = self.hub_connection_limiter.available_permits();
        let effective_concurrency = std::cmp::min(self.concurrency, hub_limit);

        info!(
            "Starting backfill worker with concurrency {} (reduced from {}) and spam filtering enabled. Hub connection limit: {}",
            effective_concurrency, self.concurrency, hub_limit
        );

        // Update the concurrency value
        self.concurrency = effective_concurrency;

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
                            
                            // Update metrics
                            metrics::increment_jobs_processed();
                            metrics::increment_fids_processed(fid_count as u64);
                            
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
                            metrics::increment_job_errors();
                        },
                        StatsUpdate::HighestFidUpdate(fid) => {
                            if fid > stats_guard.highest_fid_processed {
                                stats_guard.highest_fid_processed = fid;
                                info!("Updated highest processed FID to: {}", fid);
                            }
                        },
                    }
                }
            })
        };

        let mut handles = Vec::new();
        let mut last_progress_log = std::time::Instant::now();
        let mut last_cleanup = std::time::Instant::now();

        loop {
            // Check if we should shutdown
            if *self.shutdown.read().await {
                break;
            }

            // Periodically clean up expired jobs (every 60 seconds)
            if last_cleanup.elapsed() > Duration::from_secs(60) {
                if let Err(e) = self.queue.cleanup_expired_jobs().await {
                    error!("Error cleaning up expired jobs: {:?}", e);
                }
                last_cleanup = std::time::Instant::now();
            }

            // Update local stats from the shared stats
            self.stats = stats.read().await.clone();

            handles.retain(
                |h: &tokio::task::JoinHandle<
                    Result<(), Box<dyn std::error::Error + Send + Sync>>,
                >| !h.is_finished(),
            );
            let active_tasks = handles.len();

            // Log progress every 10 seconds
            if last_progress_log.elapsed() > Duration::from_secs(10) {
                let metrics = self.queue.get_metrics().await;
                info!(
                    "Queue state - Normal: {}, In Progress: {}, Total processed: {}",
                    metrics.normal_priority_queue_size,
                    metrics.in_progress_queue_size,
                    metrics.jobs_processed
                );

                let total_queue_length = metrics.normal_priority_queue_size;
                self.log_stats(total_queue_length as usize).await;
                last_progress_log = std::time::Instant::now();
            }

            // If we have capacity, try to get more jobs
            if active_tasks < self.concurrency {
                let pre_metrics = self.queue.get_metrics().await;
                debug!(
                    "Attempting to get job. Current queue state - Normal: {}, In Progress: {}",
                    pre_metrics.normal_priority_queue_size, pre_metrics.in_progress_queue_size
                );

                match self.queue.get_job().await {
                    Ok(Some(mut job)) => {
                        // Limit the number of FIDs processed per job to avoid overwhelming the Hub and database
                        // If there are too many FIDs in a job, split it into smaller chunks
                        // Increased batch size since we're no longer limiting database connections per task
                        const MAX_FIDS_PER_JOB: usize = 50;
                        let original_fid_count = job.fids.len();

                        if original_fid_count > MAX_FIDS_PER_JOB {
                            // Take the first MAX_FIDS_PER_JOB FIDs
                            let remaining_fids = job.fids.split_off(MAX_FIDS_PER_JOB);

                            // Create a new job with the remaining FIDs and add it back to the queue
                            let remaining_job = BackfillJob {
                                fids: remaining_fids.clone(),
                                priority: job.priority.clone(),
                                state: JobState::Pending,
                                visibility_timeout: job.visibility_timeout,
                                attempts: 0, // Reset attempts for the new job
                                created_at: chrono::Utc::now(),
                                id: uuid::Uuid::new_v4().to_string(),
                                started_at: None, // New jobs haven't started yet
                            };

                            // Add the remaining job back to the queue with the same priority
                            info!(
                                "Splitting large job {} with {} FIDs into smaller chunks. Processing first {} FIDs (range: {:?}..{:?}), queuing remaining {} FIDs (range: {:?}..{:?}).",
                                job.id,
                                original_fid_count,
                                MAX_FIDS_PER_JOB,
                                job.fids.first(),
                                job.fids.last(),
                                remaining_job.fids.len(),
                                remaining_job.fids.first(),
                                remaining_job.fids.last()
                            );

                            let remaining_job_id = remaining_job.id.clone();
                            let remaining_fid_count = remaining_job.fids.len();
                            match self.queue.add_job(remaining_job).await {
                                Ok(_) => {
                                    info!(
                                        "Successfully requeued job {} with {} remaining FIDs",
                                        remaining_job_id, remaining_fid_count
                                    );
                                },
                                Err(e) => {
                                    error!(
                                        "Failed to requeue job {} with {} remaining FIDs: {:?}",
                                        remaining_job_id, remaining_fid_count, e
                                    );
                                },
                            }
                        }

                        let reconciler = Arc::clone(&self.reconciler);
                        let processors = self.processors.clone();
                        let tx_clone = tx.clone();
                        let fids = job.fids.clone();
                        let fid_count = job.fids.len();
                        let spam_filter = Arc::clone(&self.spam_filter);
                        let hub_connection_limiter = Arc::clone(&self.hub_connection_limiter);
                        let highest_fid_in_job = *job.fids.iter().max().unwrap_or(&0);

                        info!(
                            "Starting backfill job {} with {} FIDs (range: {:?}..{:?})",
                            job.id,
                            fid_count,
                            job.fids.first(),
                            job.fids.last()
                        );

                        let tx_for_highest = tx_clone.clone();
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

                            // Use the new batch reconciliation method for better database efficiency
                            if !non_spam_fids.is_empty() {
                                let _hub_permit = hub_connection_limiter.acquire().await.unwrap();

                                // Process all FIDs using the batch processor
                                for processor in &processors {
                                    match reconciler
                                        .reconcile_fids_batch(&non_spam_fids, processor.clone())
                                        .await
                                    {
                                        Ok((success_count, error_count)) => {
                                            job_success_count += success_count;
                                            job_error_count += error_count;

                                            info!(
                                                "Batch processed {} FIDs: {} succeeded, {} failed",
                                                non_spam_fids.len(),
                                                success_count,
                                                error_count
                                            );

                                            metrics::increment_fids_processed(success_count as u64);
                                            if error_count > 0 {
                                                if let Err(e) =
                                                    tx_clone.send(StatsUpdate::Error).await
                                                {
                                                    error!("Failed to send error update: {}", e);
                                                }
                                                metrics::increment_job_errors();
                                            }
                                        },
                                        Err(e) => {
                                            error!("Error in batch reconciliation: {:?}", e);
                                            job_error_count += non_spam_fids.len();
                                            if let Err(e) = tx_clone.send(StatsUpdate::Error).await
                                            {
                                                error!("Failed to send error update: {}", e);
                                            }
                                            metrics::increment_job_errors();
                                        },
                                    }
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

                            if highest_fid_in_job > 0 {
                                if let Err(e) = tx_for_highest
                                    .send(StatsUpdate::HighestFidUpdate(highest_fid_in_job))
                                    .await
                                {
                                    error!("Failed to send highest FID update: {}", e);
                                }
                            }

                            Ok(())
                        });

                        // Clone job ID and queue reference for completion tracking
                        let job_id = job.id.clone();
                        let queue_for_completion = Arc::clone(&self.queue);

                        // Wrap the handle to mark job as complete after processing
                        let wrapped_handle = tokio::spawn(async move {
                            let result = handle.await;

                            // Mark job as complete regardless of success/failure
                            // This ensures jobs are properly removed from tracking
                            if let Err(e) = queue_for_completion.complete_job(&job_id).await {
                                error!("Failed to mark job {} as complete: {:?}", job_id, e);
                            }

                            // Flatten the result to match expected type
                            match result {
                                Ok(inner_result) => inner_result,
                                Err(e) => {
                                    Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                                },
                            }
                        });

                        handles.push(wrapped_handle);
                    },
                    Ok(None) => {
                        let post_metrics = self.queue.get_metrics().await;
                        let total_queued = post_metrics.normal_priority_queue_size;

                        // Log detailed state when no jobs found
                        debug!(
                            "No jobs retrieved. Queue state - Normal: {}, In Progress: {}, Active tasks: {}",
                            post_metrics.normal_priority_queue_size,
                            post_metrics.in_progress_queue_size,
                            active_tasks
                        );

                        if total_queued == 0 && post_metrics.in_progress_queue_size == 0 {
                            info!("All queues are empty and no jobs in progress.");

                            // Check if we should auto-queue more FIDs
                            if self.auto_queue_enabled {
                                let current_highest = stats.read().await.highest_fid_processed;
                                if let Some(max_fid) = self.max_fid_to_process {
                                    if current_highest < max_fid {
                                        // Queue the next batch of FIDs
                                        let start_fid = current_highest + 1;
                                        let end_fid = std::cmp::min(start_fid + 499, max_fid);
                                        let batch_fids = (start_fid..=end_fid).collect::<Vec<_>>();

                                        info!(
                                            "Auto-queueing FIDs {} to {} (current highest processed: {})",
                                            start_fid, end_fid, current_highest
                                        );

                                        match self
                                            .queue
                                            .add_job(BackfillJob {
                                                fids: batch_fids,
                                                priority: JobPriority::Normal,
                                                state: JobState::Pending,
                                                visibility_timeout: None,
                                                attempts: 0,
                                                created_at: chrono::Utc::now(),
                                                id: String::new(),
                                                started_at: None, // New jobs haven't started yet
                                            })
                                            .await
                                        {
                                            Ok(_) => {
                                                info!(
                                                    "Successfully auto-queued FIDs {} to {}",
                                                    start_fid, end_fid
                                                );
                                            },
                                            Err(e) => {
                                                error!("Failed to auto-queue FIDs: {:?}", e);
                                            },
                                        }
                                    } else {
                                        info!(
                                            "Backfill complete. Highest FID processed: {}, Max FID: {}",
                                            current_highest, max_fid
                                        );
                                    }
                                }
                            } else {
                                info!("Backfill may be complete. Auto-queueing is disabled.");
                            }
                        } else {
                            debug!(
                                "No jobs available but {} jobs still in progress",
                                post_metrics.in_progress_queue_size
                            );
                        }
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
