use crate::config::Config;
use cadence::{
    BufferedUdpMetricSink, Counted, CountedExt, Gauged, Histogrammed, QueuingMetricSink,
    StatsdClient, Timed,
};
use color_eyre::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use once_cell::sync::OnceCell;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tracing::info;

// Improved wrapper for StatsdClient with better error handling
pub struct StatsdClientWrapper {
    client: Arc<StatsdClient>,
    prefix: String,
    use_tags: bool,
}

impl Clone for StatsdClientWrapper {
    fn clone(&self) -> Self {
        Self { client: self.client.clone(), prefix: self.prefix.clone(), use_tags: self.use_tags }
    }
}

impl StatsdClientWrapper {
    pub fn new(client: StatsdClient, prefix: &str, use_tags: bool) -> Self {
        let wrapper = Self { client: Arc::new(client), prefix: prefix.to_string(), use_tags };

        tracing::info!("Created StatsdClient wrapper with use_tags={}", use_tags);
        wrapper
    }

    pub fn count(&self, key: &str, value: u64) {
        if self.use_tags {
            self.client.count_with_tags(key, value as i64).send();
            tracing::trace!("Sent tagged metric: {} = {}", key, value);
        } else {
            match self.client.count(key, value as i64) {
                Ok(_) => tracing::trace!("Sent metric: {} = {}", key, value),
                Err(e) => tracing::warn!("Failed to send metric {}: {}", key, e),
            }
        }
    }

    pub fn incr(&self, key: &str) {
        if self.use_tags {
            self.client.incr_with_tags(key).send();
            tracing::trace!("Sent tagged metric: {}", key);
        } else {
            match self.client.incr(key) {
                Ok(_) => tracing::trace!("Sent metric: {}", key),
                Err(e) => tracing::warn!("Failed to send metric {}: {}", key, e),
            }
        }
    }

    pub fn gauge(&self, key: &str, value: impl Into<f64>) {
        let value = value.into();
        if self.use_tags {
            self.client.gauge_with_tags(key, value).send();
            tracing::trace!("Sent tagged metric: {} = {}", key, value);
        } else {
            match self.client.gauge(key, value) {
                Ok(_) => tracing::trace!("Sent metric: {} = {}", key, value),
                Err(e) => tracing::warn!("Failed to send metric {}: {}", key, e),
            }
        }
    }

    pub fn time(&self, key: &str, value: u64) {
        if self.use_tags {
            self.client.time_with_tags(key, value).send();
            tracing::trace!("Sent tagged metric: {} = {}ms", key, value);
        } else {
            match self.client.time(key, value) {
                Ok(_) => tracing::trace!("Sent metric: {} = {}ms", key, value),
                Err(e) => tracing::warn!("Failed to send metric {}: {}", key, e),
            }
        }
    }

    pub fn histogram(&self, key: &str, value: u64) {
        if self.use_tags {
            self.client.histogram_with_tags(key, value).send();
            tracing::trace!("Sent tagged metric: {} = {}", key, value);
        } else {
            match self.client.histogram(key, value) {
                Ok(_) => tracing::trace!("Sent metric: {} = {}", key, value),
                Err(e) => tracing::warn!("Failed to send metric {}: {}", key, e),
            }
        }
    }

    // Directly check connectivity to the StatsD server
    pub fn check_connectivity(&self) -> bool {
        let test_key = format!("{}.connectivity_test", self.prefix);
        let result = self.client.incr(&test_key);
        match result {
            Ok(_) => {
                tracing::info!("Connectivity test succeeded");
                true
            },
            Err(e) => {
                tracing::warn!("Connectivity test failed: {}", e);
                false
            },
        }
    }
}

// Static client storage
static METRICS_CLIENT: OnceCell<Option<StatsdClientWrapper>> = OnceCell::new();

/// Initialize Prometheus metrics endpoint on the specified address
pub async fn init_prometheus(addr: SocketAddr) -> Result<()> {
    info!("Initializing Prometheus metrics endpoint on {}", addr);

    // Set up the PrometheusBuilder with a binding to our address
    // This will handle the /metrics endpoint automatically
    let builder =
        PrometheusBuilder::new().with_http_listener(addr).add_global_label("service", "waypoint");

    // Install the exporter as the global metrics recorder
    builder
        .install()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to install Prometheus recorder: {}", e))?;

    info!("Prometheus metrics endpoint initialized successfully at http://{}/metrics", addr);

    // Register common metrics descriptions
    register_prometheus_metrics();

    Ok(())
}

