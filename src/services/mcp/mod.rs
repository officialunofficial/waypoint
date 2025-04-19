//! MCP (Machine-Readable Client Protocol) service implementation

mod base;
pub use base::{McpService, MooCow, NullDb, WaypointMcpService};

mod handlers;
pub use handlers::WaypointMcpTools;
