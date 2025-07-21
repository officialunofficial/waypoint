# HTTP/HTTPS Protocol Support for Hub Client

Waypoint now supports both HTTP and HTTPS protocols for connecting to Farcaster hubs and Snapchain instances.

## Configuration Examples

### Using HTTPS (Default)
When no protocol is specified, HTTPS is used by default:
```toml
[hub]
url = "snapchain.farcaster.xyz:3383"
# Shard configuration (defaults to subscribe_to_all_shards=true)
# shard_indices = [1, 2, 3]  # Subscribe to specific shards
# subscribe_to_all_shards = true  # Or subscribe to all available shards
```

Or explicitly specify HTTPS:
```toml
[hub]
url = "https://snapchain.farcaster.xyz:3383"
# Shard configuration
shard_indices = [1]  # Subscribe to a single shard
```

### Using HTTP (for local development)
For local development with Snapchain or other hubs running without TLS:
```toml
[hub]
url = "http://localhost:3383"
```

### Environment Variables
You can also configure via environment variables:
```bash
# HTTPS (default)
export WAYPOINT_HUB__URL="snapchain.farcaster.xyz:3383"
# Shard configuration
export WAYPOINT_HUB__SHARD_INDICES="1,2,3"  # Subscribe to specific shards
# Or subscribe to all shards (default is true)
export WAYPOINT_HUB__SUBSCRIBE_TO_ALL_SHARDS="true"

# HTTP (for local development)
export WAYPOINT_HUB__URL="http://localhost:3383"

# HTTPS (explicit)
export WAYPOINT_HUB__URL="https://snapchain.farcaster.xyz:3383"
```

## Use Cases

### Production
For production environments, always use HTTPS:
```toml
[hub]
url = "https://hub.production.example.com:3383"
```

### Local Development
When running Waypoint alongside a local Snapchain instance:
```toml
[hub]
url = "http://localhost:2283"  # Default Snapchain port
```

### Docker Compose
When using Docker Compose with service names:
```toml
[hub]
url = "http://snapchain:2283"
```

## Security Considerations
- Always use HTTPS in production environments
- HTTP should only be used for local development or within secure private networks
- The client will automatically configure TLS for HTTPS connections