//! 服务层——代理缓存、任务 CRUD 与视图聚合、Markdown 读取、过期清理。

use rand::Rng;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};

use crate::client::GatewayApi;
use crate::fetch;

use super::config::ServerConfig;
use super::error::{AppError, AppResult};
use super::state::AppState;

pub const TERMINAL_STATUSES: [&str; 3] = ["completed", "partial", "failed"];

fn ms_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn new_task_id() -> String {
    let now = ServerConfig::now_ms() as u64;
    let rand6: u32 = rand::thread_rng().gen_range(0x100000..=0xFFFFFF);
    format!("wexp_{now:012x}{rand6:06x}")
}

// --------------------------------------------------------------------------- //
// 代理缓存
// --------------------------------------------------------------------------- //

pub struct CachedPayload {
    pub cached_at: String,
    pub fresh: bool,
    pub data: Value,
}

pub fn envelope(p: &CachedPayload) -> Value {
    json!({"cached_at": p.cached_at, "fresh": p.fresh, "data": p.data})
}

async fn cache_get(pool: &SqlitePool, key: &str) -> AppResult<Option<(String, i64, i64)>> {
    let row = sqlx::query(
        "SELECT payload, fetched_ms, expires_ms FROM cache_entries WHERE cache_key = ?",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("payload"), r.get("fetched_ms"), r.get("expires_ms"))))
}

/// 带缓存的网关调用：TTL 内直接回缓存；过期或 refresh 时回源。
/// 回源失败但存在旧缓存时降级返回过期数据（fresh=false），提升容错。
pub async fn cached_call<F, Fut>(
    state: &AppState,
    cache_key: &str,
    ttl_secs: i64,
    refresh: bool,
    fetch: F,
) -> AppResult<CachedPayload>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Value, crate::client::GatewayError>>,
{
    let now = ServerConfig::now_ms();
    if !refresh {
        if let Some((payload, fetched_ms, expires_ms)) =
            cache_get(&state.pool, cache_key).await?
        {
            if expires_ms > now {
                return Ok(CachedPayload {
                    cached_at: ms_to_rfc3339(fetched_ms),
                    fresh: true,
                    data: serde_json::from_str(&payload).unwrap_or(Value::Null),
                });
            }
        }
    }

    match fetch().await {
        Ok(data) => {
            let payload =
                serde_json::to_string(&data).map_err(|e| AppError::Internal(e.to_string()))?;
            sqlx::query(
                "INSERT INTO cache_entries (cache_key, payload, fetched_ms, expires_ms)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(cache_key) DO UPDATE SET
                    payload=excluded.payload, fetched_ms=excluded.fetched_ms,
                    expires_ms=excluded.expires_ms",
            )
            .bind(cache_key)
            .bind(&payload)
            .bind(now)
            .bind(now + ttl_secs * 1000)
            .execute(&state.pool)
            .await?;
            Ok(CachedPayload {
                cached_at: ms_to_rfc3339(now),
                fresh: true,
                data,
            })
        }
        Err(e) => {
            if let Some((payload, fetched_ms, _)) = cache_get(&state.pool, cache_key).await? {
                tracing::warn!(cache_key, error = %e, "回源失败，降级返回过期缓存");
                return Ok(CachedPayload {
                    cached_at: ms_to_rfc3339(fetched_ms),
                    fresh: false,
                    data: serde_json::from_str(&payload).unwrap_or(Value::Null),
                });
            }
            Err(e.into())
        }
    }
}

// --------------------------------------------------------------------------- //
// books 表顺手入库（best-effort，失败只记日志）
// --------------------------------------------------------------------------- //

fn field_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

