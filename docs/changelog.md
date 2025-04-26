# Changelog

All notable changes to Waypoint will be documented in this file.

## [Unreleased]

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