mod params;
mod root;
mod server;

pub use server::{serve, startup, PerfectPixelMcp, Startup, StartupError, MCP_HELP};
pub(crate) use server::MCP_RESULT_SCHEMA;
