# Architecture and collaboration contract

## Product boundaries

ReleaseHub is a portable-first Windows x64 desktop application. Release monitoring is the first module; external Agent build coordination is a separate module. Future GitHub favorites/modules can use the same project identity/provider/storage layer, but are explicitly out of this version's scope. No generic plugin loader or speculative marketplace is needed.

The UI never talks to arbitrary remote APIs. It invokes one Tauri command `dispatch` with `{ action: string, payload: object }`. The core validates every payload. Public GitHub API access is anonymous unless the user explicitly configures a token in the OS credential manager. The UI's production CSP excludes remote scripts and frames. Rich release notes must be sanitized.

## Shared JSON contract

`src/lib/types.ts` is the source contract. Rust uses serde camelCase for fields and enums. Times are RFC3339 UTC strings. Optional values are explicit null, not empty-string guesses. Errors returned to the UI are short helpful Chinese strings, without credential-bearing URLs or header values.

Snapshot shape: `{ projects, downloads, buildTasks, settings, runtime }`.

The host enriches `runtime` with app version, portable mode/data directory, and safe GitHub/Agent credential status. It never includes secret values in a snapshot.

## Core facade (backend owner's public Rust interface)

Implement in `src-tauri/src/core/mod.rs`:

```rust
pub type EventSink = std::sync::Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

impl Core {
    pub fn new(data_dir: std::path::PathBuf, portable: bool, events: EventSink)
        -> Result<std::sync::Arc<Self>, String>;
    pub fn set_github_token(&self, token: Option<String>);
    pub async fn handle(self: &std::sync::Arc<Self>, action: &str, payload: serde_json::Value)
        -> Result<serde_json::Value, String>;
}
```

`Core` must be Send + Sync. SQLite operations must not hold a lock across a network await. Downloads run in background with bounded concurrency. Core emits `releasehub:changed` after metadata changes and throttled `releasehub:download` progress (no secret fields). Download failures/cancellation remain visible and recoverable. Persist running downloads as interrupted on restart, rather than claiming completion.

The host provides periodic update scheduling and native notifications based on settings. The core handles checks and optional automatic download of confidently selected compatible assets. `check_updates` itself must not run an installer.

## Actions implemented by core

| action | payload | result |
| --- | --- | --- |
| snapshot | {} | Snapshot (runtime may use host placeholder fields) |
| add_project | { input: AddProjectInput } | Project |
| update_project | { id, changes: ProjectChanges } | Project |
| remove_project | { id } | null (remove tracking, never installed/downloaded files) |
| check_updates | { projectIds?: string[], allowAutoDownload?: boolean } | CheckResult; omitted IDs means all; UI default true, Agent HTTP must force false |
| download_asset | { projectId, assetId } | DownloadRecord (queued) |
| retry_download | { id } | DownloadRecord |
| cancel_download | { id } | null |
| install_download | { id, confirmed: true } | InstallResult |
| mark_installed | { projectId, version, path?: string } | Project (manual confirmation, not a fake detector) |
| list_managed_versions | { projectId } | ManagedVersion[]: downloadId, version, path, active; only recorded, existing, isolated managed installs |
| activate_managed_version | { projectId, downloadId, confirmed: true } | Project; switch the tracked active managed directory/version, never launch, overwrite, or remove a program |
| save_settings | { settings: Partial<Settings> } | Settings (host owns Agent lifecycle) |
| export_library | {} | string (versioned JSON containing project subscriptions/preferences only; never secrets, absolute install paths or tasks) |
| import_library | { contents: string } | { imported: number, skipped: number } (validate sizes/schema, deduplicate, no automatic downloads) |
| create_build_task | { projectId, sourceRef?: string, instructions?: string } | BuildTask |
| cancel_build_task | { id } | BuildTask |
| list_build_tasks | {} | BuildTask[] |
| claim_build_task | { id, agentName: string } | BuildTask (only queued task, transactional) |
| update_build_task | { id, status: 'running'|'succeeded'|'failed', progress?: string, artifactUrl?: string, artifactSha256?: string, sourceCommit?: string, error?: string } | BuildTask (valid transitions; result must point to HTTPS GitHub release/Actions; distinguish unofficial build) |

