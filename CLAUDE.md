# Waypoint

Farcaster synchronization tool built in Rust. Syncs data from Farcaster hubs (Snapchain) to Postgres via Redis streams, and exposes it through an MCP server.

## Build & Test

```bash
cargo build            # Build the project
cargo test             # Run all tests
cargo clippy           # Lint (must pass with zero warnings)
cargo fmt --check      # Check formatting
```

Rust edition 2024, minimum rustc 1.92.0.

## Architecture

- **Hub subscriber** (`src/hub/subscriber.rs`) connects to Farcaster hub gRPC, classifies events, publishes to Redis streams
- **Redis streams** (`src/redis/stream.rs`) are the central message queue. Key pattern: `hub:<host>:stream:<event_type>`
- **Consumer** (`src/services/streaming.rs`) reads streams via XREADGROUP, dispatches to database processors
- **Database processors** (`src/processor/database.rs`) write to Postgres via sqlx
- **Backfill reconciler** (`src/backfill/reconciler.rs`) fetches historical data per-FID from hub gRPC
- **Query core** (`src/query/`) contains transport-agnostic query/business logic shared across adapters
- **MCP server** (`src/services/mcp/`) exposes tools, prompts, and resources via Model Context Protocol; `WaypointMcpTools` is the protocol adapter and serializes query results at the transport boundary

## MCP Resource URI Conventions (RFC 3986 / RFC 6570)

All MCP resource URIs use the `waypoint://` scheme and MUST follow these standards:

### Path patterns
- Path segments for identifiers: `waypoint://users/{fid}`, `waypoint://casts/{fid}/{hash}`
- Descriptive sub-paths: `waypoint://users/by-username/{username}`, `waypoint://casts/by-fid/{fid}`

### Query parameters for URLs (RFC 6570 Level 3)
URLs are passed as **query parameters**, never embedded in path segments:
- `waypoint://casts/by-parent-url{?url}` — NOT `waypoint://casts/by-parent-url/https%3A...`
- `waypoint://reactions/by-target-url{?url}` — NOT `waypoint://reactions/by-target-url/https%3A...`

When constructing actual URIs, percent-encode the URL value: `waypoint://casts/by-parent-url?url=https%3A%2F%2Fexample.com`

### Resource template notation
Resource templates use RFC 6570 notation:
- `{fid}`, `{hash}`, `{username}`, `{name}`, `{address}` — simple path expansion
- `{?url}` — query expansion for URL parameters

### Current resource URIs
| Pattern | Description |
|---|---|
| `waypoint://users/{fid}` | User profile by FID |
| `waypoint://users/by-username/{username}` | User profile by username |
| `waypoint://verifications/{fid}` | Verified addresses for a FID |
| `waypoint://verifications/{fid}/{address}` | Specific verification |
| `waypoint://verifications/all-by-fid/{fid}` | All verification messages |
| `waypoint://casts/{fid}/{hash}` | Specific cast |
| `waypoint://casts/by-fid/{fid}` | Recent casts by FID |
| `waypoint://casts/by-mention/{fid}` | Casts mentioning a FID |
| `waypoint://casts/by-parent/{fid}/{hash}` | Replies to a parent cast |
| `waypoint://casts/by-parent-url{?url}` | Replies to a parent URL |
| `waypoint://conversations/{fid}/{hash}` | Full conversation thread |
| `waypoint://reactions/by-fid/{fid}` | Reactions by FID |
| `waypoint://reactions/by-target-cast/{fid}/{hash}` | Reactions to a cast |
| `waypoint://reactions/by-target-url{?url}` | Reactions to a URL |
| `waypoint://links/by-fid/{fid}` | Links by FID |
| `waypoint://links/by-target/{fid}` | Links targeting a FID |
| `waypoint://links/compact-state/{fid}` | Link compact state |
| `waypoint://username-proofs/{fid}` | Username proofs for a FID |
| `waypoint://username-proofs/by-name/{name}` | Username proof by name |

## Code Conventions

- Clippy must pass with zero warnings; use range patterns (`1..=3`) not alternation (`1 | 2 | 3`)
- Use `serde_json::json!` macro for building JSON values, not manual `Map::insert` calls
- Extract helpers to eliminate duplicated logic (e.g., type-name mappers, to-JSON converters)
- Prefer functional iterator chains (`filter_map`, `map`, `collect`) over imperative loops
- Minimize comments — don't add step-by-step narration comments; only comment non-obvious logic
