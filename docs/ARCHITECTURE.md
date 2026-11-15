# Architecture and collaboration contract

## Product boundaries

ReleaseHub is a portable-first Windows x64 desktop application. Version 0.3 combines release monitoring, a GitHub workspace with Lists and local repository notes, an independently connected website feed, and a general MCP/CLI information connector. External Agent platforms obtain saved software information and perform diagnosis, builds and installation in their own authorized environments. ReleaseHub does not add an executor or model-key configuration. The legacy build workboard remains for compatibility.

The UI never talks to arbitrary remote APIs. It invokes one Tauri command `dispatch` with `{ action: string, payload: object }`. The host/core validate every payload. Public GitHub API access is anonymous until the user authorizes the bundled official GitHub CLI; a legacy manually configured token is retained only for compatibility. The UI's production CSP excludes remote scripts and frames. Rich release notes and repository READMEs are sanitized.

## Shared JSON contract

`src/lib/types.ts` is the source contract. Rust uses serde camelCase for fields and enums. Times are RFC3339 UTC strings. Optional values are explicit null, not empty-string guesses. Errors returned to the UI are short helpful Chinese strings, without credential-bearing URLs or header values.

Snapshot shape: `{ projects, downloads, buildTasks, repositoryNotes, settings, runtime }`.

The host enriches `runtime` with app version, portable mode/data directory, GitHub/Agent status and `proxyPasswordConfigured`; secret values are excluded.

Settings add `startupPage` (one of the six workspace pages), `startupGithubTab` (`explore|owned|starred|activity|feed`), `navigationOrder` (all six pages exactly once), and `proxy: { mode:'system'|'manual'|'direct', url, username }`. Older settings gain defaults. Startup preferences are applied once when the application opens; changing them does not interrupt the current page. Sidebar order supports drag/drop and Alt+Up/Down; Settings remains fixed.

`RepositoryNote` stores `{ repository, name, description, updatedAt }` in a separate SQLite table. Repository keys are lowercase `owner/repo`; local names and descriptions are single-line, limited to 80 and 240 characters. Empty name plus description removes the note. A note does not require a subscription and never changes the upstream repository.

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
| save_settings | { settings: Partial<Settings> } | Settings (host owns Agent lifecycle; proxy changes use save_proxy) |
| get_repository_notes | {} | RepositoryNote[] |
| save_repository_note | { repository, name, description } | RepositoryNote or null after removal |
| export_library | {} | versioned JSON with subscriptions, notes and portable preferences; proxy/account data, install paths and tasks excluded |
| import_library | { contents: string } | { imported, skipped, notesImported }; validate before transaction, preserve local proxy, merge newer notes |
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
| import_library_file | {} | { imported, skipped, notesImported } or null (native open dialog) |
| open_url | { url } | null (validate HTTPS GitHub URLs, then system browser) |
| open_download | { id } | null (show/reveal parent folder, never execute by this action) |
| open_data_directory | {} | null |
| open_managed_version | { projectId, downloadId } | null; validate against list_managed_versions, then open that managed directory only; no execution |
| set_github_token | { token: string } | null (empty removes; OS credential storage, UI clears input) |
| reveal_agent_connection | {} | AgentConnection (only explicit UI action, token never in logs/export) |
| rotate_agent_token | {} | AgentConnection (legacy only; no normal UI entry) |
| get_agent_integration | {} | AgentIntegration: command, args, genericConfig, claudeConfig, codexConfig, cursorConfig, vscodeConfig, cliCommand, tools, readOnly:true |
| get_agent_diagnostic | { projectId } | saved software diagnostic object, also available while MCP is off |
| save_proxy | { proxy: ProxySettings, password?: string } | Settings; omitted password retains matching saved account, empty clears it |
| test_proxy | { proxy?: ProxySettings, password?: string } | { ok, mode, source, target, status, latencyMs, message }; tests unsaved input without saving |
| github_status / github_cancel_login / github_logout | {} | GithubStatus; logout disconnects this application only |
| github_login | { lists?: boolean } | GithubStatus; Lists management explicitly requests the additional user scope |
| github_repositories | { tab:'explore'\|'starred'\|'owned', query:string, page:number } | GithubRepositoryPage; 30/page, page 1..10; owned/starred filter the current page only |
| github_repository | { repository:'owner/repo' } | GithubRepositoryDetail: repository and bounded README |
| github_set_star | { repository, starred:boolean, confirmed:true } | { starred:boolean }; explicit 204 plus readback |
| github_update_repository | { repository, description:string, homepage:string, confirmed:true } | GithubRepository; only two fields, current account/admin checked |
| github_activity | { page:number } | GithubActivityPage; received_events shown separately as 关注动态 |
| github_lists | {} | GithubUserList[] from official GraphQL |
| github_save_list | { id?, name, description, confirmed:true } | created/updated GithubUserList |
| github_delete_list | { id, confirmed:true } | null |
| github_list_repositories | { listId, cursor?, query? } | GithubListRepositoryPage; cursor pagination, all/unlisted/List views |
| github_repository_lists | { repository } | { listIds } |
| github_assign_lists | { repositories, listIds, mode:'add'|'remove'|'replace', confirmed:true } | { updated }; load complete memberships before merging, then read back |
| feed_status / feed_connect / feed_refresh / feed_more / feed_disconnect | {} | GithubFeedPage; independent website session, requestId/sessionRevision reject stale results |

