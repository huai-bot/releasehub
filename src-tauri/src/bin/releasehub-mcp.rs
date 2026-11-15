// Console-subsystem executable: MCP clients own stdin/stdout; no WebView is started.
fn main() {
    if let Err(message) = releasehub_lib::run_mcp() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
