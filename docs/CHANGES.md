## Recent Changes

This document tracks recent changes to the Waypoint project that might not yet be reflected in the main documentation.

### Configuration Changes

- Replaced `store_raw_messages` with `store_messages` option in the database configuration. When set to `false`, the system will completely skip storing data in the messages table while still processing all data in the type-specific tables. This can significantly reduce database size for high-volume deployments.

```toml
[database]
# Set to false to skip storing messages in the messages table completely
store_messages = false
```

### Version 0.4.3 Changes

- Changed database storage configuration to allow completely skipping the messages table insert
- Various bug fixes and performance improvements