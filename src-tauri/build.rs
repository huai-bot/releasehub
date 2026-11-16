fn main() {
    println!("cargo:rerun-if-env-changed=RELEASEHUB_GITHUB_CLIENT_ID");
    // This is a public application identifier, never an OAuth client secret.
    let client_id = std::env::var("RELEASEHUB_GITHUB_CLIENT_ID")
        .unwrap_or_else(|_| "Ov23lia2v9F5yO8hPTYk".into());
    assert!(!client_id.trim().is_empty(), "RELEASEHUB_GITHUB_CLIENT_ID must contain the public OAuth app ID");
    println!("cargo:rustc-env=RELEASEHUB_GITHUB_CLIENT_ID={}", client_id.trim());
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/source.svg");
    // A stale developer tools directory must not silently re-bundle gh or local data.
    if let Ok(entries) = std::fs::read_dir("tools") {
        for entry in entries.flatten() {
            assert!(entry.file_name() == "releasehub-mcp.exe" && entry.file_type().is_ok_and(|kind| kind.is_file()),
                "tools may contain only the public releasehub-mcp.exe connector; remove stale build resources before packaging");
        }
    }
    tauri_build::build()
}
