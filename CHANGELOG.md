# Changelog

All notable changes to Waypoint will be documented in this file.

## [2025.12.2] - 2025-12-12

### Features

- Add `remove_spammy_user` and `remove_nerfed_user` methods for soft-deleting users from database
- Add `remove_spam_fid` and `remove_nerfed_fid` methods to SpamFilter for in-memory removal
- Support handling `label_value=2` events indicating a user is no longer considered spammy

## [2025.12.1] - 2025-12-12

### Features

- Add spammy_users and nerfed_users tables for spam label persistence
- Parse nerfed users (label_value=3) from merkle spam.jsonl
- Stream ~96MB spam list instead of loading into memory
- Add efficient batch sync methods using sqlx compile-time checked queries
- Add spam filtering to backfill queue command
- Add support for multiple spam.jsonl sources
- Upgrade MCP to rmcp 0.10 with Streamable HTTP transport

### Bug Fixes

- Always process onchain events for spam FIDs
- Embed database migrations in binary for Docker deployment
- Docker compose migration skip
- Improve Docker networking guidance and error messages for Snapchain connection

### Miscellaneous Tasks

- Update Snapchain to v0.11.0

## [2025.10.3] - 2025-10-07

### Bug Fixes

- Update Dockerfile to use Rust 1.90
- Docker buildx
- Update Dockerfile to use Ubuntu 24.04 for GLIBC 2.39 compatibility
- Remove ubuntu image

### Miscellaneous Tasks

- Upgrade rmcp to 0.8.0 and refactor to use latest macros (#55)
- Format code and remove unused const_string declarations

## [2025.10.2] - 2025-10-03

### Features

- Upgrade alloy-rs to v1.0.37 and Rust to 1.90.0

### Miscellaneous Tasks

- Bump version to 2025.10.2

## [2025.10.1] - 2025-10-02

### Features

- Implement LendStorage message type and ProfileToken user data type from Snapchain v0.9.0
- Add lend_storage table migration for storage lending messages
- Add database handler for lend_storage messages

### Miscellaneous Tasks

- Update Snapchain to v0.8.1
- Update Snapchain to v0.9.0
- Update changelog
- Sqlx
- Run cargo fmt
- Migrate biome configuration to schema v2.2.4

## [2025.9.2] - 2025-09-06

### Bug Fixes

- Initialize rustls crypto provider to resolve startup panic

### Features

- Complete Prometheus metrics infrastructure
- Integrate comprehensive metrics throughout application
- Comprehensive onchain events backfill system (#54)

### Miscellaneous Tasks

- Bump version to v2025.9.2

## [2025.9.1] - 2025-09-04

### Bug Fixes

- Remove suffix on stream keys
- Use main branch

### Dependencies

- Bump tracing-subscriber from 0.3.19 to 0.3.20 (#52)

### Features

- Migrate Redis client from bb8-redis to fred with time-based trimming (#51)
- Snapchain version action
- Add Prometheus metrics integration

### Miscellaneous Tasks

- Fmt
- Fmt
- Update Snapchain to v0.6.0
- Update Snapchain to v0.7.0

### Refactor

- Simplify get_stream_key to use 2 args and remove debug logs

## [2025.8.3] - 2025-08-20

### Bug Fixes

- Resume from checkpoint on restart instead of reprocessing from beginning

### Documentation

- Delete CAST_RETRY_SYSTEM.md
- Remove root parent hash docs

### Miscellaneous Tasks

- Cargo lock
- Upgrade dependencies and implement error-stack (#50)
- Upgrade snapchain protos to v0.5.0

## [2025.7.3] - 2025-07-18

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
- Update changelog for v2025.7.3

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


