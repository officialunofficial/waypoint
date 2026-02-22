//! MCP (Machine-Readable Client Protocol) service implementation

mod base;
pub use base::McpService;

mod handlers;
pub use handlers::WaypointMcpTools;
