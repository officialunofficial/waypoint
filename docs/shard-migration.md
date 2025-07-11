# Shard ID Migration Guide

As part of the next Snapchain protocol release, the `shard_id` parameter is becoming required for the Subscribe API. This guide explains how to migrate your Waypoint configuration.

## Background

Event IDs in Snapchain are strictly increasing on a per-shard basis, but they are NOT strictly increasing globally. If you subscribe to multiple shards and only track the highest event ID seen globally, you may miss events from shards that are behind.

## Migration Steps

### 1. Determine Your Shard Configuration

First, decide which shard(s) you need to subscribe to. Waypoint now supports subscribing to multiple shards concurrently in a single instance.

### 2. Update Your Configuration

Add the `shard_indices` to your configuration. Note that shard 0 is typically used for metadata only, so you usually want to start with shard 1:

```bash
# .env file
# Subscribe to a single shard
WAYPOINT_HUB__SHARD_INDICES=1

# Subscribe to multiple shards (skip shard 0)
WAYPOINT_HUB__SHARD_INDICES=1,2,3
```

Or in your config file:
```toml
[hub]
shard_indices = [1, 2, 3]
```

### 3. Event ID Tracking

Waypoint automatically tracks event IDs per shard. The Redis keys are formatted as:
- Per shard: `{hub_host}:shard_{index}` (e.g., `snapchain.farcaster.xyz:3383:shard_0`)

Each shard maintains its own event ID counter, ensuring no events are missed. When subscribing to multiple shards, Waypoint creates separate subscriptions for each shard and tracks their event IDs independently.

### 4. Subscribe to All Shards

You can subscribe to all available shards without knowing the exact count:

```bash
# .env file
WAYPOINT_HUB__SUBSCRIBE_TO_ALL_SHARDS=true
```

Waypoint will automatically discover the number of shards from the hub using the GetInfo API and create subscriptions for each one. This is useful when:
- You want to process all data from the hub
- The number of shards might change
- You're migrating from a single-shard setup

## Example Configurations

### Single Shard
```bash
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383
WAYPOINT_HUB__SHARD_INDICES=0
```

### Multiple Shards (Single Instance)
```bash
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383
WAYPOINT_HUB__SHARD_INDICES=0,1,2
```

This creates three concurrent subscriptions, each tracking its own event ID.

## Verification

After updating your configuration, check the logs:
- You should see: `Hub has X shards available`
- For each shard: `Starting subscriber for shard X`
- If successful: `Established gRPC stream connection`

Waypoint validates that configured shards exist. If you specify a shard that doesn't exist (e.g., shard 10 when only 3 shards are available), you'll get an error.

## Redis Key Changes

Your event ID tracking keys will change:
- Old format: `{hub_host}:default`
- New format: `{hub_host}:shard_{index}`

The old event IDs will remain in Redis but won't be used. You may want to manually delete them after migration.

## Troubleshooting

1. **Error: "No shard indices configured"**
   - Add `WAYPOINT_HUB__SHARD_INDICES` to your configuration
   - Or temporarily set `WAYPOINT_HUB__SUBSCRIBE_TO_ALL_SHARDS=true`

2. **Missing events after migration**
   - Check that you're subscribing to the correct shard
   - Verify Redis keys are being updated: `redis-cli get "{hub_host}:shard_{index}"`

3. **High memory usage**
   - If subscribing to all shards, consider running separate instances per shard
   - Each shard processes events independently