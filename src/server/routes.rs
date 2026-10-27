//! HTTP 路由与中间件（Key 鉴权、客户端限流、CORS）。
//!
//! BYOK：所有 `/api/v1` 端点要求微信读书 API Key——`X-Weread-Key` 请求头
//! 或 `?key=` 查询参数（EventSource / 下载直链场景）。服务器不持有任何默认 Key。

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde_json::{Value, json};

use super::error::{AppError, AppResult};
use super::service::{self, TERMINAL_STATUSES, TaskView};
use super::state::AppState;

/// 中间件解析出的请求级 API Key
#[derive(Clone)]
pub struct WereadKey(pub String);

pub fn build_app(state: AppState) -> Router {
    let api = Router::new()
        // 同步代理
        .route("/shelf", get(get_shelf))
        .route("/notebooks", get(get_notebooks))
        .route("/books/{book_id}", get(get_book_info))
        .route("/books/{book_id}/chapters", get(get_book_chapters))
        .route("/books/{book_id}/progress", get(get_book_progress))
        .route("/books/{book_id}/notes", get(get_book_notes))
        .route("/books/{book_id}/notes/markdown", get(get_book_markdown))
        .route("/readdata", get(get_readdata))
        .route("/search", get(get_search))
        .route("/gateway/apis", get(get_gateway_apis))
        .route("/gateway/call", post(post_gateway_call))
        // 导出任务
        .route("/exports", get(list_exports).post(create_export))
        .route("/exports/{task_id}", get(get_export).delete(delete_export))
        .route("/exports/{task_id}/result", get(get_result))
        .route("/exports/{task_id}/events", get(get_export_events))
        // 系统
        .route("/server/info", get(server_info))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            key_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .with_state(state);

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
}

/// 在基础 router 上按配置叠加 CORS（origins 为空则不加）。
pub fn build_app_with_cors(state: AppState, cors_origins: &str) -> Router {
    let app = build_app(state);
    if cors_origins.trim().is_empty() {
        return app;
    }
    use tower_http::cors::CorsLayer;
    let origins: Vec<_> = cors_origins
        .split(',')
        .filter_map(|o| o.trim().parse().ok())
        .collect();
    if origins.is_empty() {
        return app;
    }
    app.layer(CorsLayer::new().allow_origin(origins))
}

// --------------------------------------------------------------------------- //
// 中间件
// --------------------------------------------------------------------------- //

async fn key_middleware(
    State(_state): State<AppState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    // 头优先；EventSource / <a download> 无法设头，允许 ?key= 查询参数兜底
    let key = req
        .headers()
        .get("x-weread-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            req.uri()
                .query()
                .and_then(|q| {
                    q.split('&').find_map(|kv| {
                        kv.split_once('=').and_then(|(k, v)| {
                            (k == "key").then(|| urldecode(v))
                        })
                    })
                })
                .map(|v| v.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    let key = key.ok_or_else(|| {
        AppError::Unauthorized(
            "缺少微信读书 API Key（X-Weread-Key 头或 ?key= 参数）。获取页面: https://weread.qq.com/r/weread-skills".to_string(),
        )
    })?;
    req.extensions_mut().insert(WereadKey(key));
    Ok(next.run(req).await)
}

/// 极简百分号解码（Key 一般为 URL 安全字符，覆盖 %+XX 即可）
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], decoded) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    // ConnectInfo 由 into_make_service_with_connect_info 注入请求扩展；
    // 缺失时（如测试 oneshot）回退 loopback
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.ip())
        .unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
    if !state.rate_allow(ip) {
        return Err(AppError::TooManyRequests);
    }
    Ok(next.run(req).await)
}

// --------------------------------------------------------------------------- //
// 基础端点
// --------------------------------------------------------------------------- //

async fn healthz() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn server_info(State(state): State<AppState>) -> AppResult<Json<Value>> {
    Ok(Json(service::server_info(&state).await?))
}

#[derive(serde::Deserialize, Default)]
struct RefreshQuery {
    refresh: Option<bool>,
}

fn refresh_of(q: &RefreshQuery) -> bool {
    q.refresh.unwrap_or(false)
}

// --------------------------------------------------------------------------- //
// 同步代理端点
// --------------------------------------------------------------------------- //