async fn upsert_books_from_shelf(pool: &SqlitePool, data: &Value) -> anyhow::Result<()> {
    let Some(books) = data.get("books").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    let now = ServerConfig::now_ms();
    for b in books {
        let book_id = fetch::id_str(b.get("bookId"));
        if book_id == "?" {
            continue;
        }
        let title = b.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let author = b.get("author").and_then(|t| t.as_str()).unwrap_or("");
        let cover = b.get("cover").and_then(|t| t.as_str()).unwrap_or("");
        sqlx::query(
            "INSERT INTO books (book_id, title, author, cover, progress, finish_reading,
                                read_update_time, updated_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(book_id) DO UPDATE SET
                title=excluded.title, author=excluded.author, cover=excluded.cover,
                progress=COALESCE(excluded.progress, books.progress),
                finish_reading=excluded.finish_reading,
                read_update_time=excluded.read_update_time, updated_ms=excluded.updated_ms",
        )
        .bind(&book_id)
        .bind(title)
        .bind(author)
        .bind(cover)
        .bind(field_i64(b, "progress").or_else(|| field_i64(b, "readProgress")))
        .bind(field_i64(b, "finishReading").unwrap_or(0))
        .bind(field_i64(b, "readUpdateTime"))
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn upsert_books_from_notebooks(pool: &SqlitePool, books: &[Value]) -> anyhow::Result<()> {
    let now = ServerConfig::now_ms();
    for nb in books {
        let book_id = fetch::id_str(nb.get("bookId"));
        if book_id == "?" {
            continue;
        }
        let book = nb.get("book").cloned().unwrap_or(json!({}));
        let title = book.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let author = book.get("author").and_then(|t| t.as_str()).unwrap_or("");
        let cover = book.get("cover").and_then(|t| t.as_str()).unwrap_or("");
        sqlx::query(
            "INSERT INTO books (book_id, title, author, cover, bookmark_count, note_count,
                                review_count, read_update_time, updated_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(book_id) DO UPDATE SET
                title=CASE WHEN excluded.title != '' THEN excluded.title ELSE books.title END,
                author=excluded.author, cover=excluded.cover,
                bookmark_count=excluded.bookmark_count, note_count=excluded.note_count,
                review_count=excluded.review_count, updated_ms=excluded.updated_ms",
        )
        .bind(&book_id)
        .bind(title)
        .bind(author)
        .bind(cover)
        .bind(field_i64(nb, "bookmarkCount"))
        .bind(field_i64(nb, "noteCount"))
        .bind(field_i64(nb, "reviewCount"))
        .bind(field_i64(nb, "updateTime"))
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn upsert_best_effort(fut: impl std::future::Future<Output = anyhow::Result<()>>) {
    if let Err(e) = fut.await {
        tracing::warn!(error = %e, "books 表入库失败（不影响代理响应）");
    }
}

// --------------------------------------------------------------------------- //
// 同步代理端点
// --------------------------------------------------------------------------- //

pub async fn proxy_shelf<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    refresh: bool,
) -> AppResult<CachedPayload> {
    let ttl = state.config.cache_ttl_secs;
    let pool = state.pool.clone();
    cached_call(state, "shelf/sync", ttl, refresh, || async move {
        let data = gw.call("/shelf/sync", json!({})).await?;
        upsert_best_effort(upsert_books_from_shelf(&pool, &data)).await;
        Ok(data)
    })
    .await
}

pub async fn proxy_notebooks<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    refresh: bool,
) -> AppResult<CachedPayload> {
    let ttl = state.config.cache_ttl_secs;
    let pool = state.pool.clone();
    cached_call(state, "notebooks", ttl, refresh, || async move {
        let result = fetch::fetch_notebooks(gw, 100).await?;
        let data = json!({
            "books": result.books,
            "totalBookCount": result.total_book_count,
            "totalNoteCount": result.total_note_count,
        });
        upsert_best_effort(upsert_books_from_notebooks(&pool, &result.books)).await;
        Ok(data)
    })
    .await
}

pub async fn proxy_book_info<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    book_id: &str,
    refresh: bool,
) -> AppResult<CachedPayload> {
    validate_book_id(book_id)?;
    let key = format!("book/info:{book_id}");
    let ttl = state.config.cache_ttl_secs;
    cached_call(state, &key, ttl, refresh, || async move {
        gw.call("/book/info", json!({ "bookId": book_id })).await
    })
    .await
}

pub async fn proxy_book_chapters<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    book_id: &str,
    refresh: bool,
) -> AppResult<CachedPayload> {
    validate_book_id(book_id)?;
    let key = format!("book/chapterinfo:{book_id}");
    let ttl = state.config.cache_ttl_secs;
    cached_call(state, &key, ttl, refresh, || async move {
        gw.call("/book/chapterinfo", json!({ "bookId": book_id })).await
    })
    .await
}

