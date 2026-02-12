//! MCP (Machine-Readable Client Protocol) service implementation

mod base;
pub use crate::database::NullDb;
pub use base::{McpService, MooCow, WaypointMcpService};

mod handlers;
pub use handlers::WaypointMcpTools;
