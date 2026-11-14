//! Deliberately narrow local workboard: no shell, file, credential or install API.
use crate::{core::Core, secrets::random_token};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const CLAIM_LIFETIME: Duration = Duration::from_secs(2 * 60 * 60);
const MAX_REQUESTS_PER_MINUTE: u32 = 120;

struct ClaimTicket {
    token: String,
    created: Instant,
}

#[derive(Clone)]
struct AgentState {
    core: Arc<Core>,
    token: Arc<RwLock<String>>,
    claims: Arc<Mutex<HashMap<String, ClaimTicket>>>,
    rate: Arc<Mutex<(Instant, u32)>>,
    checking: Arc<AtomicBool>,
    port: u16,
}

pub struct RunningAgent {
    pub address: String,
    pub port: u16,
    state: AgentState,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl RunningAgent {
    pub fn rotate_token(&self, token: String) -> Result<(), String> {
        *self.state.token.write().map_err(|_| "接口密钥暂时不可用。")? = token;
        self.state.claims.lock().map_err(|_| "任务票据暂时不可用。")?.clear();
        Ok(())
    }

    pub async fn stop(mut self) {
        // Revoke any still-connected request's old key and claim tickets before draining.
        let _ = self.rotate_token(random_token());
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if tokio::time::timeout(Duration::from_secs(2), &mut self.task).await.is_err() {
            self.task.abort();
        }
    }
}

pub async fn start(core: Arc<Core>, port: u16, token: String) -> Result<RunningAgent, String> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("本机接口密钥格式不正确，请重新生成。".into());
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await.map_err(|_| "该本机端口已被占用，请在设置中更换端口。")?;
    let actual_port = listener.local_addr().map_err(|_| "无法读取本机接口地址。")?.port();
    let state = AgentState {
        core,
        token: Arc::new(RwLock::new(token)),
        claims: Arc::new(Mutex::new(HashMap::new())),
        rate: Arc::new(Mutex::new((Instant::now(), 0))),
        checking: Arc::new(AtomicBool::new(false)),
        port: actual_port,
    };
    let router = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/projects", get(projects))
        .route("/v1/check-updates", post(check_updates))
        .route("/v1/build-tasks", get(tasks))
        .route("/v1/build-tasks/{id}/claim", post(claim))
        .route("/v1/build-tasks/{id}/progress", post(progress))
        .route("/v1/build-tasks/{id}/result", post(result))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state.clone());
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        // Intentionally no request/header/body logging.
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async { let _ = receiver.await; })
            .await;
    });
    Ok(RunningAgent {
        address: format!("http://127.0.0.1:{actual_port}"),
        port: actual_port,
        state,
        shutdown: Some(shutdown),
        task,
    })
}

struct ApiError(StatusCode, String);
impl ApiError {
    fn invalid(message: impl Into<String>) -> Self { Self(StatusCode::BAD_REQUEST, message.into()) }
    fn conflict(message: impl Into<String>) -> Self { Self(StatusCode::CONFLICT, message.into()) }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response { (self.0, Json(json!({"error": self.1}))).into_response() }
}

fn secret_matches(expected: &str, supplied: &str) -> bool {
    expected.len() == supplied.len() && bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
}

async fn guard(State(state): State<AgentState>, request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok()).unwrap_or("");
    let expected = format!("127.0.0.1:{}", state.port);
    let local_name = format!("localhost:{}", state.port);
    if headers.contains_key(header::ORIGIN) || (host != expected && host != local_name) {
        return ApiError(StatusCode::FORBIDDEN, "仅允许直接访问本机接口，不能从网页跨站调用。".into()).into_response();
    }
    let supplied = headers.get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    let authorized = state.token.read().map(|token| secret_matches(&token, supplied)).unwrap_or(false);
    if !authorized {
        return ApiError(StatusCode::UNAUTHORIZED, "需要有效的本机访问密钥。".into()).into_response();
    }
    let permitted = if let Ok(mut rate) = state.rate.lock() {
        if rate.0.elapsed() >= Duration::from_secs(60) { *rate = (Instant::now(), 0); }
        rate.1 += 1;
        rate.1 <= MAX_REQUESTS_PER_MINUTE
    } else { false };
    if !permitted {
        return ApiError(StatusCode::TOO_MANY_REQUESTS, "请求过于频繁，请稍后再试。".into()).into_response();
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response.headers_mut().insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}

async fn health() -> Json<Value> {
    Json(json!({"name": "ReleaseHub", "version": env!("CARGO_PKG_VERSION"), "scope": "local-build-workboard", "installApi": false}))
}