pub async fn proxy_book_progress<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    book_id: &str,
    refresh: bool,
) -> AppResult<CachedPayload> {
    validate_book_id(book_id)?;
    let key = format!("book/getprogress:{book_id}");
    let ttl = state.config.cache_ttl_secs;
    cached_call(state, &key, ttl, refresh, || async move {
        gw.call("/book/getprogress", json!({ "bookId": book_id })).await
    })
    .await
}

pub async fn proxy_readdata<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    mode: &str,
    base_time: Option<i64>,
    refresh: bool,
) -> AppResult<CachedPayload> {
    if !["weekly", "monthly", "annually", "overall"].contains(&mode) {
        return Err(AppError::InvalidArgument(format!(
            "非法 mode: {mode}（允许值: weekly/monthly/annually/overall）"
        )));
    }
    let key = format!("readdata:{mode}:{}", base_time.unwrap_or(0));
    let ttl = state.config.cache_ttl_secs;
    let mode = mode.to_string();
    cached_call(state, &key, ttl, refresh, || async move {
        let mut params = json!({ "mode": mode });
        if let Some(t) = base_time {
            params["baseTime"] = json!(t);
        }
        gw.call("/readdata/detail", params).await
    })
    .await
}

pub async fn proxy_gateway_apis<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    refresh: bool,
) -> AppResult<CachedPayload> {
    cached_call(state, "gateway/_list", 86_400, refresh, || async {
        gw.call("/_list", json!({})).await
    })
    .await
}

/// 单书笔记（编排端点）：bookmarklist + 想法合并；书名缺失时从笔记本概览回查。
pub async fn proxy_book_notes<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    book_id: &str,
    refresh: bool,
) -> AppResult<CachedPayload> {
    validate_book_id(book_id)?;
    let key = format!("notes:{book_id}");
    let ttl = state.config.cache_ttl_secs;
    cached_call(state, &key, ttl, refresh, || async move {
        let detail = fetch::fetch_note_detail(gw, book_id).await?;
        let mut book = detail.book;
        let need_fallback = book
            .get("title")
            .and_then(|t| t.as_str())
            .map(|s| s.is_empty())
            .unwrap_or(true);
        if need_fallback {
            if let Ok(nbs) = fetch::fetch_notebooks(gw, 100).await {
                for nb in &nbs.books {
                    if fetch::id_str(nb.get("bookId")) == book_id {
                        if let Some(b) = nb.get("book") {
                            book = b.clone();
                        }
                        break;
                    }
                }
            }
        }
        let reviews = match fetch::fetch_my_reviews(gw, book_id).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "想法列表获取失败，返回空想法列表");
                Vec::new()
            }
        };
        let chapters: Vec<Value> = detail
            .chapters
            .iter()
            .map(|(uid, title)| json!({"chapterUid": uid, "title": title}))
            .collect();
        Ok(json!({
            "bookId": book_id,
            "book": book,
            "chapters": chapters,
            "bookmarks": detail.bookmarks,
            "reviews": reviews,
        }))
    })
    .await
}

pub fn validate_book_id(id: &str) -> AppResult<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(format!("非法 bookId: {id:?}")))
    }
}

// --------------------------------------------------------------------------- //
// 导出任务
// --------------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    /// None = 全部有笔记的书；Some(非空列表) = 指定书
    pub book_ids: Option<Vec<String>>,
    pub refresh: bool,
}

pub struct CreateTaskOutcome {
    pub view: TaskView,
    pub skipped_cached: usize,
    pub queued_books: usize,
}

