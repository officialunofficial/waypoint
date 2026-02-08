# Changelog

All notable changes to Waypoint will be documented in this file.

## [2026.2.1] - 2026-02-08

### Bug Fixes

- Fix hub headers: centralize header injection via tonic interceptor, fixing missing auth headers on retry paths and in providers/database/root_parent (#86)
- Route lend_storage to proper stream, add backfill support, handle NOGROUP errors (#87)
- Fix shard_indices parsing
- Improve Makefile portability and comma_separated deserializer

### Features

- Add MCP resources with RFC-compliant resource URIs and query parameters (#84)
- Add Conversation resource type for MCP

### Refactor

- Replace unwrap() patterns with idiomatic let-else, if-let, and is_none_or bindings (#85)
- Simplify event classification and reduce test duplication in subscriber
- Add parse_hash_bytes util for MCP handlers

### Documentation

- Fix get_casts_by_fid and get_verifications_by_fid examples

### Miscellaneous Tasks

- Add dev setup improvements (#83)
- Add extra tests for comma_separated
- Add latest migration file dynamically
- Remove ONCHAIN_EVENTS_BACKFILL.md

## [0.4.1] - 2025-04-26 (Released)

### Fixed
- Remove idle consumers in Redis stream

## [0.4.0] - 2025-04-20 (Released)

### Added
- Ethereum wallet implementation with keystore support
- Support for Ethereum providers and interactions

### Changed
- Cleaned up unused dependencies
- Removed example directory in favor of documentation

## [0.3.0] - 2025-04-19 (Released)

### Added
- MCP tools for finding users by username
- `get_fid_by_username`: Find FID by Farcaster username
- `get_user_by_username`: Get complete user profile by username
- Default "follow" link type for all Link APIs
- Optional username parameter to waypoint_prompt for better ENS support

### Changed
- Improved data architecture documentation with UML diagrams
- Documentation updates to correctly reflect DataContext pattern
- Reduced log verbosity by moving frequent logs to trace level
- Disabled PrintProcessor by default to reduce log noise
- Fixed waypoint_prompt to handle string FIDs
- Enhanced waypoint_prompt to preserve ENS domains (.eth) in usernames

## [0.2.1] - 2025-04-18 (Released)

### Added
- Model Context Protocol (MCP) service for AI assistants to access Farcaster data
- MCP tools for fetching user profiles, verifications, and casts
- Documentation for MCP service setup and usage
- Data architecture documentation with repository pattern
- Updated configuration examples with MCP settings

### Changed
- Improved error handling in MCP service
- Repository pattern for data access
- Updated MCP documentation with comprehensive examples
- Enhanced sequence diagrams for all major components

## [0.1.3] - 2025-04-01

### Added
- Initial release of Waypoint
- Streaming synchronization service for Snapchain events
- Backfill system for historical data processing
- Redis-based job queue and worker system
- Docker and Kubernetes deployment configurations