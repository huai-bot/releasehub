// One console executable for MCP and --cli queries; no WebView is started.
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let json_cli = args.iter().any(|arg| arg == "--cli") && args.iter().any(|arg| arg == "--json");
    if let Err(message) = releasehub_lib::run_mcp() {
        // --cli --json already emitted one error envelope; keep machine output clean.
        if !json_cli { eprintln!("{message}"); }
        std::process::exit(1);
    }
}
