# Changelog

All notable changes to Waypoint will be documented in this file.

## [unreleased]

### Bug Fixes

- Resolve backfill job queue exhaustion bug (#37)
- Increase Redis pool sizes to prevent backfill queue exhaustion (#38)
- Handle Redis XPENDING response variations and prevent backfill job loss (#39)
- Fix backfill early termination in Docker Compose environments by increasing BRPOP timeout

### Documentation

- Update mcp.md
- Update docs

### Features

- Support Farcaster Pro (#36)
- Custom headers (#42)
- Update proto to 0.3.1
- Add root parent tracking for cast threads

### Refactor

- Simplify backfill queue system to use only normal and inprogress queues (remove unused priority queues)

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