async fn projects(State(state): State<AgentState>) -> Result<Json<Value>, ApiError> {
    let snapshot = state.core.handle("snapshot", json!({})).await.map_err(ApiError::invalid)?;
    let rows: Vec<Value> = snapshot["projects"].as_array().into_iter().flatten().map(|project| {
        json!({
            "id": project["id"], "name": project["name"], "repository": project["url"],
            "channel": project["channel"], "latestTag": project["release"]["tag"],
            "status": project["status"], "checkedAt": project["checkedAt"],
            "sourceHeadSha": project["sourceHeadSha"]
        })
    }).collect();
    Ok(Json(json!(rows)))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckInput { project_ids: Option<Vec<String>> }

struct CheckingGuard(Arc<AtomicBool>);
impl Drop for CheckingGuard { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }

async fn check_updates(State(state): State<AgentState>, Json(input): Json<CheckInput>) -> Result<Json<Value>, ApiError> {
    if state.checking.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err(ApiError::conflict("已有检查正在进行。"));
    }
    let _checking = CheckingGuard(state.checking.clone());
    let mut payload = json!({"allowAutoDownload": false});
    if let Some(ids) = input.project_ids {
        if ids.len() > 200 { return Err(ApiError::invalid("一次最多检查 200 个项目。")); }
        payload["projectIds"] = json!(ids);
    }
    Ok(Json(state.core.handle("check_updates", payload).await.map_err(ApiError::invalid)?))
}

