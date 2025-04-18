use crate::{
    config::{Config, DatabaseConfig},
    database::error::Error,
};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use std::time::Duration;

pub use crate::database::models::{AutoFollow, AutoFollowRow};

pub struct Database {
    pub pool: Pool<Postgres>,
}

impl Database {
    /// Creates a new Database instance using the provided configuration
    pub async fn new(config: &DatabaseConfig) -> Result<Self, Error> {
        let pool = PgPoolOptions::new()
            .min_connections(10)
            .max_connections(config.max_connections)
            .test_before_acquire(true)
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(30))
            .max_lifetime(Duration::from_secs(1800))
            .connect_lazy_with(config.url.clone().parse()?);

        // Validate pool on startup
        let db = Database { pool };
        if !db.check_connection().await? {
            return Err(Error::ConnectionError("Failed initial connection check".into()));
        }

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

        loop {
            match f(&self.pool).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(e);
                    }
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
}
