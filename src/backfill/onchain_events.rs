use crate::database::client::Database;
use crate::database::error::Error;
use crate::hub::client::Hub;
use crate::processor::database::DatabaseProcessor;
use crate::proto::{OnChainEventRequest, OnChainEventType};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, trace, warn};

pub struct OnChainEventBackfiller {
    pub hub: Arc<Mutex<Hub>>,
    pub database: Arc<Database>,
    pub processor: Arc<DatabaseProcessor>,
}

impl OnChainEventBackfiller {
    pub fn new(
        hub: Arc<Mutex<Hub>>,
        database: Arc<Database>,
        processor: Arc<DatabaseProcessor>,
    ) -> Self {
        Self { hub, database, processor }
    }

    /// Backfill all missing onchain events for a specific FID
    pub async fn backfill_fid(&self, fid: u64) -> Result<OnchainEventBackfillResult, Error> {
        info!("Starting onchain events backfill for FID: {}", fid);

        let mut result = OnchainEventBackfillResult::new(fid);

        // Backfill each event type
        for event_type in [
            OnChainEventType::EventTypeSigner,
            OnChainEventType::EventTypeSignerMigrated,
            OnChainEventType::EventTypeIdRegister,
            OnChainEventType::EventTypeStorageRent,
            OnChainEventType::EventTypeTierPurchase,
        ] {
            match self.backfill_event_type(fid, event_type).await {
                Ok(events_processed) => {
                    result.add_success(event_type, events_processed);
                    info!(
                        "Backfilled {} events of type {:?} for FID {}",
                        events_processed, event_type, fid
                    );
                },
                Err(e) => {
                    result.add_error(event_type, e.to_string());
                    error!(
                        "Failed to backfill events of type {:?} for FID {}: {}",
                        event_type, fid, e
                    );
                },
            }
        }

        info!(
            "Completed onchain events backfill for FID {}: {} total events processed",
            fid, result.total_events_processed
        );

        Ok(result)
    }

    /// Backfill all missing onchain events for multiple FIDs
    pub async fn backfill_fids(
        &self,
        fids: Vec<u64>,
    ) -> Result<Vec<OnchainEventBackfillResult>, Error> {
        info!("Starting bulk onchain events backfill for {} FIDs", fids.len());

        let mut results = Vec::new();

        for (index, fid) in fids.iter().enumerate() {
            if index % 100 == 0 {
                info!("Backfill progress: {}/{} FIDs processed", index, fids.len());
            }

            match self.backfill_fid(*fid).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    error!("Failed to backfill FID {}: {}", fid, e);
                    let mut error_result = OnchainEventBackfillResult::new(*fid);
                    error_result.add_error(OnChainEventType::EventTypeNone, e.to_string());
                    results.push(error_result);
                },
            }
        }

        info!("Completed bulk onchain events backfill for {} FIDs", fids.len());
        Ok(results)
    }

    /// Backfill events of a specific type for a FID
    async fn backfill_event_type(
        &self,
        fid: u64,
        event_type: OnChainEventType,
    ) -> Result<u32, Error> {
        trace!("Starting backfill for FID {} and event type {:?}", fid, event_type);

        let mut events_processed = 0u32;
        let page_size = 1000u32;
        let mut page_token = None;

        loop {
            let request = OnChainEventRequest {
                fid,
                event_type: event_type as i32,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard
                    .get_on_chain_events(request)
                    .await
                    .map_err(|e| Error::ConnectionError(format!("Hub error: {}", e)))?
            };

            if response.events.is_empty() {
                break;
            }

            trace!(
                "Processing {} events of type {:?} for FID {}",
                response.events.len(),
                event_type,
                fid
            );

            // Process each event through the database processor
            for event in &response.events {
                if let Err(e) = self.processor.process_onchain_event(event).await {
                    warn!(
                        "Failed to process onchain event for FID {}, type {:?}: {}",
                        fid, event_type, e
                    );
                    // Continue processing other events even if one fails
                } else {
                    events_processed += 1;
                }
            }

            // Check if there are more pages
            if response.next_page_token.as_ref().is_none_or(|t| t.is_empty()) {
                break;
            }

            page_token = response.next_page_token;
        }

        Ok(events_processed)
    }

    /// Get all FIDs that need backfilling using hub info (much faster than database query)
    pub async fn get_all_fids(&self) -> Result<Vec<u64>, Error> {
        let mut hub_guard = self.hub.lock().await;

        // Try to get maximum FID directly from hub
        let max_fid = match hub_guard.get_fids(Some(1), None, Some(true)).await {
            Ok(fids_response) => {
                if let Some(max_fid) = fids_response.fids.first() {
                    info!("Detected maximum FID from hub: {}", max_fid);
                    *max_fid
                } else {
                    // No FIDs found, use hub info
                    self.get_max_fid_from_hub_info(&mut hub_guard).await
                }
            },
            Err(e) => {
                info!("Failed to get FIDs from hub: {}. Falling back to hub info.", e);
                // For sharded hubs, GetFids might not work, so use hub info
                self.get_max_fid_from_hub_info(&mut hub_guard).await
            },
        };

        info!("Generating FID list from 1 to {}", max_fid);
        Ok((1..=max_fid).collect())
    }

    /// Get max FID from hub info as fallback
    async fn get_max_fid_from_hub_info(&self, hub: &mut crate::hub::client::Hub) -> u64 {
        match hub.get_hub_info().await {
            Ok(info) => {
                // Use the total number of FID registrations as an approximation
                let total_fids =
                    info.db_stats.as_ref().map(|stats| stats.num_fid_registrations).unwrap_or(0);

                if total_fids > 0 {
                    info!("Detected {} total FID registrations from hub info", total_fids);
                    // Add some buffer to account for recent registrations
                    let max_fid_estimate = total_fids + 10000;
                    info!(
                        "Using estimated max FID: {} (total registrations + 10k buffer)",
                        max_fid_estimate
                    );
                    max_fid_estimate
                } else {
                    let default_max = 10000;
                    warn!(
                        "No FID registrations found in hub info, using default max FID: {}",
                        default_max
                    );
                    default_max
                }
            },
            Err(e) => {
                warn!("Failed to get hub info: {}. Using default max FID.", e);
                let default_max = 10000;
                warn!("Using default max FID: {}", default_max);
                default_max
            },
        }
    }

    /// Get FIDs that are missing specific onchain event types
    /// For efficiency, we return all FIDs and let the backfill process handle detecting missing events
    pub async fn get_fids_missing_event_type(
        &self,
        _event_type: OnChainEventType,
    ) -> Result<Vec<u64>, Error> {
        info!(
            "Getting all FIDs for event type backfill (individual event detection handled during processing)"
        );
        // Use the same efficient hub-based FID discovery as get_all_fids
        self.get_all_fids().await
    }
}

