//! 编排层——多接口翻页与合并（CLI 与服务器共享）。
//!
//! 从原 commands.rs 提出，供 `bin/weread-notes` 与 `server::worker` 共用。

use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::client::{GatewayApi, GatewayError};

pub fn as_i64(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64()).unwrap_or(0)
}

/// Unix 时间戳(秒或毫秒) -> "YYYY-MM-DD HH:MM"(本地时区)。
pub fn fmt_time(v: i64) -> Option<String> {
    use chrono::{Local, TimeZone};
    if v <= 0 {
        return None;
    }
    let secs = if v > 1_000_000_000_000 { v / 1000 } else { v };
    match Local.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => Some(dt.format("%Y-%m-%d %H:%M").to_string()),
        _ => None,
    }
}

/// 按字符数截断(对中日韩字符与 Python len 口径一致)。
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n - 1).collect();
        format!("{}…", head)
    }
}

/// bookId 兼容字符串与数字两种返回形态。
pub fn id_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "?".into(),
    }
}

#[derive(Debug, Default)]
pub struct NotebooksResult {
    pub books: Vec<Value>,
    /// 响应头部的汇总（首页才有；网关未返回时为 None）
    pub total_book_count: Option<i64>,
    pub total_note_count: Option<i64>,
}

/// 翻页拉全 /user/notebooks(游标为最后一条的 sort 字段)。
pub async fn fetch_notebooks(
    gw: &dyn GatewayApi,
    count: u64,
) -> Result<NotebooksResult, GatewayError> {
    let mut result = NotebooksResult::default();
    let mut last_sort: Option<i64> = None;
    for _ in 0..20 {
        let mut params = json!({ "count": count });
        if let Some(ls) = last_sort {
            params["lastSort"] = json!(ls);
        }
        let data = gw.call("/user/notebooks", params).await?;
        let empty = Vec::new();
        let books = data.get("books").and_then(|v| v.as_array()).unwrap_or(&empty);
        let page_len = books.len();
        if result.total_book_count.is_none() {
            result.total_book_count = data.get("totalBookCount").and_then(|v| v.as_i64());
            result.total_note_count = data.get("totalNoteCount").and_then(|v| v.as_i64());
        }
        result.books.extend(books.iter().cloned());
        let has_more = as_i64(data.get("hasMore")) == 1;
        if !has_more || page_len == 0 {
            break;
        }
        last_sort = books[page_len - 1].get("sort").and_then(|v| v.as_i64());
        if last_sort.is_none() {
            break;
        }
    }
    Ok(result)
}

pub struct NoteDetail {
    pub bookmarks: Vec<Value>,
    /// chapterUid -> 章节标题（有序）
    pub chapters: BTreeMap<i64, String>,
    pub book: Value,
}

/// 一次 /book/bookmarklist 拿全: 划线列表(updated) + 章节表(chapters) + 书籍信息(book)。
pub async fn fetch_note_detail(
    gw: &dyn GatewayApi,
    book_id: &str,
) -> Result<NoteDetail, GatewayError> {
    let data = gw.call("/book/bookmarklist", json!({ "bookId": book_id })).await?;
    let bookmarks = data
        .get("updated")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut chapters: BTreeMap<i64, String> = BTreeMap::new();
    if let Some(cs) = data.get("chapters").and_then(|v| v.as_array()) {
        for c in cs {
            if let Some(uid) = c.get("chapterUid").and_then(|v| v.as_i64()) {
                let title = c
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("第 {} 章", uid));
                chapters.insert(uid, title);
            }
        }
    }
    let book = data.get("book").cloned().unwrap_or(json!({}));
    Ok(NoteDetail {
        bookmarks,
        chapters,
        book,
    })
}

/// 我的想法(/review/list/mine, 注意参数是小写 bookid), synckey 翻页。
/// 失败是否容忍由调用方决定(CLI 与 worker 均降级为空列表并告警)。
pub async fn fetch_my_reviews(
    gw: &dyn GatewayApi,
    book_id: &str,
) -> Result<Vec<Value>, GatewayError> {
    let mut reviews = Vec::new();
    let mut synckey: Option<i64> = None;
    for _ in 0..20 {
        let mut params = json!({ "bookid": book_id, "count": 100 });
        if let Some(sk) = synckey {
            params["synckey"] = json!(sk);
        }
        let data = gw.call("/review/list/mine", params).await?;
        if let Some(arr) = data.get("reviews").and_then(|v| v.as_array()) {
            reviews.extend(arr.iter().cloned());
        }
        let has_more = as_i64(data.get("hasMore")) == 1;
        let sk = data.get("synckey").and_then(|v| v.as_i64());
        if !has_more || sk.is_none() {
            break;
        }
        synckey = sk;
    }
    Ok(reviews)
}