async fn get_shelf(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_shelf(&state, gw.as_ref(), &key, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

async fn get_notebooks(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_notebooks(&state, gw.as_ref(), &key, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

async fn get_book_info(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(book_id): Path<String>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_book_info(&state, gw.as_ref(), &key, &book_id, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

async fn get_book_chapters(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(book_id): Path<String>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_book_chapters(&state, gw.as_ref(), &key, &book_id, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

async fn get_book_progress(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(book_id): Path<String>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_book_progress(&state, gw.as_ref(), &key, &book_id, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

async fn get_book_notes(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(book_id): Path<String>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_book_notes(&state, gw.as_ref(), &key, &book_id, refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

#[derive(serde::Deserialize, Default)]
struct MarkdownQuery {
    download: Option<bool>,
}

/// RFC 5987 percent-encode（filename* 用，保留 ASCII 字母数字与少数安全字符）
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn get_book_markdown(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(book_id): Path<String>,
    Query(q): Query<MarkdownQuery>,
) -> AppResult<Response> {
    let file = service::get_markdown(&state, &key, &book_id).await?;
    let content_type: axum::http::HeaderValue = "text/markdown; charset=utf-8".parse().unwrap();
    if q.download.unwrap_or(false) {
        let ascii_name = format!("weread-notes-{book_id}.md");
        let encoded = pct_encode(&format!("{}.md", file.title));
        let disposition: axum::http::HeaderValue = format!(
            "attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}"
        )
        .parse()
        .unwrap();
        return Ok((
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_DISPOSITION, disposition),
            ],
            file.markdown,
        )
            .into_response());
    }
    Ok(([(header::CONTENT_TYPE, content_type)], file.markdown).into_response())
}

#[derive(serde::Deserialize)]
struct ReaddataQuery {
    mode: Option<String>,
    #[serde(rename = "baseTime")]
    base_time: Option<i64>,
    refresh: Option<bool>,
}

async fn get_readdata(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<ReaddataQuery>,
) -> AppResult<Json<Value>> {
    let mode = q.mode.unwrap_or_else(|| "overall".to_string());
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_readdata(
        &state,
        gw.as_ref(),
        &key,
        &mode,
        q.base_time,
        q.refresh.unwrap_or(false),
    )
    .await?;
    Ok(Json(service::envelope(&p)))
}

#[derive(serde::Deserialize)]
struct SearchQuery {
    keyword: Option<String>,
    scope: Option<i64>,
    count: Option<i64>,
    max_idx: Option<i64>,
}

async fn get_search(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    let keyword = q
        .keyword
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| AppError::InvalidArgument("缺少 keyword 参数".to_string()))?;
    let mut params = json!({ "keyword": keyword });
    if let Some(scope) = q.scope {
        params["scope"] = json!(scope);
    }
    if let Some(count) = q.count {
        params["count"] = json!(count);
    }
    if let Some(max_idx) = q.max_idx {
        params["maxIdx"] = json!(max_idx);
    }
    // 搜索不做缓存（每人搜的词不同）
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let data = gw.call("/store/search", params).await?;
    Ok(Json(data))
}

async fn get_gateway_apis(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<RefreshQuery>,
) -> AppResult<Json<Value>> {
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let p = service::proxy_gateway_apis(&state, gw.as_ref(), refresh_of(&q)).await?;
    Ok(Json(service::envelope(&p)))
}

#[derive(serde::Deserialize)]
struct GatewayCallBody {
    api_name: String,
    #[serde(default)]
    params: Option<Value>,
}

/// 逃生门：透传任意网关调用（官方网关全只读，无滥用面）。
async fn post_gateway_call(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    body: Bytes,
) -> AppResult<Json<Value>> {
    let req: GatewayCallBody = serde_json::from_slice(&body)
        .map_err(|e| AppError::InvalidArgument(e.to_string()))?;
    if req.api_name.trim().is_empty() {
        return Err(AppError::InvalidArgument("api_name 不能为空".to_string()));
    }
    let params = match req.params {
        Some(v @ Value::Object(_)) => v,
        _ => json!({}),
    };
    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let data = gw.call(&req.api_name, params).await?;
    Ok(Json(data))
}

// --------------------------------------------------------------------------- //
// /exports
// --------------------------------------------------------------------------- //

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct CreateReq {
    #[serde(default)]
    book_ids: Option<Vec<String>>,
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    wait_timeout: u64,
}

/// 创建导出任务。body 手动解析以保证错误信封统一（400 而非 axum 默认 422）。
async fn create_export(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    body: Bytes,
) -> AppResult<Response> {
    let req: CreateReq =
        serde_json::from_slice(&body).map_err(|e| AppError::InvalidArgument(e.to_string()))?;
    if req.wait_timeout > 30 {
        return Err(AppError::InvalidArgument("wait_timeout 最大 30 秒".to_string()));
    }

    let gw = state
        .registry
        .for_key(&key)
        .map_err(AppError::from)?;
    let outcome = service::create_task(
        &state,
        gw.as_ref(),
        &key,
        service::CreateTaskInput {
            book_ids: req.book_ids,
            refresh: req.refresh,
        },
    )
    .await?;

    // 有限同步等待：wait_timeout 内完成则返回终态（单书导出秒级完成）
    let mut view = outcome.view;
    if req.wait_timeout > 0 && !TERMINAL_STATUSES.contains(&view.status.as_str()) {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(req.wait_timeout);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            view = service::get_task(&state, &key, &view.task_id).await?;
            if TERMINAL_STATUSES.contains(&view.status.as_str()) {
                break;
            }
        }
    }

    let mut body = serde_json::to_value(&view)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    body["skipped_cached"] = json!(outcome.skipped_cached);
    body["queued_books"] = json!(outcome.queued_books);

    let code = if TERMINAL_STATUSES.contains(&view.status.as_str()) {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok((code, Json(body)).into_response())
}

async fn get_export(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(task_id): Path<String>,
) -> AppResult<Json<TaskView>> {
    Ok(Json(service::get_task(&state, &key, &task_id).await?))
}

#[derive(serde::Deserialize, Default)]
struct ListQuery {
    status: Option<String>,
    limit: Option<i64>,
    cursor: Option<i64>,
}

async fn list_exports(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    if let Some(s) = &q.status {
        if !["queued", "running", "completed", "partial", "failed"].contains(&s.as_str()) {
            return Err(AppError::InvalidArgument(format!("非法 status 过滤值: {s}")));
        }
    }
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let (tasks, next_cursor) = service::list_tasks(
        &state,
        &key,
        service::ListFilter {
            status: q.status,
            limit,
            cursor_rowid: q.cursor,
        },
    )
    .await?;
    Ok(Json(json!({
        "tasks": tasks,
        "next_cursor": next_cursor,
        "has_more": next_cursor.is_some(),
    })))
}

async fn delete_export(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(task_id): Path<String>,
) -> AppResult<StatusCode> {
    service::delete_task(&state, &key, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_result(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(task_id): Path<String>,
) -> AppResult<Json<service::ResultView>> {
    Ok(Json(service::task_result(&state, &key, &task_id).await?))
}

// --------------------------------------------------------------------------- //
// SSE 进度推流
// --------------------------------------------------------------------------- //

fn task_event(name: &str, view: &TaskView) -> Result<Event, Infallible> {
    let data = serde_json::to_string(view).unwrap_or_else(|_| "{}".to_string());
    Ok(Event::default().event(name).data(data))
}

/// SSE 流：先推当前快照（progress），随后仅在进度变更时推送；
/// 终态推 done（partial 也归 done，内容里带 error）/ error 后关流。
async fn get_export_events(
    State(state): State<AppState>,
    Extension(WereadKey(key)): Extension<WereadKey>,
    Path(task_id): Path<String>,
) -> AppResult<
    Sse<
        axum::response::sse::KeepAliveStream<
            tokio_stream::wrappers::ReceiverStream<Result<Event, Infallible>>,
        >,
    >,
> {
    // 校验任务存在且属于该 Key（404 快速失败）；订阅必须发生在 spawn 之前，
    // 保证响应返回时已建立订阅，连接期间的通知不会丢失
    service::get_task(&state, &key, &task_id).await?;
    let mut notify = state.subscribe_progress();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);
    tokio::spawn(async move {
        loop {
            let Ok(view) = service::get_task(&state, &key, &task_id).await else { return };
            if TERMINAL_STATUSES.contains(&view.status.as_str()) {
                let name = if view.status == "failed" { "error" } else { "done" };
                let _ = tx.send(task_event(name, &view)).await;
                return;
            }
            if tx.send(task_event("progress", &view)).await.is_err() {
                return; // 客户端断开
            }
            // 等待下一次该任务的进度通知
            loop {
                match notify.recv().await {
                    Ok(id) if id == task_id => break,
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                }
            }
        }
    });

    Ok(Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default()))
}