Support a stable channel (author's latest non-prerelease release) and a preview channel (latest published eligible release including prereleases, not source commit recency). Preserve API failure versus no release. Never compare version strings lexically. Downloads and manually recorded/managed installed versions are separate.

Asset selection considers Windows/x64, installer/ZIP preference, debug/symbol/source/checksum exclusions and ambiguity. Preserve every asset but mark compatibility/reason. If confidence is insufficient, show choices rather than auto-download. Reject explicit ARM/macOS/Linux packages on x64. Case-insensitive names and filenames without platform tags need sensible scores, especially the three requested sample repositories.

Downloads: HTTPS GitHub release domains only; reject userinfo/nonstandard ports/untrusted redirects. Never send GitHub API authorization headers to artifact hosts. Stream to a .part file, enforce finite sizes, compute SHA-256, compare an upstream sha256 digest if supplied, and only then rename as completed. A computed hash without an upstream digest is not publisher verification. Sanitize names and do not overwrite an existing completed asset.

Install: EXE/MSI opens the official installer only with confirmed=true; do not mark installed until verified or the user records it. ZIP extraction checks enclosed paths, links, file count and expanded size. Install only in managed `data/apps/<project-id>/...`, never arbitrary existing installPath. Keep rollback/recovery local to the managed application and preserve known configuration safely. If preservation cannot be established, do not silently replace an existing install: require a user-readable safe fallback. Do not execute anything when creating an Agent task.

## Host-only actions (lead implements)

| action | payload | result |
| --- | --- | --- |
| pick_directory | {} | string or null (native chooser) |
| export_library_file | {} | { path: string } or null (native save dialog) |
| import_library_file | {} | { imported, skipped } or null (native open dialog) |
| open_url | { url } | null (validate HTTPS GitHub URLs, then system browser) |
| open_download | { id } | null (show/reveal parent folder, never execute by this action) |
| open_data_directory | {} | null |
| open_managed_version | { projectId, downloadId } | null; validate against list_managed_versions, then open that managed directory only; no execution |
| set_github_token | { token: string } | null (empty removes; OS credential storage, UI clears input) |
| reveal_agent_connection | {} | AgentConnection (only explicit UI action, token never in logs/export) |
| rotate_agent_token | {} | AgentConnection |

`save_settings` is intercepted by the host to start/stop/rebind the Agent server safely; on failure preserve previous enabled settings and report the error. `snapshot` is enriched by the host. The browser-only development preview must be clearly marked and never pretend real operations succeeded. Production must never silently fall back to demo data.

## Agent interface

Off by default. Bind only 127.0.0.1, default port 47831, adjustable in Settings. Require `Authorization: Bearer <locally-generated-random-token>` on every request. Reject Origin headers and invalid Host, do not enable CORS, bound request bodies and traffic, and compare token in constant time. Tokens live in the OS credential manager, keyed by a portable instance ID. No credentials accompany portable exports or releases; a new computer reconnects explicitly.

Supported routes (lead implements):
- GET /v1/health (authenticated, safe status only)
- GET /v1/projects (metadata only)
- POST /v1/check-updates (non-installing update check)
- GET /v1/build-tasks
- POST /v1/build-tasks/:id/claim
- POST /v1/build-tasks/:id/progress (running/failed)
- POST /v1/build-tasks/:id/result (succeeded, URL+commit+optional digest)

There is no route for arbitrary commands, file reads/writes, tokens, settings, installation, or automatic model invocation. The external Agent obtains a pinned source task and performs the build in its explicitly authorized environment, then reports a result URL. Unofficial Agent builds are shown as such and are not automatically installed. Ship a PowerShell/HTTP integration guide instead of modifying the user's DSH installation.

## Portable mode and delivery

Presence of `portable.flag` next to ReleaseHub.exe selects `data/` beside the executable; installed mode uses the platform app-data directory. A non-writable portable directory fails clearly instead of silently losing portability. Credentials are separate. Import/export migrates subscriptions, while copying the portable folder migrates full non-secret local history and managed apps.

Final distribution: Windows x64 EXE installer, portable ZIP (exe + portable.flag + quick-start guide). WebView2 is required; use the shared installed runtime and give an actionable missing-runtime explanation. No bundled private endpoints or developer credentials. Final artifact build is a manual `workflow_dispatch` after implementation and two concrete improvement rounds. Public CI permissions and artifact contents are explicitly reviewed.
