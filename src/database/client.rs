use crate::{
    config::{Config, DatabaseConfig},
    database::error::Error,
    metrics,
};
use sqlx::postgres::PgConnectOptions;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use std::{fmt, time::Duration};

pub use crate::database::models::{AutoFollow, AutoFollowRow};

pub struct Database {
    pub pool: Pool<Postgres>,
}

impl Database {
    /// Creates a new Database instance using the provided configuration
    pub async fn new(config: &DatabaseConfig) -> Result<Self, Error> {
        let pool = PgPoolOptions::new()
            .min_connections(5)
            .max_connections(config.max_connections)
            .test_before_acquire(true)
            .idle_timeout(Duration::from_secs(300)) // 5 minutes
            .max_lifetime(Duration::from_secs(3600)) // 1 hour
            .acquire_timeout(Duration::from_secs(config.timeout_seconds))
            .connect_lazy_with(config.url.clone().parse()?);

        // Validate pool on startup
        let db = Database { pool };
        if !db.check_connection().await? {
            return Err(Error::ConnectionError("Failed initial connection check".into()));
        }

        // Start metrics collection for connection pool
        db.start_connection_metrics_monitoring();

        Ok(db)
    }

    /// Creates an empty database instance for testing or mocks
    /// This should not be used in production
    pub fn empty() -> Self {
        // Create a pool configuration that should never be used
        let pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .test_before_acquire(true)
            .connect_lazy_with(
                "postgresql://postgres:postgres@localhost/postgres"
                    .parse()
                    .expect("Failed to parse dummy database URL"),
            );

        Self { pool }
    }

    // Creates a new Database instance using environment variables
    pub async fn from_env() -> Result<Self, Error> {
        let config = Config::load().map_err(|e| Error::ConnectionError(e.to_string()))?;

        Database::new(&config.database).await
    }

    pub async fn with_retry<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: Fn(&Pool<Postgres>) -> futures::future::BoxFuture<'_, Result<R, Error>>,
    {
        let mut attempts = 0;
        let max_attempts = 3;
        let mut delay = Duration::from_millis(100);

        // Get connection info for better error messages
        let db_info = match self.get_connection_info() {
            Ok(info) => info.to_string(),
            Err(_) => "unknown database".to_string(),
        };

        loop {
            match f(&self.pool).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        tracing::error!(
                            database = %db_info,
                            "Database operation failed after {} attempts: {}",
                            max_attempts, e
                        );
                        return Err(e);
                    }

                    tracing::warn!(
                        database = %db_info,
                        "Database operation attempt {} failed, retrying: {}",
                        attempts, e
                    );

                    tokio::time::sleep(delay).await;
                    delay *= 2;
                },
            }
        }
    }

    // Tests the database connection
    pub async fn check_connection(&self) -> Result<bool, Error> {
        self.with_retry(|pool| {
            Box::pin(async move {
                match sqlx::query("SELECT 1").execute(pool).await {
                    Ok(_) => Ok(true),
                    Err(e) => {
                        tracing::warn!("Database connection check failed: {}", e);
                        Ok(false)
                    },
                }
            })
        })
        .await
    }

    /// Gets safe connection details for logging or display.
    /// This method masks sensitive information like passwords in the database URL.
    pub fn get_connection_info(&self) -> Result<DatabaseConnectionInfo, Error> {
        // Get connection options from the pool
        let opts = self.pool.connect_options();
        DatabaseConnectionInfo::from_options(&opts)
    }

    /// Logs information about the database connection.
    /// This is safe to use in production as it masks sensitive information.
    pub fn log_connection_info(&self) {
        match self.get_connection_info() {
            Ok(info) => {
                tracing::info!("Connected to database: {}", info);
            },
            Err(e) => {
                tracing::warn!("Unable to get database connection info: {}", e);
            },
        }
    }

    /// Start monitoring database connection pool metrics
    fn start_connection_metrics_monitoring(&self) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                
                // Track connection pool size
                let connections = pool.size() as u64;
                metrics::set_database_connections_active(connections);
                
                tracing::trace!("Database pool size: {} connections", connections);
            }
        });
    }

    /// Execute a database operation with timing metrics
    pub async fn execute_with_metrics<F, R>(&self, operation_name: &str, f: F) -> Result<R, Error>
    where
        F: Fn(&Pool<Postgres>) -> futures::future::BoxFuture<'_, Result<R, Error>>,
    {
        let start = std::time::Instant::now();
        let result = f(&self.pool).await;
        let duration = start.elapsed();
        
        metrics::record_database_query_duration(duration);
        
        if result.is_err() {
            tracing::warn!("Database operation '{}' failed after {:?}", operation_name, duration);
            metrics::increment_database_errors();
        } else {
            tracing::trace!("Database operation '{}' completed in {:?}", operation_name, duration);
        }
        
        result
    }
}

/// A struct containing safe-to-display database connection information.
/// Sensitive details like passwords are masked.
#[derive(Debug, Clone)]
pub struct DatabaseConnectionInfo {
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub user: String,
}

impl DatabaseConnectionInfo {
    /// Create a new DatabaseConnectionInfo from PgConnectOptions
    fn from_options(options: &PgConnectOptions) -> Result<Self, Error> {
        // Extract fields from the options - we can only see what sqlx exposes publicly
        Ok(Self {
            host: options.get_host().to_string(),
            port: options.get_port(),
            database_name: options.get_database().unwrap_or("unknown").to_string(),
            user: options.get_username().to_string(),
        })
    }
}

impl fmt::Display for DatabaseConnectionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "postgresql://{}@{}:{}/{}", self.user, self.host, self.port, self.database_name,)
    }
}
