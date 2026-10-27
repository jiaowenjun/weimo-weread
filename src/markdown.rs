use chrono::{Local, TimeZone};
use serde_json::Value;
use std::collections::BTreeMap;

/// Unix 时间戳(秒或毫秒) -> "YYYY-MM-DD HH:MM"(本地时区)。
pub fn fmt_time(v: i64) -> Option<String> {
    if v <= 0 {
        return None;
    }
    let secs = if v > 1_000_000_000_000 { v / 1000 } else { v };
    match Local.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => Some(dt.format("%Y-%m-%d %H:%M").to_string()),
        _ => None,
    }
}

/// 从 JSON 字段取时间戳(兼容数字与数字字符串)再格式化。
pub fn fmt_time_value(v: Option<&Value>) -> Option<String> {
    let n = match v? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }?;
    fmt_time(n)
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

fn chapter_uid(b: &Value) -> i64 {
    b.get("chapterUid").and_then(|v| v.as_i64()).unwrap_or(0)
}

fn time_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn review_time(r: &Value) -> i64 {
    time_i64(r.get("review").and_then(|inner| inner.get("createTime")))
}

/// 生成与 Python 版一致的 Markdown 结构:
/// 标题/作者/导出时间 -> ## 划线(按章节分组) -> ## 我的想法(带原文引用)。
pub fn build_markdown(
    book_id: &str,
    book_info: &Value,
    chapters: &BTreeMap<i64, String>,
    bookmarks: &[Value],
    reviews: &[Value],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let title = book_info
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(book_id);
    let author = book_info.get("author").and_then(|a| a.as_str()).unwrap_or("");

    lines.push(format!("# 《{}》读书笔记", title));
    lines.push(String::new());
    if !author.is_empty() {
        lines.push(format!("> 作者: {}", author));
    }
    lines.push(format!(
        "> 导出时间: {} | 划线 {} 条 | 想法 {} 条",
        Local::now().format("%Y-%m-%d %H:%M"),
        bookmarks.len(),
        reviews.len()
    ));
    lines.push(String::new());

    if !bookmarks.is_empty() {
        lines.push("## 划线".into());
        lines.push(String::new());
        let mut sorted: Vec<&Value> = bookmarks.iter().collect();
        sorted.sort_by_key(|b| (chapter_uid(b), time_i64(b.get("createTime"))));
        let mut groups: BTreeMap<i64, Vec<&Value>> = BTreeMap::new();
        for b in sorted {
            groups.entry(chapter_uid(b)).or_default().push(b);
        }
        for (uid, items) in &groups {
            let name = chapters.get(uid).cloned().unwrap_or_else(|| {
                if *uid != 0 {
                    format!("第 {} 章", uid)
                } else {
                    "未分章".to_string()
                }
            });
            lines.push(format!("### {}", name));
            lines.push(String::new());
            for b in items {
                let text = b.get("markText").and_then(|t| t.as_str()).unwrap_or("").trim();
                match fmt_time_value(b.get("createTime")) {
                    Some(t) => lines.push(format!("- {}  \n  `({})`", text, t)),
                    None => lines.push(format!("- {}", text)),
                }
            }
            lines.push(String::new());
        }
    }

    if !reviews.is_empty() {
        lines.push("## 我的想法".into());
        lines.push(String::new());
        // 结构: {reviewId, review: {abstract(划线原文), content(想法文字), createTime, ...}}
        let mut sorted: Vec<&Value> = reviews.iter().collect();
        sorted.sort_by_key(|r| review_time(r));
        for r in sorted {
            let inner = r.get("review").cloned().unwrap_or(Value::Null);
            let content = inner.get("content").and_then(|c| c.as_str()).unwrap_or("").trim();
            let original = inner.get("abstract").and_then(|c| c.as_str()).unwrap_or("").trim();
            let t = fmt_time_value(inner.get("createTime"));
            if !content.is_empty() {
                lines.push(match &t {
                    Some(t) => format!("- {}  \n  `({})`", content, t),
                    None => format!("- {}", content),
                });
                if !original.is_empty() {
                    lines.push(format!("  > 原文: {}", clip(original, 100)));
                }
            } else if !original.is_empty() {
                lines.push(match &t {
                    Some(t) => format!("- {}  \n  `({})`", clip(original, 100), t),
                    None => format!("- {}", clip(original, 100)),
                });
            }
        }
        lines.push(String::new());
    }

    if bookmarks.is_empty() && reviews.is_empty() {
        lines.push("(这本书没有可导出的划线或想法)".into());
    }
    lines.push(String::new());
    lines.join("\n")
}
