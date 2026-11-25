mod agent;
pub mod core;
mod host;
mod mcp;
mod github;
mod github_oauth;
mod github_cache;
mod github_feed;
mod reader;
pub mod proxy;
mod secrets;

pub fn run_mcp() -> Result<(), String> { mcp::run() }

pub fn run() {
    host::run();
}
