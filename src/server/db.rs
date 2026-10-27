//! SQLite 连接与建表（内嵌 bootstrap SQL，不用 sqlx migrate）。
//!
//! 表分两类：cache_entries 可再生（代理缓存）；
//! books/exports 是长期资产（元数据汇总与导出产物），tasks/export_items 是任务调度状态。

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
  book_id         TEXT PRIMARY KEY,
  title           TEXT NOT NULL DEFAULT '',
  author          TEXT NOT NULL DEFAULT '',
  cover           TEXT NOT NULL DEFAULT '',
  progress        INTEGER,
  finish_reading  INTEGER NOT NULL DEFAULT 0,
  read_update_time INTEGER,
  bookmark_count  INTEGER,
  note_count      INTEGER,
  review_count    INTEGER,
  updated_ms      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
  task_id      TEXT PRIMARY KEY,
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
  book_id     TEXT PRIMARY KEY,
  markdown    TEXT NOT NULL,
  book_json   TEXT NOT NULL,
  bookmarks   INTEGER NOT NULL DEFAULT 0,
  reviews     INTEGER NOT NULL DEFAULT 0,
  exported_ms INTEGER NOT NULL
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