`save_settings` is intercepted by the host to start/stop/rebind the Agent server safely; on failure preserve previous enabled settings and report the error. `snapshot` is enriched by the host. The browser-only development preview must be clearly marked and never pretend real operations succeeded. Production must never silently fall back to demo data.

## GitHub authorization and fixed operations

`src-tauri/src/github.rs` owns `GithubService`, using official gh 2.100.0. Login is `gh auth login --hostname github.com --web --skip-ssh-key`; only its validated short code and the fixed GitHub device URL reach the UI. No borrowed OAuth client ID, token extraction, model configuration, shell interpolation, setup-git or auth logout. Before accepting a login, require exactly one active successful `github.com` account with `tokenSource=keyring`, then verify `/user` agrees. Mutations recheck identity, require confirmation, and validate their result instead of trusting process exit 0.

The host passes `app_local_data_dir/accounts/<instance-id>` as the authorization root even in portable mode. gh config is isolated there, but Windows `gh:github.com` credentials remain shared by account with official CLI users. A non-keyring login is rejected and this application's exact hosts configuration removed without reading its token. Disconnect changes only the local connection flag and clears application UI state.

Every gh call uses a direct executable plus argument array, a fixed constructed `https://api.github.com/...` URL, bounded output, fixed timeout, cleared token/debug/browser overrides, and no automatic write retry. Anonymous reqwest requests disable redirects; signed requests follow gh's own protection (not a claim of universally disabled redirects). No arbitrary endpoint/gh command is exposed. Core's internal optional `repository_api(owner,repo,tail,page)` shares this authorization for metadata/releases/commits/compare GETs without copying credentials. The outer None means disconnected; inner Ok(None) means 404. Signed reads bypass the old anonymous cache/cooldown. Download transport is unchanged and never receives authorization.

## Proxy policy and desktop shell

One shared `Arc<ProxyState>` builds the HTTP clients for metadata, update checks and downloads, and configures only the bundled gh child process environment. System mode uses the Windows static proxy, then environment settings when applicable; direct mode disables proxies. Manual mode accepts HTTP/HTTPS and SOCKS5/SOCKS5h with separate account fields. Loopback connections always bypass the proxy. WebView2 uses its own system/PAC support in system mode; manual authenticated webpage proxies require a system or local forwarding proxy.

Proxy passwords are stored in the Windows credential manager as an account record bound to the exact URL and username. They are not saved in SQLite, snapshots, logs or export files. The host validates first, persists settings under its serial lock, rolls back on a failed save, and rebuilds the independent website window after a change. Existing downloads retain their current connection until cancelled/retried.

The main window disables native decorations and uses the shared Slate/Cobalt theme for a single 52px title/search bar. Window controls use Tauri's scoped minimize, maximize, close, drag and theme APIs; close-to-tray remains a main-window behavior. Native file-drop handling is disabled so HTML sidebar dragging works. The application icon and sidebar mark use the same blue cube asset.

