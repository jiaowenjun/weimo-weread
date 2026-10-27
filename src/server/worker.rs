//! 导出 worker——DB 即队列，单 worker 串行。
//!
//! 网关侧全局限速由 ThrottledGateway 统一保证（代理端点与 worker 共享），
//! worker 自身不再额外休眠；重试/熔断策略见设计文档第 8 节。

use anyhow::{Context, Result};
use rand::Rng;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::client::{GatewayApi, GatewayError};
use crate::fetch;

use super::config::ServerConfig;
use super::service;
use super::state::AppState;

/// 单书最大尝试次数（含首次）
const MAX_ATTEMPTS: i64 = 3;
const IDLE_SCAN: Duration = Duration::from_secs(60);

pub(super) struct DueItem {
    task_id: String,
    book_id: String,
    attempts: i64,
}

pub async fn run_worker<G: GatewayApi>(state: AppState, gw: Arc<G>) {
    let mut wakeup = state.worker_wakeup_receiver();
    loop {
        // 熔断期间不抓取，等冷却结束（分段等待，最长 60s 一段）
        let circuit_ms = state.circuit_remaining_ms();
        if circuit_ms > 0 {
            tokio::time::sleep(Duration::from_millis(circuit_ms.min(60_000) as u64)).await;
            continue;
        }

        match process_next_due(&state, gw.as_ref()).await {
            Ok(true) => {} // 处理了一本书，立即取下一本
            Ok(false) => {
                tokio::select! {
                    changed = wakeup.changed() => {
                        if changed.is_err() {
                            warn!("worker wakeup channel closed");
                            tokio::time::sleep(IDLE_SCAN).await;
                        }
                    }
                    _ = tokio::time::sleep(IDLE_SCAN) => {}
                }
            }
            Err(e) => {
                warn!(error = %e, "worker tick failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// 处理一本到期书（导出 + 状态推进）。返回是否处理了；
/// 供 run_worker 循环与测试复用（测试可直接驱动而无需起后台 worker）。
pub async fn process_next_due<G: GatewayApi>(state: &AppState, gw: &G) -> Result<bool> {
    match pick_due_item(&state.pool).await? {
        Some(item) => {
            export_item(state, gw, &item).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn pick_due_item(pool: &SqlitePool) -> Result<Option<DueItem>> {
    // 含 running：进程重启后中断的书直接重试（重抓单书成本低，无需断点）
    let row = sqlx::query(
        "SELECT task_id, book_id, attempts FROM export_items
         WHERE state IN ('waiting','running')
           AND (next_sync_ms IS NULL OR next_sync_ms <= ?)
         ORDER BY updated_ms ASC, rowid ASC
         LIMIT 1",
    )
    .bind(ServerConfig::now_ms())
    .fetch_optional(pool)
    .await
    .context("查询到期导出项失败")?;

    Ok(row.map(|r| DueItem {
        task_id: r.get("task_id"),
        book_id: r.get("book_id"),
        attempts: r.get("attempts"),
    }))
}

async fn export_item<G: GatewayApi>(state: &AppState, gw: &G, item: &DueItem) -> Result<()> {
    let now = ServerConfig::now_ms();

    sqlx::query(
        "UPDATE export_items SET state='running', updated_ms=? WHERE task_id=? AND book_id=?",
    )
    .bind(now)
    .bind(&item.task_id)
    .bind(&item.book_id)
    .execute(&state.pool)
    .await
    .context("标记导出项 running 失败")?;

    // 首次处理的书把任务置为 running
    sqlx::query(
        "UPDATE tasks SET status='running', started_ms=COALESCE(started_ms, ?)
         WHERE task_id=? AND status='queued'",
    )
    .bind(now)
    .bind(&item.task_id)
    .execute(&state.pool)
    .await
    .context("标记任务 running 失败")?;

    match export_book_once(state, gw, &item.book_id).await {
        Ok((bookmarks, reviews)) => {
            state.record_crawl_success();
            sqlx::query(
                "UPDATE export_items SET state='done', bookmarks=?, reviews=?,
                        attempts=attempts+1, next_sync_ms=NULL, error=NULL, updated_ms=?
                 WHERE task_id=? AND book_id=?",
            )
            .bind(bookmarks)
            .bind(reviews)
            .bind(ServerConfig::now_ms())
            .bind(&item.task_id)
            .bind(&item.book_id)
            .execute(&state.pool)
            .await
            .context("标记导出项 done 失败")?;
            info!(
                task_id = %item.task_id,
                book_id = %item.book_id,
                bookmarks, reviews,
                "书籍导出完成"
            );
        }
        Err(e) => handle_export_error(state, item, e).await?,
    }

    service::recompute_task_status(state, &item.task_id).await?;
    state.notify_progress(&item.task_id);
    Ok(())
}

/// 抓取并生成单书 Markdown，写入 exports 表（INSERT OR REPLACE 语义）。
async fn export_book_once<G: GatewayApi>(
    state: &AppState,
    gw: &G,
    book_id: &str,
) -> Result<(i64, i64), GatewayError> {
    let detail = fetch::fetch_note_detail(gw, book_id).await?;
    let mut book = detail.book;
    let need_fallback = book
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.is_empty())
        .unwrap_or(true);
    if need_fallback {
        // 划线响应缺书籍信息，从笔记本概览回查（与 CLI 行为一致）
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
            warn!(error = %e, "想法列表获取失败，导出不含想法（与 CLI 行为一致）");
            Vec::new()
        }
    };
    let (bm_count, rv_count) = (detail.bookmarks.len() as i64, reviews.len() as i64);
    let md = crate::markdown::build_markdown(
        book_id,
        &book,
        &detail.chapters,
        &detail.bookmarks,
        &reviews,
    );

    let now = ServerConfig::now_ms();
    let book_json = serde_json::to_string(&book).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        "INSERT INTO exports (book_id, markdown, book_json, bookmarks, reviews, exported_ms)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(book_id) DO UPDATE SET
            markdown=excluded.markdown, book_json=excluded.book_json,
            bookmarks=excluded.bookmarks, reviews=excluded.reviews,
            exported_ms=excluded.exported_ms",
    )
    .bind(book_id)
    .bind(&md)
    .bind(&book_json)
    .bind(bm_count)
    .bind(rv_count)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| GatewayError::Io(format!("写入 exports 表失败: {e}")))?;

    // best-effort 更新 books 表计数
    let _ = sqlx::query("UPDATE books SET bookmark_count=?, review_count=?, updated_ms=? WHERE book_id=?")
        .bind(bm_count)
        .bind(rv_count)
        .bind(now)
        .bind(book_id)
        .execute(&state.pool)
        .await;

    Ok((bm_count, rv_count))
}

async fn handle_export_error(state: &AppState, item: &DueItem, e: GatewayError) -> Result<()> {
    let now = ServerConfig::now_ms();
    warn!(
        task_id = %item.task_id,
        book_id = %item.book_id,
        attempt = item.attempts + 1,
        error = %e,
        "书籍导出失败"
    );

    // 业务错误（无效 bookId 等）永久失败；其余按次数退避重试，超限终态失败
    let permanent = !e.is_transient();
    let over_limit = item.attempts + 1 >= MAX_ATTEMPTS;
    if permanent || over_limit {
        sqlx::query(
            "UPDATE export_items SET state='error', attempts=?, next_sync_ms=NULL,
                    error=?, updated_ms=?
             WHERE task_id=? AND book_id=?",
        )
        .bind(item.attempts + 1)
        .bind(e.to_string())
        .bind(now)
        .bind(&item.task_id)
        .bind(&item.book_id)
        .execute(&state.pool)
        .await
        .context("标记导出项 error 失败")?;
        if !permanent {
            state.record_crawl_failure();
        }
    } else {
        let tripped = state.record_crawl_failure();
        let backoff_ms = (item.attempts + 1) * 20_000 + rand::thread_rng().gen_range(0..5_000);
        sqlx::query(
            "UPDATE export_items SET state='waiting', attempts=?, next_sync_ms=?, error=?, updated_ms=?
             WHERE task_id=? AND book_id=?",
        )
        .bind(item.attempts + 1)
        .bind(now + backoff_ms)
        .bind(e.to_string())
        .bind(now)
        .bind(&item.task_id)
        .bind(&item.book_id)
        .execute(&state.pool)
        .await
        .context("标记导出项退避失败")?;
        if tripped {
            info!("熔断已触发，worker 将暂停抓取直至冷却结束");
        }
    }
    Ok(())
}
