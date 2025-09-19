# Onchain Events Backfill Implementation

## Summary

This document outlines the complete implementation for backfilling all onchain events for all Farcaster FIDs. The implementation adds support for tracking all 5 onchain event types defined in the Farcaster protocol, with dedicated database tables and backfill capabilities.

## Changes Made

### 1. Database Schema Updates

**New Migration: `migrations/003_add_onchain_event_tables.sql`**

Added 4 new tables for the previously untracked onchain event types:

- **`signer_events`** - For EVENT_TYPE_SIGNER (type 1)
  - Tracks signer key additions, removals, and admin resets
  - Fields: fid, key, key_type, event_type, metadata, metadata_type, blockchain data
  
- **`signer_migrated_events`** - For EVENT_TYPE_SIGNER_MIGRATED (type 2)  
  - Tracks signer migration timestamps
  - Fields: fid, migrated_at, blockchain data

- **`id_register_events`** - For EVENT_TYPE_ID_REGISTER (type 3)
  - Tracks FID registrations, transfers, and recovery changes
  - Fields: fid, to_address, event_type, from_address, recovery_address, blockchain data

- **`storage_rent_events`** - For EVENT_TYPE_STORAGE_RENT (type 4)
  - Tracks storage payment events  
  - Fields: fid, payer, units, expiry, blockchain data

**Note**: `tier_purchases` table already existed for EVENT_TYPE_TIER_PURCHASE (type 5)

### 2. Database Models

**Updated `src/database/models.rs`:**
- Added new enums: `SignerEventType`, `IdRegisterEventType`
- Added new structs: `SignerEvent`, `SignerMigratedEvent`, `IdRegisterEvent`, `StorageRentEvent`
- All follow the existing pattern with UUIDs, timestamps, and blockchain metadata

### 3. Event Processing

**Updated `src/processor/database.rs`:**
- Enhanced `process_onchain_event()` method to handle all 5 event types
- Added specific database insertion logic for each event type
- Maintains backwards compatibility with existing tier purchase processing
- Uses pattern matching for clean separation of event type handling

### 4. Database Providers

**Updated `src/database/providers.rs`:**
- Updated table mapping for onchain message types
- Maps each event type to its specific table instead of generic `onchain_events`

### 5. Backfill Infrastructure  

**New Module: `src/backfill/onchain_events.rs`**
- **`OnChainEventBackfiller`**: Core backfill logic
- **Supports**:
  - Single FID backfill
  - Bulk FID backfill with batching
  - Event type-specific backfill
  - Comprehensive error handling and reporting
- **Features**:
  - Fetches events from hub using existing `get_on_chain_events` RPC
  - Processes events through existing `DatabaseProcessor`
  - Detailed result tracking and error reporting
  - Configurable batch sizes

## Current State Analysis

### Previously Tracked Events
✅ **EVENT_TYPE_TIER_PURCHASE** (5) - Already had dedicated `tier_purchases` table

### Newly Tracked Events (This PR)
🆕 **EVENT_TYPE_SIGNER** (1) - Now tracked in `signer_events` table
🆕 **EVENT_TYPE_SIGNER_MIGRATED** (2) - Now tracked in `signer_migrated_events` table  
🆕 **EVENT_TYPE_ID_REGISTER** (3) - Now tracked in `id_register_events` table
🆕 **EVENT_TYPE_STORAGE_RENT** (4) - Now tracked in `storage_rent_events` table

### Generic Storage
All events continue to be stored in the generic `onchain_events` table for backwards compatibility, with specific events also stored in their dedicated tables for optimized querying.

## Deployment Instructions

### 1. Run Database Migration

```bash
# Apply the new migration to create the tables
# Migration will be applied automatically on next startup
# Or run manually: 
psql -d your_database -f migrations/003_add_onchain_event_tables.sql
```

### 2. Update SQLx Query Cache (if using SQLX_OFFLINE=true)

```bash  
# Regenerate query cache for new tables
cargo sqlx prepare
```

### 3. Deploy Application

The updated processor will automatically start tracking new onchain events as they arrive from the hub.

### 4. Run Backfill (Optional)

Use the backfill infrastructure to populate historical data:

```rust
// Example backfill usage
use waypoint::backfill::onchain_events::OnChainEventBackfiller;

let backfiller = OnChainEventBackfiller::new(hub_client, database, processor);

// Backfill single FID
let result = backfiller.backfill_fid(12345).await?;

// Backfill all FIDs  
let all_fids = backfiller.get_all_fids().await?;
let results = backfiller.backfill_fids(all_fids).await?;

// Backfill FIDs missing specific event type
let missing_fids = backfiller.get_fids_missing_event_type(
    OnChainEventType::EventTypeSigner
).await?;
let results = backfiller.backfill_fids(missing_fids).await?;
```

## Performance Considerations

### Database Performance
- All new tables include appropriate indexes on `fid`, `timestamp`, `block_number`, and event-specific fields
- Uses `ON CONFLICT DO NOTHING` for idempotent insertion
- Follows existing ULID pattern for primary keys

### Backfill Performance  
- Processes events in configurable batches (default: 1000 per hub request)
- Built-in error handling continues processing even if individual events fail
- Supports incremental backfill (only missing FIDs/event types)

### Memory Usage
- Backfill processes events in streaming fashion (not loading all into memory)
- Hub client connection pooling and circuit breaker patterns apply

## Monitoring and Observability

### Metrics
The existing database processor metrics will automatically capture the new event processing:
- `waypoint_database_operations_total` - Will show inserts for new tables
- `waypoint_database_operation_duration_seconds` - Will show performance  
- `waypoint_hub_events_processed_total` - Will show event processing

### Logs
- Backfill operations log progress, errors, and completion statistics
- Database processor logs event processing for new types
- Existing tracing and structured logging applies

## Testing Strategy

### Unit Tests
- Added tests for new data models (`SignerEvent`, etc.)
- Backfill logic includes comprehensive unit tests
- Result tracking and error handling tests

### Integration Testing
1. **Database Migration**: Verify tables created correctly
2. **Event Processing**: Send test events, verify correct table insertion  
3. **Backfill**: Test with known FID, verify data retrieval and insertion
4. **Performance**: Measure backfill throughput with various batch sizes

### Manual Verification
```sql
-- Verify new tables exist
SELECT table_name FROM information_schema.tables 
WHERE table_name IN ('signer_events', 'signer_migrated_events', 'id_register_events', 'storage_rent_events');

-- Check event distribution by type
SELECT type, COUNT(*) FROM onchain_events GROUP BY type ORDER BY type;

-- Verify specific event table population
SELECT COUNT(*) FROM signer_events;
SELECT COUNT(*) FROM id_register_events;
-- etc.
```

## Rollback Plan

If issues are discovered:

1. **Disable Processing**: Set feature flag to skip new event processing
2. **Database Rollback**: Drop new tables if needed
3. **Application Rollback**: Deploy previous version
4. **Data Consistency**: All events remain in generic `onchain_events` table

## Future Enhancements

### API Endpoints
Add REST/GraphQL endpoints for querying specific event types:
- `GET /api/v1/fids/{fid}/signer-events`  
- `GET /api/v1/fids/{fid}/id-register-events`
- etc.

### Analytics
- Event frequency analysis by type
- FID activity patterns based on onchain events  
- Storage rent expiration tracking and notifications

### Backfill Automation
- Scheduled backfill jobs for new FIDs
- Event-driven backfill triggers
- Backfill progress tracking and resumability

## Documentation Updates

This implementation completes the onchain events tracking infrastructure, ensuring all protocol-defined event types are captured and stored efficiently for analysis and querying.