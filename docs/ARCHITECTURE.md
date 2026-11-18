# Architecture and collaboration contract

## Product boundaries

ReleaseHub is a portable-first Windows x64 desktop application. Version 0.4 combines release monitoring, a GitHub workspace with Lists and local repository notes, an independently connected website feed, and a general MCP/CLI information connector. External Agent platforms obtain saved software information and perform diagnosis, builds and installation in their own authorized environments. ReleaseHub does not add an executor or model-key configuration. The legacy build workboard remains for compatibility.

The UI never talks to arbitrary remote APIs. It invokes one Tauri command `dispatch` with `{ action: string, payload: object }`. The host/core validate every payload. Public GitHub API access is anonymous until the user authorizes ReleaseHub's own OAuth application; a legacy manually configured token is retained only for compatibility. The UI's production CSP excludes remote scripts and frames. Rich release notes and repository READMEs are sanitized.

## Shared JSON contract

`src/lib/types.ts` is the source contract. Rust uses serde camelCase for fields and enums. Record times are RFC3339 UTC strings; GitHub cache updatedAt and sessionRevision are epoch-millisecond numbers. Optional values are explicit null, not empty-string guesses. Errors returned to the UI are short helpful Chinese strings, without credential-bearing URLs or header values.

Snapshot shape: `{ projects, downloads, buildTasks, repositoryNotes, settings, runtime }`.

The host enriches `runtime` with app version, portable mode/data directory, GitHub/Agent status and `proxyPasswordConfigured`; secret values are excluded.

Settings add `startupPage` (one of the six workspace pages), `startupGithubTab` (`explore|owned|starred|activity|feed`), `navigationOrder` (all six pages exactly once), and `proxy: { mode:'system'|'manual'|'direct', url, username }`. Settings also add `colorScheme: blue|teal|violet|amber`, independent of light/dark/system mode. Older settings gain defaults. Startup preferences are applied once when the application opens; changing them does not interrupt the current page. Sidebar order supports drag/drop and Alt+Up/Down; Settings remains fixed.

`RepositoryNote` stores `{ repository, name, description, updatedAt }` in a separate SQLite table. Repository keys are lowercase `owner/repo`; local names and descriptions are single-line, limited to 80 and 240 characters. Empty name plus description removes the note. A note does not require a subscription and never changes the upstream repository.

Projects add `managementMode: software|files` (old records default software), and `fileStatus: notDownloaded|downloaded|updateAvailable|missing`. Assets expose separate `downloadable` and `installable` booleans; legacy `compatible` retains the software recommendation meaning. Download records persist `installable` so a ZIP saved in file mode never gains installation actions. Add/update/import/export preserve mode. File status is derived from matching completed download records and actual paths, not installedVersion; multiple files from one release are supported. SQLite migrations are transactional and refuse unknown future schema versions. Schema 4 adds executablePath, backups and install_journals. Download status `deleted` keeps history after managed package deletion; it does not erase installedVersion or backups. External original software directories are not portable data.

## Download connections and external handoff

Settings add `downloadConnections` (1..8, default 4), `useExternalDownloader` (default false), and `externalDownloader: { name, endpoint, requestTemplate }`. Older settings receive defaults. Subscription export carries connection count but excludes the machine-local external interface configuration. Internal downloads of at least 4 MiB probe a byte range and use validated segments only with stable identity, exact range/length and trusted URLs; unsupported servers fall back to one stream. Publication never overwrites an existing file and validates the merged SHA-256.

Manual external handoff requires confirmation and sends public release metadata via a loopback HTTP JSON POST, without credentials or proxy settings. The AB preset uses port 15151 and `/start-headless-download`. Templates support `{url}`, `{pageUrl}`, `{fileName}`, `{projectName}` and `{tag}`. The record stays `external`, with no invented progress or completion. Ambiguous receipt failures require checking the downloader before confirmed retry. `import_external_download` uses a native picker, verifies exact size and any publisher digest, and copies into the owned download directory before enabling installation. The source remains intact. Automatic download continues to use the internal downloader.

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