/// Initialize Prometheus with default address (0.0.0.0:9090)
pub async fn init_prometheus_default() -> Result<()> {
    let addr: SocketAddr = "0.0.0.0:9090".parse().expect("Failed to parse default metrics address");
    init_prometheus(addr).await
}

/// Register Prometheus metric descriptions
fn register_prometheus_metrics() {
    use metrics::{describe_counter, describe_gauge, describe_histogram};

    // Backfill metrics
    describe_counter!(
        "waypoint_backfill_jobs_processed",
        "Total number of backfill jobs processed"
    );
    describe_counter!("waypoint_backfill_fids_processed", "Total number of FIDs processed");
    describe_gauge!("waypoint_backfill_jobs_in_queue", "Number of backfill jobs in queue");
    describe_counter!("waypoint_backfill_job_errors", "Total number of backfill job errors");
    describe_gauge!("waypoint_backfill_fids_per_second", "Backfill FIDs processing rate");

    // Stream metrics
    describe_counter!("waypoint_stream_events_received", "Total number of stream events received");
    describe_counter!(
        "waypoint_stream_events_processed",
        "Total number of stream events processed"
    );
    describe_counter!("waypoint_stream_events_filtered", "Total number of stream events filtered");
    describe_histogram!(
        "waypoint_stream_processing_time_ms",
        "Stream event processing time in milliseconds"
    );

    // Business logic metrics
    describe_counter!("waypoint_events_by_type", "Events processed by type");
    describe_counter!("waypoint_casts_processed", "Cast events processed");
    describe_counter!("waypoint_reactions_processed", "Reaction events processed");
    describe_counter!("waypoint_follows_processed", "Follow/link events processed");
    describe_counter!("waypoint_user_data_processed", "User data events processed");

    // Error metrics by type
    describe_counter!("waypoint_errors_total", "Total number of errors by type");
    describe_counter!("waypoint_database_errors", "Database-related errors");
    describe_counter!("waypoint_redis_errors", "Redis-related errors");
    describe_counter!("waypoint_hub_errors", "Hub connection errors");
    describe_counter!("waypoint_processing_errors", "Event processing errors");

    // Database metrics
    describe_gauge!(
        "waypoint_database_connections_active",
        "Number of active database connections"
    );
    describe_histogram!(
        "waypoint_database_query_duration_ms",
        "Database query duration in milliseconds"
    );

    // System metrics
    describe_gauge!("waypoint_system_memory_usage_bytes", "System memory usage in bytes");
    describe_gauge!("waypoint_system_cpu_usage_percent", "System CPU usage percentage");
}

/// Initialize and set up metrics
pub fn setup_metrics(config: &Config) {
    METRICS_CLIENT.get_or_init(|| {
        // Check if metrics are enabled
        tracing::info!("Metrics configuration: enabled={}", config.statsd.enabled);

        if !config.statsd.enabled {
            tracing::info!("Metrics disabled in configuration");
            return None;
        }

        // Get configuration values
        let addr_string = config.statsd.addr.clone();
        let prefix_str = config.statsd.prefix.as_str();
        let use_tags = config.statsd.use_tags;

        // Use the string reference for the rest of the function
        let addr = addr_string.as_str();

        tracing::info!(
            "Using metrics configuration: addr={}, prefix={}, use_tags={}",
            addr,
            prefix_str,
            use_tags
        );

        tracing::info!(
            "Attempting to initialize StatsD metrics with endpoint {} and prefix '{}'",
            addr,
            prefix_str
        );

        // Create Cadence/StatsD client
        match create_statsd_client(addr, prefix_str) {
            Ok(client) => {
                tracing::info!("StatsD metrics initialized successfully with endpoint {}", addr);

                // Create our wrapper
                let client_wrapper = StatsdClientWrapper::new(client, prefix_str, use_tags);

                // Test network connectivity
                if client_wrapper.check_connectivity() {
                    tracing::info!("StatsD connectivity test passed");
                } else {
                    tracing::warn!("StatsD connectivity test failed");
                }

                // Test metrics
                client_wrapper.incr("metrics.initialization");
                client_wrapper.gauge("metrics.test_gauge", 100);

                // Add a network connectivity verification using raw socket
                match std::net::UdpSocket::bind("0.0.0.0:0") {
                    Ok(socket) => {
                        // Try to connect directly to verify network path
                        match socket.connect(addr) {
                            Ok(_) => tracing::info!("UDP socket connection to {} succeeded", addr),
                            Err(e) => {
                                tracing::warn!("UDP socket connection to {} failed: {}", addr, e)
                            },
                        }
                    },
                    Err(e) => tracing::warn!("Failed to create test UDP socket: {}", e),
                }

                // Start the system metrics monitoring
                let client_for_monitoring = client_wrapper.clone();
                tokio::spawn(async move {
                    monitor_system_metrics(client_for_monitoring).await;
                });

                Some(client_wrapper)
            },
            Err(e) => {
                tracing::error!("Failed to create StatsD client: {}", e);
                None
            },
        }
    });
}