#[derive(Debug, Clone)]
pub struct OnchainEventBackfillResult {
    pub fid: u64,
    pub event_type_results: Vec<EventTypeResult>,
    pub total_events_processed: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EventTypeResult {
    pub event_type: OnChainEventType,
    pub events_processed: u32,
    pub error: Option<String>,
}

impl OnchainEventBackfillResult {
    fn new(fid: u64) -> Self {
        Self { fid, event_type_results: Vec::new(), total_events_processed: 0, errors: Vec::new() }
    }

    fn add_success(&mut self, event_type: OnChainEventType, events_processed: u32) {
        self.event_type_results.push(EventTypeResult { event_type, events_processed, error: None });
        self.total_events_processed += events_processed;
    }

    fn add_error(&mut self, event_type: OnChainEventType, error: String) {
        self.event_type_results.push(EventTypeResult {
            event_type,
            events_processed: 0,
            error: Some(error.clone()),
        });
        self.errors.push(error);
    }

    /// Check if the backfill was successful (no errors)
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get summary of the backfill operation
    pub fn summary(&self) -> String {
        if self.is_success() {
            format!(
                "FID {}: Successfully processed {} events across {} event types",
                self.fid,
                self.total_events_processed,
                self.event_type_results.len()
            )
        } else {
            format!(
                "FID {}: Processed {} events with {} errors: {}",
                self.fid,
                self.total_events_processed,
                self.errors.len(),
                self.errors.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backfill_result_creation() {
        let mut result = OnchainEventBackfillResult::new(12345);
        assert_eq!(result.fid, 12345);
        assert_eq!(result.total_events_processed, 0);
        assert!(result.is_success());

        result.add_success(OnChainEventType::EventTypeSigner, 10);
        assert_eq!(result.total_events_processed, 10);
        assert!(result.is_success());

        result.add_error(OnChainEventType::EventTypeIdRegister, "Test error".to_string());
        assert!(!result.is_success());
        assert_eq!(result.errors.len(), 1);
    }
}
