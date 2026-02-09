# Changelog

All notable changes to Waypoint will be documented in this file.

## [2026.2.4] - 2026-02-09

### Miscellaneous Tasks

- Upgrade fred Redis client v9 → v10.1

## [2026.2.2] - 2026-02-08

### Performance

- Remove Hub mutex serialization, enabling concurrent per-FID message-type fetching via `try_join!`
- Add batch root parent resolution via single DB query before bulk cast insert
- Fetch all 5 onchain event types concurrently via `tokio::join!` instead of serially
- Process onchain events before batch message processing to avoid discarding on batch failure
- Fix dropped onchain events in `reconcile_fids_batch`

## [2026.1.2] - 2026-01-01

### Bug Fixes

- Increase Redis max_pool_size to 50
- Fix docker compose timing and add worker improvements (#71)
- Support crates.io packaging without proto submodule (#74)

### Features

- Add unified messages stream and optimize hub client (#70)
- Add root_parent tracking columns
- Add root_parent backfill worker
- Wire up root_parent backfill command (#73)

### Miscellaneous Tasks

- Release v2026.1.1
- Release v2026.1.2
- Release v2026.1.2

## [2025.12.3] - 2025-12-22

### Bug Fixes

- Rename pool_size to max_pool_size to match ConfigMap
- Update remaining pool_size references to max_pool_size

### Features

- Add methods to remove users from spam/nerf lists (#65)
- Add resilience improvements (#67)

### Miscellaneous Tasks

- Bump version to 2025.12.2
- Use snapchain submodule for proto files (#69)
- Release v2025.12.3

### Performance

- Optimize Redis connection usage and event processing (#66)

## [2025.12.1] - 2025-12-12

### Bug Fixes

- Improve Docker networking guidance and error messages for Snapchain connection (#58)
- Embed database migrations in binary for Docker deployment (#60)
- Skip migrations env var
- Docker compose migration skip
- Always process onchain events for spam FIDs (#62)
- Add deleted_at to spammy_users and nerfed_users tables (#64)

### Documentation

- Update MCP documentation for rmcp 0.8.0

### Features

- Add support for multiple spam.jsonl sources
- Add spam filtering to backfill queue command
- Upgrade MCP to rmcp 0.10 with Streamable HTTP transport (#61)
- Add nerfed_users and spammy_users tables (#63)

### Miscellaneous Tasks

- Update Snapchain to v0.9.1
- Update Snapchain to v0.9.2
- Update Snapchain to v0.10.0
- Update Snapchain to v0.11.0
- Fmt
- Bump version to 2025.12.1

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