Downloads use `downloadConnections` (1..8, default 4) independently of concurrent task count. Files >=4MiB probe a byte range and stable strong ETag/Last-Modified; exact 206 headers, identity encoding and trusted final URLs are required. Unsupported ranges fall back to one stream. Segments drain on failure/cancellation before cleanup and are merged with SHA-256 before no-overwrite publication.

External downloader settings are `useExternalDownloader` (default false) and `externalDownloader: { name, endpoint, requestTemplate }`. Only loopback HTTP JSON POST is allowed, with no proxy, redirects, cookies or OAuth headers. The AB preset uses `127.0.0.1:15151/start-headless-download`; tokens are substituted as JSON string data without recursive expansion. `external` records do not imply completion: explicit file import validates exact size, known upstream digest, owned destination and no symlink/reparse ancestors before atomically publishing. Automatic downloads stay internal. The external configuration is excluded from portable JSON library export.

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
| send_external_download | { projectId, assetId, confirmed: true } | DownloadRecord; reserved external state, deduplicated, public metadata only |
| retry_external_download | { id, confirmed: true } | DownloadRecord; explicit resend after receipt check |
| import_external_download | { id, confirmed: true } | DownloadRecord|null; host file picker, exact size/digest copy into owned download directory |
| test_external_downloader | { config: ExternalDownloader } | { ok, message }; TCP reachability only, not API acceptance |
| retry_download | { id } | DownloadRecord |
| cancel_download | { id } | null |
| preview_install | { id, targetPath?: string } | InstallPreview; read-only target/package/space/root checks |
| install_download | { id, confirmed: true, targetPath, archiveRoot?: string, previousVersion?: string|null } | InstallResult; target must match validation; ZIP root explicitly selected |
| get_backup | { projectId } | BackupRecord|null; latest backup or interrupted recovery |
| restore_backup | { projectId, confirmed: true } | Project; restore complete file state and recorded version/path |
| delete_backup | { projectId, confirmed: true } | null; only idle completed backups |
| delete_download | { id, confirmed: true } | null; delete managed package, retain history/software/backup |
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

Asset selection considers Windows/x64, installer/ZIP preference, debug/symbol/source/checksum exclusions and ambiguity. Preserve every asset but mark compatibility/reason. If confidence is insufficient, show choices rather than auto-download. Do not recommend explicit ARM/macOS/Linux packages for x64 installation. File-only downloads are independent of software compatibility; all candidates retain HTTPS/size/path validation. Case-insensitive names and filenames without platform tags need sensible scores, especially the three requested sample repositories.

Files mode never chooses all attachments automatically and never installs, extracts, launches or marks software installed. Switching modes revokes old installation capabilities; retry permits a missing completed download without overwriting existing files.

Downloads: single-file maximum 4 GiB. HTTPS GitHub release domains only; reject userinfo/nonstandard ports/untrusted redirects. Never send GitHub API authorization headers to artifact hosts. Stream to a .part file, enforce finite sizes, compute SHA-256, compare an upstream sha256 digest if supplied, and only then rename as completed. A computed hash without an upstream digest is not publisher verification. Sanitize names and do not overwrite an existing completed asset.

Install: explicit confirmation is required after a read-only preview of the dedicated original software directory, complete backup size, disk space and archive roots. ZIP extraction checks enclosed paths, links/reparse points, file count and expanded size; never guess a wrapper directory. Overlay the explicitly selected package root onto the original directory: replace same-name files (including configuration), retain files absent from the package. A durable install journal and full pre-update file snapshot in `data/backups/<project-id>/<run-id>` precede writes; interrupted/failed operations restore the snapshot or remain visibly recovery-required. Preserve the prior completed backup until the new transaction succeeds, then retain only the latest per project. Restore/deletion require separate confirmation. Old managed installs can migrate only to an explicitly chosen external empty dedicated directory; retain the old location and commit the new tracked path only on success. Existing managed-version path switching remains compatibility-only.

