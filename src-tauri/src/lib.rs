mod agent;
pub mod core;
mod host;
mod mcp;
mod github;
mod secrets;

pub fn run_mcp() -> Result<(), String> { mcp::run() }

pub fn run() {
    host::run();
}