async fn tasks(State(state): State<AgentState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.core.handle("list_build_tasks", json!({})).await.map_err(ApiError::invalid)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimInput { agent_name: String }

async fn claim(State(state): State<AgentState>, Path(id): Path<String>, Json(input): Json<ClaimInput>) -> Result<Json<Value>, ApiError> {
    if input.agent_name.trim().is_empty() || input.agent_name.len() > 100 {
        return Err(ApiError::invalid("请提供不超过 100 字符的 Agent 名称。"));
    }
    let task = state.core.handle("claim_build_task", json!({"id": id, "agentName": input.agent_name}))
        .await.map_err(ApiError::conflict)?;
    let token = random_token();
    let mut claims = state.claims.lock().map_err(|_| ApiError::conflict("任务票据暂时不可用。"))?;
    claims.retain(|_, claim| claim.created.elapsed() < CLAIM_LIFETIME);
    claims.insert(id, ClaimTicket { token: token.clone(), created: Instant::now() });
    Ok(Json(json!({"task": task, "claimToken": token, "expiresInSeconds": CLAIM_LIFETIME.as_secs()})))
}

fn verify_claim(state: &AgentState, id: &str, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = headers.get("x-releasehub-claim").and_then(|value| value.to_str().ok()).unwrap_or("");
    let claims = state.claims.lock().map_err(|_| ApiError::conflict("任务票据暂时不可用。"))?;
    let valid = claims.get(id).map(|claim| claim.created.elapsed() < CLAIM_LIFETIME && secret_matches(&claim.token, supplied)).unwrap_or(false);
    if valid { Ok(()) } else { Err(ApiError(StatusCode::FORBIDDEN, "任务票据无效或已过期，请在应用中取消旧任务后重新创建。".into())) }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProgressStatus { Running, Failed }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgressInput { status: ProgressStatus, progress: Option<String>, error: Option<String> }

async fn progress(State(state): State<AgentState>, Path(id): Path<String>, headers: HeaderMap, Json(input): Json<ProgressInput>) -> Result<Json<Value>, ApiError> {
    verify_claim(&state, &id, &headers)?;
    if input.progress.as_ref().map_or(false, |text| text.len() > 8000) || input.error.as_ref().map_or(false, |text| text.len() > 8000) {
        return Err(ApiError::invalid("进度说明过长，请只报告简短摘要。"));
    }
    let failed = matches!(input.status, ProgressStatus::Failed);
    let status = if failed { "failed" } else { "running" };
    let task = state.core.handle("update_build_task", json!({"id": id, "status": status, "progress": input.progress, "error": input.error}))
        .await.map_err(ApiError::conflict)?;
    if failed { if let Ok(mut claims) = state.claims.lock() { claims.remove(&id); } }
    Ok(Json(task))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultInput {
    artifact_url: String,
    source_commit: String,
    artifact_sha256: Option<String>,
    progress: Option<String>,
}

fn is_hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_artifact_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else { return false };
    if url.scheme() != "https" || url.host_str() != Some("github.com") || !url.username().is_empty()
        || url.password().is_some() || url.port().is_some() || url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    parts.len() >= 4 && !parts[0].is_empty() && !parts[1].is_empty()
        && (parts[2] == "releases" || (parts[2] == "actions" && parts[3] == "runs" && parts.len() >= 5))
}

async fn result(State(state): State<AgentState>, Path(id): Path<String>, headers: HeaderMap, Json(input): Json<ResultInput>) -> Result<Json<Value>, ApiError> {
    verify_claim(&state, &id, &headers)?;
    if !validate_artifact_url(&input.artifact_url) { return Err(ApiError::invalid("构建结果必须是公开的 GitHub 发行页、发行资产或 Actions 运行地址。")); }
    if !is_hex(&input.source_commit, &[40, 64]) { return Err(ApiError::invalid("请报告本次实际构建的完整源码提交号。")); }
    if input.artifact_sha256.as_ref().map_or(false, |hash| !is_hex(hash, &[64])) {
        return Err(ApiError::invalid("构建产物 SHA-256 格式不正确。"));
    }
    if input.progress.as_ref().map_or(false, |text| text.len() > 8000) { return Err(ApiError::invalid("结果摘要过长。")); }
    let all = state.core.handle("list_build_tasks", json!({})).await.map_err(ApiError::invalid)?;
    let task = all.as_array().and_then(|tasks| tasks.iter().find(|task| task["id"].as_str() == Some(&id)))
        .ok_or_else(|| ApiError::invalid("任务不存在。"))?;
    if let Some(expected) = task["sourceCommit"].as_str().filter(|sha| !sha.is_empty()) {
        if !expected.eq_ignore_ascii_case(&input.source_commit) {
            return Err(ApiError::conflict("结果对应的源码与任务锁定的提交不同，请为新提交创建任务。"));
        }
    }
    let task = state.core.handle("update_build_task", json!({
        "id": id, "status": "succeeded", "artifactUrl": input.artifact_url,
        "artifactSha256": input.artifact_sha256, "sourceCommit": input.source_commit,
        "progress": input.progress.unwrap_or_else(|| "Agent 已报告构建完成；这是非上游官方构建。".into())
    })).await.map_err(ApiError::conflict)?;
    if let Ok(mut claims) = state.claims.lock() { claims.remove(&id); }
    Ok(Json(task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_cannot_point_to_private_services_or_injected_urls() {
        assert!(validate_artifact_url("https://github.com/example/app/releases/download/v1/app.exe"));
        assert!(validate_artifact_url("https://github.com/example/app/actions/runs/123"));
        for value in ["http://127.0.0.1/private", "https://github.com.evil.test/a/b/releases/v1", "https://user@github.com/a/b/releases/v1", "https://github.com/a/b/issues/1", "https://github.com/a/b/releases/v1?secret=value", "javascript:alert(1)"] {
            assert!(!validate_artifact_url(value));
        }
    }

    #[test]
    fn authentication_compares_complete_values() {
        assert!(secret_matches("abcdef", "abcdef"));
        assert!(!secret_matches("abcdef", "abc"));
        assert!(!secret_matches("abcdef", "abcdeg"));
    }

    #[tokio::test]
    async fn server_rejects_missing_auth_origins_and_generic_commands() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::new(directory.path().to_path_buf(), true, Arc::new(|_, _| {})).unwrap();
        let token = random_token();
        let server = start(core, 0, token.clone()).await.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let health = format!("{}/v1/health", server.address);
        assert_eq!(client.get(&health).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(client.get(&health).bearer_auth(&token).send().await.unwrap().status(), StatusCode::OK);
        assert_eq!(client.get(&health).bearer_auth(&token).header("Origin", "https://example.test").send().await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(client.get(&health).bearer_auth(&token).header("Host", "attacker.test").send().await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(client.post(format!("{}/v1/dispatch", server.address)).bearer_auth(&token).json(&json!({"action":"install_download"})).send().await.unwrap().status(), StatusCode::NOT_FOUND);
        assert_eq!(client.post(format!("{}/v1/check-updates", server.address)).bearer_auth(&token).json(&json!({"allowAutoDownload":true})).send().await.unwrap().status(), StatusCode::UNPROCESSABLE_ENTITY);
        let next_token = random_token();
        server.state.claims.lock().unwrap().insert("test-task".into(), ClaimTicket {
            token: random_token(), created: Instant::now(),
        });
        server.rotate_token(next_token.clone()).unwrap();
        assert!(server.state.claims.lock().unwrap().is_empty());
        assert_eq!(client.get(&health).bearer_auth(&token).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(client.get(&health).bearer_auth(&next_token).send().await.unwrap().status(), StatusCode::OK);
        let held_state = server.state.clone();
        server.stop().await;
        assert!(!secret_matches(&held_state.token.read().unwrap(), &next_token));
        assert!(held_state.claims.lock().unwrap().is_empty());
    }
}