EXE/MSI opens the author's official wizard only after a file backup; the user must select the same directory and record the completed version. File restore cannot undo registry, services or all installer changes. Download completion or opening a wizard is never proof of installation. Development fixtures never update real installed user software. Agent tasks cannot invoke this write/launch flow.

## Host-only actions (lead implements)

| action | payload | result |
| --- | --- | --- |
| pick_directory | {} | string or null (native chooser) |
| pick_project_executable | { projectId } | Project|null; native EXE picker, validate inside recorded install directory without links/reparse points |
| open_project | { projectId } | null; revalidate saved executable, spawn directly with its directory, no shell or arguments |
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
| test_proxy | { proxy?: ProxySettings, password?: string } | { ok, mode, source, target, status, latencyMs, message, warning?, retryAt? }; tests unsaved input without saving |
| github_status / github_cancel_login / github_logout | {} | GithubStatus; logout disconnects this application only |
| github_login | { lists?: boolean, privateRepositories?: boolean } | GithubStatus; explicit optional user/repo scope upgrades |
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
| github_cache_read | { operation, parameters } | null or { data, updatedAt, fresh, sessionRevision }; fixed six reads, no network |
| github_cache_stats / github_cache_clear | {} | combined API/feed { bytes, entries } / null; machine-local only |
| feed_status / feed_connect / feed_refresh / feed_more / feed_disconnect | {} | GithubFeedPage; independent website session, requestId/sessionRevision reject stale results |

`save_settings` is intercepted by the host to start/stop/rebind the Agent server safely; on failure preserve previous enabled settings and report the error. `snapshot` is enriched by the host. The browser-only development preview must be clearly marked and never pretend real operations succeeded. Production must never silently fall back to demo data.

## GitHub authorization and fixed operations

`src-tauri/src/github.rs` owns `GithubService`; `github_oauth.rs` implements ReleaseHub's OAuth Device Flow directly, without a client secret or server. The public build-time `RELEASEHUB_GITHUB_CLIENT_ID` identifies the project-owned app. Basic scopes are `read:user public_repo offline_access`; `repo` (private repositories) and `user` (Lists) require explicit additional consent. GitHub OAuth scopes include broad write powers; the UI explains this before escalation. Old CLI users authorize once again. The distribution retains the independent MCP executable, not gh.exe.

`SecretStore` stores the complete versioned token/refresh/account record only in the per-instance Windows credential namespace. No environment-token borrowing or plaintext fallback. Refresh rotation is serialized, persisted before use, and dirty rotated credentials remain in memory on a transient keyring write failure instead of replaying the old refresh token. Status returns cached identity immediately with `verified:false`; background `/user` verification or authoritative mutations establish identity. Disconnect deletes this application's record and protected caches, without logging other tools out or claiming upstream grant revocation.

REST and GraphQL use fixed compiled operations, bounded replies, no redirects, 4 concurrent requests and per-account generation guards. Mutations serialize, verify current identity, require confirmation, read back and never automatically retry. Matching reads coalesce; memory-only REST ETags reduce repeat payloads. Core's internal `repository_api(owner,repo,tail,page)` shares the authorization for allowlisted metadata/releases/commits/compare GETs. Download transport never receives authorization.

## Proxy policy and desktop shell

One shared `Arc<ProxyState>` builds the HTTP clients for metadata, update checks and downloads, and supplies a reusable HTTPS/no-redirect GitHub client pool. Every call resolves current proxy policy; policy or password changes invalidate the pool, without storing default authorization headers. System mode uses the Windows static proxy, then environment settings when applicable; direct mode disables proxies. Manual mode accepts HTTP/HTTPS and SOCKS5/SOCKS5h with separate account fields. Loopback connections always bypass the proxy. WebView2 uses its own system/PAC support in system mode; manual authenticated webpage proxies require a system or local forwarding proxy.

Proxy passwords are stored in the Windows credential manager as an account record bound to the exact URL and username. They are not saved in SQLite, snapshots, logs or export files. The host validates first, persists settings under its serial lock, rolls back on a failed save, and rebuilds the independent website window after a change. Existing downloads retain their current connection until cancelled/retried.