// Create a properly configured StatsD client
fn create_statsd_client(
    addr: &str,
    prefix: &str,
) -> Result<StatsdClient, Box<dyn std::error::Error + Send + Sync>> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.set_nonblocking(true)?;

    // The addr is already in host:port format
    let udp_sink = BufferedUdpMetricSink::from(addr, socket)?;
    let queuing_sink = QueuingMetricSink::from(udp_sink);

    Ok(StatsdClient::from_sink(prefix, queuing_sink))
}

// System metrics monitoring loop
async fn monitor_system_metrics(_client: StatsdClientWrapper) {
    let interval = Duration::from_secs(15); // Update every 15 seconds

    loop {
        // Approximate memory usage - in a real system you'd use a proper
        // memory usage library like sysinfo or sys-info-rs
        let mem_usage = std::process::Command::new("ps")
            .args(["o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|output| {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                s.parse::<u64>().ok().map(|kb| kb * 1024) // Convert KB to bytes
            })
            .unwrap_or(0);

        // Set the memory usage metric using our unified function
        set_memory_usage(mem_usage);

        // Simple CPU usage approximation (not accurate, but gives a relative value)
        // In a real system, use a proper CPU usage library
        let cpu_usage = 0.0; // Placeholder - would be implemented with a proper library
        set_cpu_usage(cpu_usage);

        // Sleep until next update
        tokio::time::sleep(interval).await;
    }
}

// Helper function to get the metrics client
fn get_client() -> Option<&'static StatsdClientWrapper> {
    METRICS_CLIENT.get().and_then(|client_opt| client_opt.as_ref())
}

// Backfill metrics
pub fn increment_jobs_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("backfill.jobs_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_backfill_jobs_processed").increment(1);
}

pub fn increment_fids_processed(count: u64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.count("backfill.fids_processed", count);
    }
    // Prometheus metrics
    metrics::counter!("waypoint_backfill_fids_processed").increment(count);
}

pub fn set_jobs_in_queue(count: u64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.gauge("backfill.jobs_in_queue", count as f64);
    }
    // Prometheus metrics
    metrics::gauge!("waypoint_backfill_jobs_in_queue").set(count as f64);
}

pub fn increment_job_errors() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("backfill.job_errors");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_backfill_job_errors").increment(1);
}

pub fn set_backfill_fids_per_second(rate: f64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.gauge("backfill.fids_per_second", rate);
    }
    // Prometheus metrics
    metrics::gauge!("waypoint_backfill_fids_per_second").set(rate);
}

// Stream metrics
pub fn increment_events_received() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("stream.events_received");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_stream_events_received").increment(1);
}

pub fn increment_events_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("stream.events_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_stream_events_processed").increment(1);
}

pub fn increment_events_filtered() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("stream.events_filtered");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_stream_events_filtered").increment(1);
}

pub fn record_event_processing_time(duration: Duration) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.histogram("stream.processing_time", duration.as_millis() as u64);
    }
    // Prometheus metrics
    metrics::histogram!("waypoint_stream_processing_time_ms").record(duration.as_millis() as f64);
}

// Database metrics
pub fn set_database_connections_active(count: u64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.gauge("database.connections_active", count as f64);
    }
    // Prometheus metrics
    metrics::gauge!("waypoint_database_connections_active").set(count as f64);
}

pub fn record_database_query_duration(duration: Duration) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.histogram("database.query_duration", duration.as_millis() as u64);
    }
    // Prometheus metrics
    metrics::histogram!("waypoint_database_query_duration_ms").record(duration.as_millis() as f64);
}

