# Changelog

All notable changes to Waypoint will be documented in this file.

## [unreleased]

### Bug Fixes

- Resolve backfill job queue exhaustion bug (#37)
- Increase Redis pool sizes to prevent backfill queue exhaustion (#38)
- Handle Redis XPENDING response variations and prevent backfill job loss (#39)
- Sqlx
- Traversal
- Root parent hub
- Use ConfigurationError instead of internal error

### Documentation

- Update mcp.md
- Update docs

### Features

- Support Farcaster Pro (#36)
- Custom headers (#42)
- Update proto to 0.3.1
- Add root parent tracking for cast threads (#44)
- Add HTTP/HTTPS protocol support for hub client gRPC connections (#45)
- Add shard_id support for Subscribe API with dynamic discovery

### Miscellaneous Tasks

- Version and changelog

### Refactor

- Simplify backfill queue to use only normal and inprogress q… (#43)

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


