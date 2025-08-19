# Changelog

All notable changes to Waypoint will be documented in this file.

## [unreleased]

## [2025.8.1] - 2025-08-19

### Bug Fixes

- Resume from checkpoint on restart instead of reprocessing from beginning
  - Fetch latest checkpoint from Redis before reconnecting stream
  - Add periodic checkpointing (every 100 events or 10 seconds)
  - Update last_id variable when reconnecting to use current progress
  - Improve logging to show checkpoint resumption

## [2025.7.3]

### Bug Fixes

- Resolve backfill job queue exhaustion bug (#37)
- Increase Redis pool sizes to prevent backfill queue exhaustion (#38)
- Handle Redis XPENDING response variations and prevent backfill job loss (#39)
- Sqlx
- Traversal
- Root parent hub
- Formatting & imports
- Apply cargo fmt formatting
- Add missing dead code attributes and clippy allow directives
- Update spam filter URL and correct label value logic (#48)
- Use Git LFS media URL directly for spam list
- Remove root parent tracking feature
- Remove root parent migration files

### Documentation

- Update mcp.md
- Update docs

### Features

- Support Farcaster Pro (#36)
- Custom headers (#42)
- Update proto to 0.3.1
- Add root parent tracking for cast threads (#44)
- Add HTTP/HTTPS protocol support for hub client gRPC connections (#45)
- Implement cast retry system with negative caching and background processing
- Add shard_id support for Subscribe API with dynamic discovery (#46)

### Miscellaneous Tasks

- Version and changelog
- Version tick + changelog
- Cargo.lock
- Changelog
- Version bump to 2025.7.3 and update sqlx queries

### Refactor

- Simplify backfill queue to use only normal and inprogress q… (#43)
- Integrate cast retry worker into main application service to avoid dead code warnings

## [0.6.3] - 2025-06-09

### Bug Fixes

- Proto
- Redis defaults

### Miscellaneous Tasks

- Changelog

## [0.6.1] - 2025-05-30

### Miscellaneous Tasks

- Version

## [0.6.0] - 2025-05-30

### Bug Fixes

- Lock file

### Features

- Batch backfill (#30)

## [0.5.5] - 2025-05-17

### Bug Fixes

- Prune idle consumers

### Miscellaneous Tasks

- Bump version to 0.5.5

## [0.4.5] - 2025-05-16

### Bug Fixes

- Consumer cleanup (#32)

### Miscellaneous Tasks

- Support auth addresses (#31)

## [0.4.4] - 2025-05-14

### Bug Fixes

- Align with snapchain release (#29)

### Features

- Spam and message table toggle (#26)

### Miscellaneous Tasks

- Version bump (#25)

## [0.4.1] - 2025-04-26

### Bug Fixes

- Allow http (#11)
- Idle consumers (#18)
- Build version and snapchain bump (#20)
- Delete idle consumers (#21)

### Documentation

- Templates (#13)
- Logos (#17)

### Features

- Wallet implementation, cleanup, and onchain features (#14)

## [0.3.0] - 2025-04-19

### Features

- Reactions and links (#10)

## [0.2.1] - 2025-04-18

### Documentation

- Updates docs and version (#9)

### Features

- Implement MCP server (#2)

### Refactor

- Improve backfill system with standalone docker-compose (#7)


