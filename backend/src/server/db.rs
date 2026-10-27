//! SQLite 连接与建表（内嵌 bootstrap SQL，不用 sqlx migrate）。
//!
//! BYOK 模型下存储按 Key 指纹（owner 列 / cache key 前缀）隔离，不同 Key
//! 的缓存与已同步笔记互不混淆。表分两类：cache_entries 可再生（代理缓存）；
//! books/notes 是长期资产（notes 存笔记原始 JSON），tasks/task_items 是任务调度状态。

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

CREATE TABLE IF NOT EXISTS task_items (
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

CREATE TABLE IF NOT EXISTS notes (
  owner       TEXT NOT NULL,
  book_id     TEXT NOT NULL,
  data_json   TEXT NOT NULL,
  bookmarks   INTEGER NOT NULL DEFAULT 0,
  reviews     INTEGER NOT NULL DEFAULT 0,
  saved_ms    INTEGER NOT NULL,
  PRIMARY KEY (owner, book_id)
);

CREATE INDEX IF NOT EXISTS idx_task_items_due ON task_items(state, next_sync_ms);
CREATE INDEX IF NOT EXISTS idx_notes_time ON notes(saved_ms DESC);
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
    // - v0.1 单用户 schema：exports 表无 owner / tasks 无 api_key
    // - v0.2 markdown 时代：exports 含 markdown/book_json 列
    // - v0.3 导出命名时代：表名还是 exports / export_items
    if needs_rebuild(&pool).await? {
        tracing::warn!("检测到旧版库，重建表结构（缓存/已同步笔记将重新生成）");
        for table in [
            "notes",
            "task_items",
            "tasks",
            "books",
            "cache_entries",
            "exports",
            "export_items",
        ] {
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
    // sqlite_master 查旧表名（v0.1~v0.3），pragma 查 v0.1 单用户列缺失
    let legacy: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
            AND name IN ('exports', 'export_items')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    if legacy > 0 {
        return Ok(true);
    }
    let tasks_cols: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('tasks')")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    Ok(!tasks_cols.is_empty() && !tasks_cols.iter().any(|c| c == "api_key"))
}