The proxy test uses the unauthenticated `https://api.github.com/rate_limit` endpoint. Successful connectivity and exhausted GitHub quota are separate outcomes: `ok` stays true for a confirmed quota limit, with `warning` and optional ISO `retryAt`. Ordinary access denial, proxy authentication failure and server failures remain errors.

The main window disables native decorations and uses the shared Slate/Cobalt theme for a single 52px title/search bar. Window controls use Tauri's scoped minimize, maximize, close, drag and theme APIs; close-to-tray remains a main-window behavior. Native file-drop handling is disabled so HTML sidebar dragging works. The application icon and sidebar mark use the same blue cube asset. Windows startup sends an item-specific shell notification for the current executable so in-place portable upgrades refresh its cached icon.

## GitHub Lists and homepage feed

The GitHub workspace mounts hidden after startup and preloads connected-account views with bounded concurrency; it remains mounted but hidden when another workspace page is selected. Its dedicated scroll container retains the current tab, category, loaded cursor pages, filter, detail and scroll position without refetching on navigation. Each GitHub tab has independent query, pagination, filter, selection and scroll state. `github_cache_read` returns account/scope/parameter-bound projections from machine-local disk before any network wait. Current-session fresh entries skip refetch; disk-restored entries are shown while background validation runs once. TTLs: search 10min, owned/Stars/Lists 5min, detail/README 60min, activity 2min. Failures preserve visible data and timestamp. Disk API cache is 98 MiB / 30 days plus a separate 2 MiB feed budget; corrupt/expired entries are misses. Clear invalidates in-flight persistence. Explicit refresh retains old content until a result arrives; account changes and logout clear account-scoped state and invalidate in-flight results. Login and feed status polling pause while the workspace is hidden and resume without clearing displayed content.

Stars results, hover/focus and detail views preload category membership through one two-worker frontend queue. Opening the assignment editor accepts fresh account-bound cache entries, coalesces pending reads, and preserves user checkbox edits during revalidation. Backend membership and unlisted views share a complete catalog/membership snapshot for 60 seconds, keyed by account, session revision and cache epoch. Mutation and cache clearing invalidate it; explicit category refresh reads the live catalog and invalidates a changed snapshot. Mutation before/after verification remains live.

Stars classification uses fixed official GraphQL queries for `User.lists`, `UserList.items`, `createUserList`, `updateUserList`, `deleteUserList`, and `updateUserListsForItem`. The last mutation replaces all membership: add/remove first read every list and membership cursor, merge the requested change, then verify the result. All/unlisted Stars use cursor pagination. List URLs are constructed from the authenticated login and list slug. Names/descriptions shown as local notes remain local.

Homepage feed uses a separate GitHub website window and a machine-local profile beneath the application instance account root. The user logs in there explicitly. `https://github.com/dashboard-feed` is read from the rendered page in its original order and presented as plain text/cards in the application; Events/RSS are not substituted. Content is bounded to 160 items and 2 MiB, links are allowlisted, and changed/unrecognized page structures produce an actionable error. Sanitized cards also persist in one bounded, machine-local profile/account-bound file. Restoration is marked cached, not a verified website login; the UI starts background revalidation and retains cards on temporary failures. Website session changes discard old cards; disconnect rotates and clears only this module's profile.

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

Presence of `portable.flag` next to ReleaseHub.exe selects `data/` beside the executable; installed mode uses the platform app-data directory. A non-writable portable directory fails clearly instead of silently losing portability. Credentials are separate. Import/export migrates subscriptions, repository notes and portable preferences, while copying the portable folder migrates full non-secret local history and managed apps. Webpage logins, account-bound GitHub caches and proxy passwords remain machine-local.

Final distribution: Windows x64 EXE installer, portable ZIP (exe + portable.flag + quick-start guide). WebView2 is required; use the shared installed runtime and give an actionable missing-runtime explanation. No bundled private endpoints or developer credentials. Final artifact build is a manual `workflow_dispatch` after implementation and two concrete improvement rounds. Public CI permissions and artifact contents are explicitly reviewed.