## GitHub Lists and homepage feed

Stars classification uses fixed official GraphQL queries for `User.lists`, `UserList.items`, `createUserList`, `updateUserList`, `deleteUserList`, and `updateUserListsForItem`. The last mutation replaces all membership: add/remove first read every list and membership cursor, merge the requested change, then verify the result. All/unlisted Stars use cursor pagination. List URLs are constructed from the authenticated login and list slug. Names/descriptions shown as local notes remain local.

Homepage feed uses a separate GitHub website window and a machine-local profile beneath the application instance account root. The user logs in there explicitly. `https://github.com/dashboard-feed` is read from the rendered page in its original order and presented as plain text/cards in the application; Events/RSS are not substituted. Content is bounded to 160 items and 2 MiB, links are allowlisted, and changed/unrecognized page structures produce an actionable error. Website session changes discard old cards; disconnect rotates and clears only this module's profile.

The remote website capability permits only `feed_receive`, with a per-window token plus request/session validation. It has no host dispatch or filesystem permission. Host `dispatch` additionally rejects any window label except `main`. Refresh and pagination only operate on the recognized official page controls. Implementation and desktop-login steps are documented in `src/lib/feed/INTEGRATION.md`.

## External-AI and legacy Agent interfaces

Off by default. Bind only 127.0.0.1, default port 47831, adjustable in Settings. Require a locally generated bearer credential on every request. Reject Origin headers and invalid Host, do not enable CORS, bound request bodies and traffic, and compare credentials in constant time. Credentials live in Windows keyring under a portable instance ID. No credentials accompany portable exports/releases; a new computer reconnects explicitly.

`tools/releasehub-mcp.exe` serves newline-delimited JSON-RPC over stdin/stdout, or CLI queries when passed `--cli`. It does not initialize a WebView or open SQLite. Generic JSON, Claude, Codex, Cursor and VS Code configuration contains only this executable and `--data-dir`. A safe `agent-connection.json` descriptor stores schema version and loopback address; the helper reads the separate `agent-read` credential itself. It disables proxies/redirects and accepts only the local nonprivileged HTTP port.

Ten tools: `releasehub_status`, `releasehub_list_software`, `releasehub_get_software`, `releasehub_get_releases`, `releasehub_get_source`, `releasehub_get_downloads`, `releasehub_diagnose`, `releasehub_environment`, `releasehub_snapshot`, and `releasehub_saved_context`. CLI commands use the same information functions; `--cli schema --json` works offline and structured failures exit 1. Query paging uses offset and limit 1..100. The context endpoint projects saved software/releases/assets, install metadata, download records, basic environment and selected preferences; runtime credentials, account/Lists/feed contents and build-task instructions are excluded.

Diagnostics keep unknown versions and locations explicit, include record timestamps and application download errors, and construct source archive links only from a saved full SHA. Tool versions are not probed. The desktop Copy to AI action uses the same diagnostic projection without requiring the MCP switch. An external Agent decides and performs its own work using its own tools and permissions. Upstream descriptions remain untrusted data.

The distinct read credential can only GET health/projects/context. Legacy checks/task writes require the separate old credential, cannot be reached through MCP tools, and have no normal creation/secret-reveal UI in 0.3.

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

Presence of `portable.flag` next to ReleaseHub.exe selects `data/` beside the executable; installed mode uses the platform app-data directory. A non-writable portable directory fails clearly instead of silently losing portability. Credentials are separate. Import/export migrates subscriptions, repository notes and portable preferences, while copying the portable folder migrates full non-secret local history and managed apps. Webpage logins and proxy passwords remain machine-local.

Final distribution: Windows x64 EXE installer, portable ZIP (exe + portable.flag + quick-start guide). WebView2 is required; use the shared installed runtime and give an actionable missing-runtime explanation. No bundled private endpoints or developer credentials. Final artifact build is a manual `workflow_dispatch` after implementation and two concrete improvement rounds. Public CI permissions and artifact contents are explicitly reviewed.
