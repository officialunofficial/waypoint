//! MCP (Machine-Readable Client Protocol) service implementation

mod base;
pub use base::{McpService, WaypointMcpService, NullDb, MooCow};

mod handlers;
pub use handlers::WaypointMcpTools;