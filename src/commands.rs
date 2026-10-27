//! CLI 命令实现——薄 IO 层，编排逻辑在 [`crate::fetch`]。

use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::client::{GatewayApi, GatewayError, WereadClient};
use crate::fetch;
use crate::markdown;

pub async fn cmd_notebooks(
    client: &WereadClient,
    limit: usize,
    page_size: u64,
) -> Result<(), GatewayError> {
    let result = fetch::fetch_notebooks(client, page_size).await?;
    let items = result.books;
    if items.is_empty() {
        println!("没有做过任何笔记(空笔记本)。");
        return Ok(());
    }
    println!("共 {} 本有笔记的书:\n", items.len());
    println!(
        "{:<4} {:<14} {:<34} {:<10} {:<8} {:<8} {}",
        "#", "bookId", "书名", "划线", "笔记", "想法", "最近更新"
    );
    let mut rows = items;
    rows.sort_by_key(|nb| {
        let total = fetch::as_i64(nb.get("bookmarkCount"))
            + fetch::as_i64(nb.get("noteCount"))
            + fetch::as_i64(nb.get("reviewCount"));
        std::cmp::Reverse(total)
    });
    let shown = rows.len().min(limit);
    for (i, nb) in rows.iter().take(shown).enumerate() {
        let book = nb.get("book").unwrap_or(&Value::Null);
        let bm = fetch::as_i64(nb.get("bookmarkCount"));
        let nt = fetch::as_i64(nb.get("noteCount"));
        let rv = fetch::as_i64(nb.get("reviewCount"));
        let total = bm + nt + rv;
        let raw_title = book.get("title").and_then(|t| t.as_str()).unwrap_or("?");
        let title = if total > 0 {
            format!("{} *{}*", raw_title, total)
        } else {
            raw_title.to_string()
        };
        let upd = nb
            .get("updateTime")
            .filter(|v| !v.is_null())
            .or_else(|| book.get("updateTime"));
        let time = upd
            .and_then(|v| v.as_i64())
            .and_then(markdown::fmt_time)
            .unwrap_or_default();
        println!(
            "{:<4} {:<14} {:<34} {:<10} {:<8} {:<8} {}",
            i + 1,
            fetch::id_str(nb.get("bookId")),
            markdown::clip(&title, 34),
            bm,
            nt,
            rv,
            time
        );
    }
    if shown < rows.len() {
        println!("\n(仅显示前 {} 本, 共 {} 本)", shown, rows.len());
    }
    Ok(())
}

/// 导出单书：划线 + 想法 → Markdown（供 CLI 与测试复用的完整编排）。
pub async fn export_book_markdown(
    gw: &dyn GatewayApi,
    book_id: &str,
) -> Result<(String, Value, i64, i64), GatewayError> {
    let detail = fetch::fetch_note_detail(gw, book_id).await?;
    let mut book_info = detail.book;
    let need_fallback = book_info
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.is_empty())
        .unwrap_or(true);
    if need_fallback {
        eprintln!("(!) 划线响应缺少书籍信息, 从笔记本概览回查。");
        let nbs = fetch::fetch_notebooks(gw, 100).await?;
        for nb in &nbs.books {
            if fetch::id_str(nb.get("bookId")) == book_id {
                if let Some(b) = nb.get("book") {
                    book_info = b.clone();
                }
                break;
            }
        }
    }
    let reviews = match fetch::fetch_my_reviews(gw, book_id).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("(!) 想法列表获取失败, 已跳过: {}", e);
            Vec::new()
        }
    };
    let chapters: BTreeMap<i64, String> = detail.chapters;
    let (bm_count, rv_count) = (detail.bookmarks.len() as i64, reviews.len() as i64);
    let md = markdown::build_markdown(book_id, &book_info, &chapters, &detail.bookmarks, &reviews);
    Ok((md, book_info, bm_count, rv_count))
}

pub async fn cmd_export(
    client: &WereadClient,
    book_id: &str,
    output: Option<&str>,
) -> Result<(), GatewayError> {
    let (md, _book_info, bm_count, rv_count) = export_book_markdown(client, book_id).await?;
    match output {
        Some(path) => {
            std::fs::write(path, &md)
                .map_err(|e| GatewayError::Io(format!("写入 {} 失败: {}", path, e)))?;
            println!("已导出 {} 条划线 + {} 条想法 -> {}", bm_count, rv_count, path);
        }
        None => println!("{}", md),
    }
    Ok(())
}

fn smart_value(s: &str) -> Value {
    let t = s.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = t.parse::<i64>() {
            return Value::from(n);
        }
    }
    match t {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(t.to_string()),
    }
}

pub async fn cmd_raw(
    client: &WereadClient,
    api_name: &str,
    params: &[String],
) -> Result<(), GatewayError> {
    let mut body = Map::new();
    for kv in params {
        let (k, v) = match kv.split_once('=') {
            Some(p) => p,
            None => {
                return Err(GatewayError::InvalidJson(format!(
                    "参数格式应为 k=v, 得到: {:?}",
                    kv
                )))
            }
        };
        body.insert(k.to_string(), smart_value(v));
    }
    let data = client.call(api_name, Value::Object(body)).await?;
    let pretty =
        serde_json::to_string_pretty(&data).map_err(|e| GatewayError::InvalidJson(e.to_string()))?;
    println!("{}", pretty);
    Ok(())
}

pub async fn cmd_apis(client: &WereadClient) -> Result<(), GatewayError> {
    match client.call("/_list", serde_json::json!({})).await {
        Ok(data) => {
            let pretty = serde_json::to_string_pretty(&data)
                .map_err(|e| GatewayError::InvalidJson(e.to_string()))?;
            println!("{}", pretty);
        }
        Err(e) => eprintln!("(/_list 不可用: {})", e),
    }
    Ok(())
}
