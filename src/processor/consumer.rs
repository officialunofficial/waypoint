//! Event processor trait for consuming Redis stream events.
//!
//! This module defines the `EventProcessor` trait which is the core abstraction
//! for processing Hub events from Redis streams. Implementations of this trait
//! are registered with the streaming consumer to handle different types of events.
//!
//! # Example
//!
//! ```ignore
//! use crate::processor::consumer::EventProcessor;
//! use crate::proto::HubEvent;
//!
//! struct MyProcessor;
//!
//! #[async_trait::async_trait]
//! impl EventProcessor for MyProcessor {
//!     async fn process_event(&self, event: HubEvent) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!         // Process the event
//!         Ok(())
//!     }
//! }
//! ```

use crate::proto::HubEvent;

/// Trait for processing Hub events from Redis streams.
///
/// Implementations should handle specific event types and perform
/// the necessary business logic (e.g., database updates, notifications).
#[async_trait::async_trait]
pub trait EventProcessor: Send + Sync + 'static {
    /// Process a single Hub event.
    ///
    /// # Arguments
    /// * `event` - The Hub event to process
    ///
    /// # Returns
    /// * `Ok(())` if processing succeeded
    /// * `Err(...)` if processing failed (event may be retried)
    async fn process_event(
        &self,
        event: HubEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Allows downcasting to concrete types for optimization.
    ///
    /// This is useful when you need to access type-specific methods
    /// or perform batch optimizations on a specific processor type.
    fn as_any(&self) -> &dyn std::any::Any {
        &() // Default implementation returns empty Any
    }
}