pub async fn create_task<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    input: CreateTaskInput,
) -> AppResult<CreateTaskOutcome> {
    let now = ServerConfig::now_ms();

    // 解析书单
    let requested: Vec<String> = match &input.book_ids {
        None => {
            let nbs = fetch::fetch_notebooks(gw, 100).await?;
            nbs.books
                .iter()
                .map(|nb| fetch::id_str(nb.get("bookId")))
                .filter(|id| id != "?")
                .collect()
        }
        Some(ids) => {
            if ids.is_empty() {
                return Err(AppError::InvalidArgument(
                    "book_ids 不能为空数组（缺省表示全部书）".to_string(),
                ));
            }
            let mut seen: Vec<String> = Vec::new();
            for id in ids {
                validate_book_id(id)?;
                if !seen.contains(id) {
                    seen.push(id.clone());
                }
            }
            seen
        }
    };
    if requested.is_empty() {
        return Err(AppError::InvalidArgument(
            "没有可导出的书（笔记本为空或 book_ids 无效）".to_string(),
        ));
    }

    // 运行中任务上限
    let running_tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status IN ('queued','running')",
    )
    .fetch_one(&state.pool)
    .await?;
    if running_tasks >= state.config.max_running_tasks {
        return Err(AppError::TooManyTasks);
    }

    // 已导出的书直接跳过（除非 refresh）
    struct Item {
        book_id: String,
        exported: Option<(i64, i64)>,
    }
    let mut items: Vec<Item> = Vec::with_capacity(requested.len());
    for book_id in &requested {
        let exported: Option<(i64, i64)> = if input.refresh {
            None
        } else {
            sqlx::query_as("SELECT bookmarks, reviews FROM exports WHERE book_id = ?")
                .bind(book_id)
                .fetch_optional(&state.pool)
                .await?
        };
        items.push(Item {
            book_id: book_id.clone(),
            exported,
        });
    }
    let queued_count = items.iter().filter(|i| i.exported.is_none()).count() as i64;

    // 队列长度上限
    let waiting: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM export_items WHERE state = 'waiting'")
            .fetch_one(&state.pool)
            .await?;
    if waiting + queued_count > state.config.queue_limit {
        return Err(AppError::TooManyTasks);
    }

    let task_id = new_task_id();
    let request_json = serde_json::to_string(&json!({
        "book_ids": requested,
        "explicit_books": input.book_ids.is_some(),
        "refresh": input.refresh,
    }))
    .map_err(|e| AppError::Internal(e.to_string()))?;
    sqlx::query("INSERT INTO tasks (task_id, request_json, status, created_ms) VALUES (?, ?, 'queued', ?)")
        .bind(&task_id)
        .bind(&request_json)
        .bind(now)
        .execute(&state.pool)
        .await?;

    for item in &items {
        sqlx::query(
            "INSERT INTO export_items (task_id, book_id, state, bookmarks, reviews, updated_ms)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&task_id)
        .bind(&item.book_id)
        .bind(if item.exported.is_some() { "skipped" } else { "waiting" })
        .bind(item.exported.map(|c| c.0))
        .bind(item.exported.map(|c| c.1))
        .bind(now)
        .execute(&state.pool)
        .await?;
    }

    let skipped_cached = items.iter().filter(|i| i.exported.is_some()).count();
    if queued_count > 0 {
        state.wake_worker();
    } else {
        // 全部命中已有导出 → 直接终态
        recompute_task_status(state, &task_id).await?;
    }

    let view = get_task(state, &task_id).await?;
    Ok(CreateTaskOutcome {
        view,
        skipped_cached,
        queued_books: queued_count as usize,
    })
}

// --------------------------------------------------------------------------- //
// 任务视图
// --------------------------------------------------------------------------- //

#[derive(Serialize)]
pub struct TaskView {
    pub task_id: String,
    pub status: String,
    pub request: Value,
    pub progress: Value,
    pub items_count: i64,
    pub error: Option<Value>,
    /// 0-100；completed 恒 100，未终态封顶 99
    pub percent: Option<u32>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub expires_at: Option<String>,
    pub result_url: String,
}

