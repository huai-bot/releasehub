# Architecture and collaboration contract

## Product boundaries

ReleaseHub is a portable-first Windows x64 desktop application. Release monitoring, a native GitHub workspace, and a read-only external-AI connector are the current modules. Codex / Claude Code run outside this application and connect through the bundled MCP stdio bridge; no model keys or model execution belong in ReleaseHub. The legacy build workboard remains for compatibility, not as the primary Agent workflow. No generic plugin loader or speculative marketplace is needed.

The UI never talks to arbitrary remote APIs. It invokes one Tauri command `dispatch` with `{ action: string, payload: object }`. The host/core validate every payload. Public GitHub API access is anonymous until the user authorizes the bundled official GitHub CLI; a legacy manually configured token is retained only for compatibility. The UI's production CSP excludes remote scripts and frames. Rich release notes and repository READMEs are sanitized.

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
| get_settings | {} | Settings only; used by idle scheduling without loading the library |
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
| rotate_agent_token | {} | AgentConnection (legacy only; no normal UI entry) |
| get_agent_integration | {} | AgentIntegration: command, args, claudeConfig, codexConfig, readOnly:true; no secrets |
| github_status / github_login / github_cancel_login / github_logout | {} | GithubStatus; logout disconnects this application only |
| github_repositories | { tab:'explore'\|'starred'\|'owned', query:string, page:number } | GithubRepositoryPage; 30/page, page 1..10; owned/starred filter the current page only |
| github_repository | { repository:'owner/repo' } | GithubRepositoryDetail: repository and bounded README |
| github_set_star | { repository, starred:boolean, confirmed:true } | { starred:boolean }; explicit 204 plus readback |
| github_update_repository | { repository, description:string, homepage:string, confirmed:true } | GithubRepository; only two fields, current account/admin checked |
| github_activity | { page:number } | GithubActivityPage; received_events, not a real-time all-Star feed |

`save_settings` is intercepted by the host to start/stop/rebind the Agent server safely; on failure preserve previous enabled settings and report the error. `snapshot` is enriched by the host. The browser-only development preview must be clearly marked and never pretend real operations succeeded. Production must never silently fall back to demo data.

## GitHub authorization and fixed operations

`src-tauri/src/github.rs` owns `GithubService`, using official gh 2.100.0. Login is `gh auth login --hostname github.com --web --skip-ssh-key`; only its validated short code and the fixed GitHub device URL reach the UI. No borrowed OAuth client ID, token extraction, model configuration, shell interpolation, setup-git or auth logout. Before accepting a login, require exactly one active successful `github.com` account with `tokenSource=keyring`, then verify `/user` agrees. Mutations recheck identity, require confirmation, and validate their result instead of trusting process exit 0.

The host passes `app_local_data_dir/accounts/<instance-id>` as the authorization root even in portable mode. gh config is isolated there, but Windows `gh:github.com` credentials remain shared by account with official CLI users. A non-keyring login is rejected and this application's exact hosts configuration removed without reading its token. Disconnect changes only the local connection flag and clears application UI state.

Every gh call uses a direct executable plus argument array, a fixed constructed `https://api.github.com/...` URL, bounded output, fixed timeout, cleared token/debug/browser overrides, and no automatic write retry. Anonymous reqwest requests disable redirects; signed requests follow gh's own protection (not a claim of universally disabled redirects). No arbitrary endpoint/gh command is exposed. Core's internal optional `repository_api(owner,repo,tail,page)` shares this authorization for metadata/releases/commits/compare GETs without copying credentials. The outer None means disconnected; inner Ok(None) means 404. Signed reads bypass the old anonymous cache/cooldown. Download transport is unchanged and never receives authorization.

## External-AI and legacy Agent interfaces

Off by default. Bind only 127.0.0.1, default port 47831, adjustable in Settings. Require a locally generated bearer credential on every request. Reject Origin headers and invalid Host, do not enable CORS, bound request bodies and traffic, and compare credentials in constant time. Credentials live in Windows keyring under a portable instance ID. No credentials accompany portable exports/releases; a new computer reconnects explicitly.

`tools/releasehub-mcp.exe` is a separate console executable using newline-delimited JSON-RPC over stdin/stdout; it does not initialize a WebView or open the SQLite database. The generated Claude JSON/Codex TOML config contains only this executable and `--data-dir`. A safe `agent-connection.json` descriptor stores schema version and loopback address; the helper reads the separate `agent-read` credential itself. It disables proxies/redirects and accepts only the local nonprivileged HTTP port. No arbitrary path or URL tools are exposed.

Four tools: `releasehub_status`, `releasehub_list_software` (search, updatesOnly, offset, limit 1..100), `releasehub_get_software` (projectId), and `releasehub_saved_context`. They return saved data only, with explicit read-only annotations. The context endpoint projects allowed fields: software/releases/assets, install metadata, download records and selected preferences; never runtime credentials, account/private-repository lists or build-task instructions. Upstream text is untrusted data, not instructions.

The distinct read credential can only GET health/projects/context. Legacy checks/task writes require the separate old credential, cannot be reached through MCP tools, and have no normal creation/secret-reveal UI in 0.2.

Supported routes (lead implements):
- GET /v1/health (authenticated, safe status only)
- GET /v1/projects (metadata only)
- GET /v1/context (allowlisted saved software/download/preferences, read-only)
- POST /v1/check-updates (non-installing update check)
- GET /v1/build-tasks
- POST /v1/build-tasks/:id/claim
- POST /v1/build-tasks/:id/progress (running/failed)
- POST /v1/build-tasks/:id/result (succeeded, URL+commit+optional digest)

There is no route for arbitrary commands, file reads/writes, tokens, settings, installation, or automatic model invocation. The external Agent obtains a pinned source task and performs the build in its explicitly authorized environment, then reports a result URL. Unofficial Agent builds are shown as such and are not automatically installed. Ship a PowerShell/HTTP integration guide instead of modifying the user's DSH installation.

## Portable mode and delivery

Presence of `portable.flag` next to ReleaseHub.exe selects `data/` beside the executable; installed mode uses the platform app-data directory. A non-writable portable directory fails clearly instead of silently losing portability. Credentials are separate. Import/export migrates subscriptions, while copying the portable folder migrates full non-secret local history and managed apps.

Final distribution: Windows x64 EXE installer, portable ZIP (exe + portable.flag + quick-start guide). WebView2 is required; use the shared installed runtime and give an actionable missing-runtime explanation. No bundled private endpoints or developer credentials. Final artifact build is a manual `workflow_dispatch` after implementation and two concrete improvement rounds. Public CI permissions and artifact contents are explicitly reviewed.
