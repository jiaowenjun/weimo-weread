//! SQLite 连接与建表（内嵌 bootstrap SQL，不用 sqlx migrate）。
//!
//! BYOK 模型下存储按 Key 指纹（owner 列 / cache key 前缀）隔离，不同 Key
//! 的缓存与导出产物互不混淆。表分两类：cache_entries 可再生（代理缓存）；
//! books/exports 是长期资产（exports 存笔记原始 JSON），tasks/export_items 是任务调度状态。

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const BOOTSTRAP_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS cache_entries (
  cache_key  TEXT PRIMARY KEY,
  payload    TEXT NOT NULL,
  fetched_ms INTEGER NOT NULL,
  expires_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS books (
  owner          TEXT NOT NULL,
  book_id        TEXT NOT NULL,
  title          TEXT NOT NULL DEFAULT '',
  author         TEXT NOT NULL DEFAULT '',
  cover          TEXT NOT NULL DEFAULT '',
  progress       INTEGER,
  finish_reading INTEGER NOT NULL DEFAULT 0,
  read_update_time INTEGER,
  bookmark_count INTEGER,
  note_count     INTEGER,
  review_count   INTEGER,
  updated_ms     INTEGER NOT NULL,
  PRIMARY KEY (owner, book_id)
);

CREATE TABLE IF NOT EXISTS tasks (
  task_id      TEXT PRIMARY KEY,
  api_key      TEXT NOT NULL DEFAULT '',
  request_json TEXT NOT NULL,
  status       TEXT NOT NULL DEFAULT 'queued',
  items_count  INTEGER NOT NULL DEFAULT 0,
  error_json   TEXT,
  created_ms   INTEGER NOT NULL,
  started_ms   INTEGER,
  finished_ms  INTEGER,
  expires_ms   INTEGER
);

CREATE TABLE IF NOT EXISTS export_items (
  task_id      TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
  book_id      TEXT NOT NULL,
  state        TEXT NOT NULL DEFAULT 'waiting',
  bookmarks    INTEGER,
  reviews      INTEGER,
  error        TEXT,
  attempts     INTEGER NOT NULL DEFAULT 0,
  next_sync_ms INTEGER,
  updated_ms   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (task_id, book_id)
);

CREATE TABLE IF NOT EXISTS exports (
  owner       TEXT NOT NULL,
  book_id     TEXT NOT NULL,
  data_json   TEXT NOT NULL,
  bookmarks   INTEGER NOT NULL DEFAULT 0,
  reviews     INTEGER NOT NULL DEFAULT 0,
  exported_ms INTEGER NOT NULL,
  PRIMARY KEY (owner, book_id)
);

CREATE INDEX IF NOT EXISTS idx_export_items_due ON export_items(state, next_sync_ms);
CREATE INDEX IF NOT EXISTS idx_exports_time ON exports(exported_ms DESC);
"#;

pub async fn connect(db_path: &std::path::Path) -> Result<SqlitePool> {
    let url = format!("sqlite:{}", db_path.display());
    let opts = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .with_context(|| format!("连接数据库失败: {}", db_path.display()))?;

    // 旧版库整体重建（产物可再生，重新抓取即可）：
    // - v0.1 单用户 schema：exports 无 owner / tasks 无 api_key
    // - v0.2 markdown 时代：exports 含 markdown/book_json 列，无法无损转为原始 JSON
    if needs_rebuild(&pool).await? {
        tracing::warn!("检测到旧版库，重建表结构（缓存/导出数据将重新生成）");
        for table in ["exports", "books", "export_items", "tasks", "cache_entries"] {
            sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
                .execute(&pool)
                .await
                .context("旧表清理失败")?;
        }
    }

    for statement in BOOTSTRAP_SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .with_context(|| format!("建表失败: {statement}"))?;
    }
    Ok(pool)
}

async fn needs_rebuild(pool: &SqlitePool) -> Result<bool> {
    let exports_cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('exports')")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    let tasks_cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    // 空表列表 = 全新库；含 markdown 列 = v0.2 产物库；缺 owner/api_key = v0.1 单用户库
    Ok(exports_cols.iter().any(|c| c == "markdown" || c == "book_json")
        || (!exports_cols.is_empty() && !exports_cols.iter().any(|c| c == "owner"))
        || (!tasks_cols.is_empty() && !tasks_cols.iter().any(|c| c == "api_key")))
}