async fn lookup_title(state: &AppState, book_id: &str) -> Option<String> {
    let t: Option<String> = sqlx::query_scalar("SELECT title FROM books WHERE book_id = ?")
        .bind(book_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    if let Some(title) = t.filter(|s| !s.is_empty()) {
        return Some(title);
    }
    let bj: Option<String> = sqlx::query_scalar("SELECT book_json FROM exports WHERE book_id = ?")
        .bind(book_id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();
    bj.and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(String::from))
        .filter(|s| !s.is_empty())
}

pub async fn get_task(state: &AppState, task_id: &str) -> AppResult<TaskView> {
    let row = sqlx::query(
        "SELECT task_id, request_json, status, items_count, error_json,
                created_ms, started_ms, finished_ms, expires_ms
         FROM tasks WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::TaskNotFound(task_id.to_string()))?;
    let status: String = row.get("status");

    let states: Vec<(String, i64)> =
        sqlx::query_as("SELECT state, COUNT(*) FROM export_items WHERE task_id = ? GROUP BY state")
            .bind(task_id)
            .fetch_all(&state.pool)
            .await?;
    let mut done = 0i64;
    let mut skipped = 0i64;
    let mut failed = 0i64;
    let mut pending = 0i64;
    for (s, n) in &states {
        match s.as_str() {
            "done" => done += n,
            "skipped" => skipped += n,
            "error" => failed += n,
            _ => pending += n,
        }
    }
    let total_books = done + skipped + failed + pending;

    let current_book_id: Option<String> =
        sqlx::query("SELECT book_id FROM export_items WHERE task_id = ? AND state = 'running' LIMIT 1")
            .bind(task_id)
            .fetch_optional(&state.pool)
            .await?
            .map(|r| r.get::<String, _>("book_id"));

    let current = match current_book_id {
        Some(book_id) => {
            let title = lookup_title(state, &book_id).await;
            Some(json!({"book_id": book_id, "title": title}))
        }
        None => None,
    };

    let percent = if status == "completed" {
        Some(100)
    } else if total_books <= 0 {
        None
    } else {
        let pct = ((done + skipped) as f64 / total_books as f64 * 100.0).floor() as u32;
        Some(pct.min(99))
    };

    let error_json: Option<String> = row.get("error_json");
    Ok(TaskView {
        task_id: row.get("task_id"),
        status,
        request: serde_json::from_str(&row.get::<String, _>("request_json")).unwrap_or(json!({})),
        progress: json!({
            "done_books": done + skipped,
            "total_books": total_books,
            "skipped_books": skipped,
            "failed_books": failed,
            "current": current,
        }),
        items_count: row.get("items_count"),
        error: error_json.and_then(|s| serde_json::from_str(&s).ok()),
        percent,
        created_at: ms_to_rfc3339(row.get("created_ms")),
        started_at: row.get::<Option<i64>, _>("started_ms").map(ms_to_rfc3339),
        finished_at: row.get::<Option<i64>, _>("finished_ms").map(ms_to_rfc3339),
        expires_at: row.get::<Option<i64>, _>("expires_ms").map(ms_to_rfc3339),
        result_url: format!("/api/v1/exports/{task_id}/result"),
    })
}

pub struct ListFilter {
    pub status: Option<String>,
    pub limit: i64,
    pub cursor_rowid: Option<i64>,
}

#[derive(Serialize)]
pub struct TaskSummary {
    pub task_id: String,
    pub status: String,
    pub items_count: i64,
    pub created_at: String,
    pub url: String,
}

pub async fn list_tasks(
    state: &AppState,
    f: ListFilter,
) -> AppResult<(Vec<TaskSummary>, Option<i64>)> {
    let rows = sqlx::query(
        "SELECT rowid, task_id, status, items_count, created_ms FROM tasks
         WHERE (? IS NULL OR status = ?)
           AND (? IS NULL OR rowid < ?)
         ORDER BY rowid DESC
         LIMIT ?",
    )
    .bind(&f.status)
    .bind(&f.status)
    .bind(f.cursor_rowid)
    .bind(f.cursor_rowid)
    .bind(f.limit + 1)
    .fetch_all(&state.pool)
    .await?;

    let has_more = rows.len() as i64 > f.limit;
    let mut next_cursor = None;
    let mut summaries = Vec::with_capacity(rows.len().saturating_sub(1));
    for r in rows {
        let rowid: i64 = r.get("rowid");
        if summaries.len() as i64 >= f.limit {
            break;
        }
        let task_id: String = r.get("task_id");
        summaries.push(TaskSummary {
            url: format!("/api/v1/exports/{task_id}"),
            task_id,
            status: r.get("status"),
            items_count: r.get("items_count"),
            created_at: ms_to_rfc3339(r.get("created_ms")),
        });
        next_cursor = Some(rowid);
    }
    if !has_more {
        next_cursor = None;
    }
    Ok((summaries, next_cursor))
}

pub async fn delete_task(state: &AppState, task_id: &str) -> AppResult<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM tasks WHERE task_id = ?")
        .bind(task_id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    if exists.is_none() {
        return Err(AppError::TaskNotFound(task_id.to_string()));
    }
    // 只删任务记录；exports 产物与任务生命周期解耦，长期保留
    sqlx::query("DELETE FROM export_items WHERE task_id = ?")
        .bind(task_id)
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM tasks WHERE task_id = ?")
        .bind(task_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// 任务结果
// --------------------------------------------------------------------------- //

#[derive(Serialize)]
pub struct ResultBookView {
    pub book_id: String,
    pub title: Option<String>,
    pub state: String,
    pub bookmarks: Option<i64>,
    pub reviews: Option<i64>,
    pub error: Option<String>,
    pub markdown_url: Option<String>,
}

#[derive(Serialize)]
pub struct ResultView {
    pub task_id: String,
    pub status: String,
    pub summary: Value,
    pub books: Vec<ResultBookView>,
}

pub async fn task_result(state: &AppState, task_id: &str) -> AppResult<ResultView> {
    let row = sqlx::query("SELECT status FROM tasks WHERE task_id = ?")
        .bind(task_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::TaskNotFound(task_id.to_string()))?;
    let status: String = row.get("status");
    if !TERMINAL_STATUSES.contains(&status.as_str()) {
        return Err(AppError::TaskNotFinished(status));
    }

    let items = sqlx::query(
        "SELECT book_id, state, bookmarks, reviews, error FROM export_items
         WHERE task_id = ? ORDER BY rowid",
    )
    .bind(task_id)
    .fetch_all(&state.pool)
    .await?;

    let mut succeeded = 0i64;
    let mut failed = 0i64;
    let mut total_bm = 0i64;
    let mut total_rv = 0i64;
    let mut books = Vec::with_capacity(items.len());
    for r in items {
        let book_id: String = r.get("book_id");
        let item_state: String = r.get("state");
        let bookmarks: Option<i64> = r.get("bookmarks");
        let reviews: Option<i64> = r.get("reviews");
        let ok = item_state == "done" || item_state == "skipped";
        if ok {
            succeeded += 1;
            total_bm += bookmarks.unwrap_or(0);
            total_rv += reviews.unwrap_or(0);
        } else {
            failed += 1;
        }
        books.push(ResultBookView {
            title: lookup_title(state, &book_id).await,
            markdown_url: if ok {
                Some(format!("/api/v1/books/{book_id}/notes/markdown"))
            } else {
                None
            },
            book_id,
            state: item_state,
            bookmarks,
            reviews,
            error: r.get("error"),
        });
    }

    Ok(ResultView {
        task_id: task_id.to_string(),
        status,
        summary: json!({
            "books": succeeded + failed,
            "succeeded": succeeded,
            "failed": failed,
            "bookmarks": total_bm,
            "reviews": total_rv,
        }),
        books,
    })
}

// --------------------------------------------------------------------------- //
// 任务终态推进（worker 与创建路径共用）
// --------------------------------------------------------------------------- //

pub async fn recompute_task_status(state: &AppState, task_id: &str) -> anyhow::Result<()> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT state, COUNT(*) AS n,
                COALESCE(SUM(COALESCE(bookmarks,0) + COALESCE(reviews,0)), 0) AS items
         FROM export_items WHERE task_id = ? GROUP BY state",
    )
    .bind(task_id)
    .fetch_all(&state.pool)
    .await?;

    let mut done = 0i64;
    let mut skipped = 0i64;
    let mut error = 0i64;
    let mut pending = 0i64;
    let mut items = 0i64;
    for (s, n, it) in rows {
        match s.as_str() {
            "done" => {
                done += n;
                items += it;
            }
            "skipped" => {
                skipped += n;
                items += it;
            }
            "error" => error += n,
            _ => pending += n,
        }
    }
    if pending > 0 {
        return Ok(()); // 尚有书未终态
    }

    let status = if error == 0 {
        "completed"
    } else if done + skipped > 0 {
        "partial"
    } else {
        "failed"
    };
    let now = ServerConfig::now_ms();
    let expires = now + state.config.task_retention_days * 24 * 3600 * 1000;
    sqlx::query(
        "UPDATE tasks SET status = ?, finished_ms = ?, expires_ms = ?, items_count = ?
         WHERE task_id = ?",
    )
    .bind(status)
    .bind(now)
    .bind(expires)
    .bind(items)
    .bind(task_id)
    .execute(&state.pool)
    .await?;

    if status != "completed" {
        let failed_books: Vec<(String, String)> = sqlx::query_as(
            "SELECT book_id, COALESCE(error, '') FROM export_items
             WHERE task_id = ? AND state = 'error'",
        )
        .bind(task_id)
        .fetch_all(&state.pool)
        .await?;
        let error_json = json!({
            "code": if status == "partial" { "EXPORT_PARTIAL" } else { "EXPORT_FAILED" },
            "message": format!("{error} 本书导出失败"),
            "failed_books": failed_books
                .iter()
                .map(|(id, e)| json!({"book_id": id, "error": e}))
                .collect::<Vec<_>>(),
        });
        sqlx::query("UPDATE tasks SET error_json = ? WHERE task_id = ?")
            .bind(error_json.to_string())
            .bind(task_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

// --------------------------------------------------------------------------- //
// Markdown 读取
// --------------------------------------------------------------------------- //

pub struct MarkdownFile {
    pub markdown: String,
    pub title: String,
    pub bookmarks: i64,
    pub reviews: i64,
    pub exported_at: String,
}

pub async fn get_markdown(state: &AppState, book_id: &str) -> AppResult<MarkdownFile> {
    validate_book_id(book_id)?;
    let row = sqlx::query(
        "SELECT markdown, book_json, bookmarks, reviews, exported_ms FROM exports WHERE book_id = ?",
    )
    .bind(book_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::ExportNotFound {
        book_id: book_id.to_string(),
    })?;
    let title = serde_json::from_str::<Value>(&row.get::<String, _>("book_json"))
        .ok()
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(String::from))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| book_id.to_string());
    Ok(MarkdownFile {
        markdown: row.get("markdown"),
        title,
        bookmarks: row.get("bookmarks"),
        reviews: row.get("reviews"),
        exported_at: ms_to_rfc3339(row.get("exported_ms")),
    })
}

// --------------------------------------------------------------------------- //
// 过期清理
// --------------------------------------------------------------------------- //

pub async fn run_cleanup_loop(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        if let Err(e) = cleanup_once(&state).await {
            tracing::warn!(error = %e, "cleanup failed");
        }
    }
}

async fn cleanup_once(state: &AppState) -> anyhow::Result<()> {
    let now = ServerConfig::now_ms();

    let expired: Vec<String> = sqlx::query_scalar(
        "SELECT task_id FROM tasks WHERE expires_ms IS NOT NULL AND expires_ms < ?",
    )
    .bind(now)
    .fetch_all(&state.pool)
    .await?;
    for task_id in expired {
        sqlx::query("DELETE FROM export_items WHERE task_id = ?")
            .bind(&task_id)
            .execute(&state.pool)
            .await?;
        sqlx::query("DELETE FROM tasks WHERE task_id = ?")
            .bind(&task_id)
            .execute(&state.pool)
            .await?;
        tracing::info!(task_id = %task_id, "清理过期任务（exports 产物保留）");
    }

    // 缓存表防止无限增长：过期超过 7 天的条目清理
    sqlx::query("DELETE FROM cache_entries WHERE expires_ms < ?")
        .bind(now - 7 * 24 * 3600 * 1000)
        .execute(&state.pool)
        .await?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// 服务元信息
// --------------------------------------------------------------------------- //

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() > 16 {
        let head: String = chars[..8].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", head, tail)
    } else {
        "***".to_string()
    }
}

pub async fn server_info(state: &AppState) -> AppResult<Value> {
    let waiting: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM export_items WHERE state = 'waiting'")
            .fetch_one(&state.pool)
            .await?;
    let running_items: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM export_items WHERE state = 'running'")
            .fetch_one(&state.pool)
            .await?;
    let running_tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status IN ('queued','running')",
    )
    .fetch_one(&state.pool)
    .await?;
    let exported_books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exports")
        .fetch_one(&state.pool)
        .await?;
    let circuit_remaining = state.circuit_remaining_ms();
    Ok(json!({
        "name": "weread-notes-server",
        "version": env!("CARGO_PKG_VERSION"),
        "gateway": {
            "base_url": state.config.gateway_url,
            "skill_version": crate::client::SKILL_VERSION,
            "api_key": mask_key(&state.config.api_key),
            "min_interval_ms": state.config.gateway_min_interval_ms,
        },
        "cache": { "ttl_seconds": state.config.cache_ttl_secs },
        "queue": {
            "waiting": waiting,
            "running_items": running_items,
            "running_tasks": running_tasks,
            "exported_books": exported_books,
        },
        "circuit": {
            "threshold": state.config.circuit_threshold,
            "cooldown_seconds": state.config.circuit_cooldown_secs,
            "open": circuit_remaining > 0,
            "remaining_ms": circuit_remaining,
        },
        "auth_enabled": !state.config.token.is_empty(),
    }))
}
