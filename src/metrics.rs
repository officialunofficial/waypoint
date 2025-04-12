use crate::config::Config;
use cadence::{
    BufferedUdpMetricSink, Counted, CountedExt, Gauged, Histogrammed, QueuingMetricSink,
    StatsdClient, Timed,
};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

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
async fn monitor_system_metrics(client: StatsdClientWrapper) {
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

        // Set the memory usage metric
        client.gauge("system.memory_usage", mem_usage as f64);

        // Simple CPU usage approximation (not accurate, but gives a relative value)
        // In a real system, use a proper CPU usage library
        let cpu_usage = 0.0; // Placeholder - would be implemented with a proper library
        client.gauge("system.cpu_usage", cpu_usage);

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
    if let Some(client) = get_client() {
        client.incr("backfill.jobs_processed");
    }
}

pub fn increment_fids_processed(count: u64) {
    if let Some(client) = get_client() {
        client.count("backfill.fids_processed", count);
    }
}

pub fn set_jobs_in_queue(count: u64) {
    if let Some(client) = get_client() {
        client.gauge("backfill.jobs_in_queue", count as f64);
    }
}

pub fn increment_job_errors() {
    if let Some(client) = get_client() {
        client.incr("backfill.job_errors");
    }
}

pub fn set_backfill_fids_per_second(rate: f64) {
    if let Some(client) = get_client() {
        client.gauge("backfill.fids_per_second", rate);
    }
}

// Stream metrics
pub fn increment_events_received() {
    if let Some(client) = get_client() {
        client.incr("stream.events_received");
    }
}

pub fn increment_events_processed() {
    if let Some(client) = get_client() {
        client.incr("stream.events_processed");
    }
}

pub fn increment_events_filtered() {
    if let Some(client) = get_client() {
        client.incr("stream.events_filtered");
    }
}

pub fn record_event_processing_time(duration: Duration) {
    if let Some(client) = get_client() {
        client.histogram("stream.processing_time", duration.as_millis() as u64);
    }
}

// System metrics
pub fn set_memory_usage(bytes: u64) {
    if let Some(client) = get_client() {
        client.gauge("system.memory_usage", bytes as f64);
    }
}

pub fn set_cpu_usage(percent: f64) {
    if let Some(client) = get_client() {
        client.gauge("system.cpu_usage", percent);
    }
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