// System metrics
pub fn set_memory_usage(bytes: u64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.gauge("system.memory_usage", bytes as f64);
    }
    // Prometheus metrics
    metrics::gauge!("waypoint_system_memory_usage_bytes").set(bytes as f64);
}

pub fn set_cpu_usage(percent: f64) {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.gauge("system.cpu_usage", percent);
    }
    // Prometheus metrics
    metrics::gauge!("waypoint_system_cpu_usage_percent").set(percent);
}

// Error tracking metrics
pub fn increment_database_errors() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("errors.database");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_database_errors").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "database").increment(1);
}

pub fn increment_redis_errors() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("errors.redis");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_redis_errors").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "redis").increment(1);
}

pub fn increment_hub_errors() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("errors.hub");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_hub_errors").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "hub").increment(1);
}

pub fn increment_processing_errors() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("errors.processing");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_processing_errors").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "processing").increment(1);
}

// Business logic metrics
pub fn increment_casts_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("events.casts_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_casts_processed").increment(1);
    metrics::counter!("waypoint_events_by_type", "type" => "cast").increment(1);
}

pub fn increment_reactions_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("events.reactions_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_reactions_processed").increment(1);
    metrics::counter!("waypoint_events_by_type", "type" => "reaction").increment(1);
}

pub fn increment_follows_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("events.follows_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_follows_processed").increment(1);
    metrics::counter!("waypoint_events_by_type", "type" => "follow").increment(1);
}

pub fn increment_user_data_processed() {
    // StatsD metrics
    if let Some(client) = get_client() {
        client.incr("events.user_data_processed");
    }
    // Prometheus metrics
    metrics::counter!("waypoint_user_data_processed").increment(1);
    metrics::counter!("waypoint_events_by_type", "type" => "user_data").increment(1);
}

// Timer utility for measuring durations
pub struct MetricsTimer {
    start: Instant,
    metric_name: &'static str,
}

impl MetricsTimer {
    pub fn new(metric_name: &'static str) -> Self {
        Self { start: Instant::now(), metric_name }
    }
}

impl Drop for MetricsTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        if let Some(client) = get_client() {
            client.time(self.metric_name, duration.as_millis() as u64);
        }
    }
}

// Helper to time a function call
pub async fn time_async<F, T>(metric_name: &'static str, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let timer = MetricsTimer::new(metric_name);
    let result = f.await;
    std::mem::drop(timer); // Explicit drop to record the metric
    result
}

// Event processing error metrics
pub fn increment_events_decode_error() {
    if let Some(client) = get_client() {
        client.incr("stream.events_decode_error");
    }
    metrics::counter!("waypoint_stream_events_decode_error").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "decode").increment(1);
}

pub fn increment_events_processing_error() {
    if let Some(client) = get_client() {
        client.incr("stream.events_processing_error");
    }
    metrics::counter!("waypoint_stream_events_processing_error").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "processing").increment(1);
}

pub fn increment_events_timeout() {
    if let Some(client) = get_client() {
        client.incr("stream.events_timeout");
    }
    metrics::counter!("waypoint_stream_events_timeout").increment(1);
    metrics::counter!("waypoint_errors_total", "type" => "timeout").increment(1);
}

pub fn increment_events_dead_lettered() {
    if let Some(client) = get_client() {
        client.incr("stream.events_dead_lettered");
    }
    metrics::counter!("waypoint_stream_events_dead_lettered").increment(1);
}

pub fn increment_events_retried() {
    if let Some(client) = get_client() {
        client.incr("stream.events_retried");
    }
    metrics::counter!("waypoint_stream_events_retried").increment(1);
}

// Consumer lag metrics
pub fn set_consumer_lag(stream: &str, lag: u64) {
    if let Some(client) = get_client() {
        client.gauge(&format!("stream.{}.lag", stream), lag as f64);
    }
    metrics::gauge!("waypoint_stream_consumer_lag", "stream" => stream.to_string()).set(lag as f64);
}

pub fn set_consumer_pending(stream: &str, pending: u64) {
    if let Some(client) = get_client() {
        client.gauge(&format!("stream.{}.pending", stream), pending as f64);
    }
    metrics::gauge!("waypoint_stream_consumer_pending", "stream" => stream.to_string())
        .set(pending as f64);
}
