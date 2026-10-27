use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use crate::client::{GatewayError, WereadClient};
use crate::markdown;

fn as_i64(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64()).unwrap_or(0)
}

/// bookId 兼容字符串与数字两种返回形态。
fn id_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => "?".into(),
    }
}

/// 翻页拉全 /user/notebooks(游标为最后一条的 sort 字段)。
pub fn fetch_notebooks(client: &WereadClient, count: u64) -> Result<Vec<Value>, GatewayError> {
    let mut items = Vec::new();
    let mut last_sort: Option<i64> = None;
    for _ in 0..20 {
        let mut params = json!({ "count": count });
        if let Some(ls) = last_sort {
            params["lastSort"] = json!(ls);
        }
        let data = client.call("/user/notebooks", params)?;
        let empty = Vec::new();
        let books = data.get("books").and_then(|v| v.as_array()).unwrap_or(&empty);
        let page_len = books.len();
        items.extend(books.iter().cloned());
        let has_more = as_i64(data.get("hasMore")) == 1;
        if !has_more || page_len == 0 {
            break;
        }
        last_sort = books[page_len - 1].get("sort").and_then(|v| v.as_i64());
        if last_sort.is_none() {
            break;
        }
    }
    Ok(items)
}

pub fn cmd_notebooks(
    client: &WereadClient,
    limit: usize,
    page_size: u64,
) -> Result<(), GatewayError> {
    let items = fetch_notebooks(client, page_size)?;
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
        let total = as_i64(nb.get("bookmarkCount"))
            + as_i64(nb.get("noteCount"))
            + as_i64(nb.get("reviewCount"));
        std::cmp::Reverse(total)
    });
    let shown = rows.len().min(limit);
    for (i, nb) in rows.iter().take(shown).enumerate() {
        let book = nb.get("book").unwrap_or(&Value::Null);
        let bm = as_i64(nb.get("bookmarkCount"));
        let nt = as_i64(nb.get("noteCount"));
        let rv = as_i64(nb.get("reviewCount"));
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
        let time = upd.and_then(|v| v.as_i64()).and_then(markdown::fmt_time).unwrap_or_default();
        println!(
            "{:<4} {:<14} {:<34} {:<10} {:<8} {:<8} {}",
            i + 1,
            id_str(nb.get("bookId")),
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

/// 一次 /book/bookmarklist 拿全: 划线列表(updated) + 章节表(chapters) + 书籍信息(book)。
fn fetch_note_detail(
    client: &WereadClient,
    book_id: &str,
) -> Result<(Vec<Value>, BTreeMap<i64, String>, Value), GatewayError> {
    let data = client.call("/book/bookmarklist", json!({ "bookId": book_id }))?;
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
    Ok((bookmarks, chapters, book))
}

/// 我的想法(/review/list/mine, 注意参数是小写 bookid), synckey 翻页。失败不影响划线导出。
pub fn fetch_my_reviews(client: &WereadClient, book_id: &str) -> Vec<Value> {
    let mut reviews = Vec::new();
    let mut synckey: Option<i64> = None;
    for _ in 0..20 {
        let mut params = json!({ "bookid": book_id, "count": 100 });
        if let Some(sk) = synckey {
            params["synckey"] = json!(sk);
        }
        let data = match client.call("/review/list/mine", params) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("(!) 想法列表获取失败, 已跳过: {}", e);
                return reviews;
            }
        };
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
    reviews
}

pub fn cmd_export(
    client: &WereadClient,
    book_id: &str,
    output: Option<&str>,
) -> Result<(), GatewayError> {
    let (bookmarks, chapters, mut book_info) = fetch_note_detail(client, book_id)?;
    let need_fallback = book_info
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.is_empty())
        .unwrap_or(true);
    if need_fallback {
        eprintln!("(!) 划线响应缺少书籍信息, 从笔记本概览回查。");
        for nb in fetch_notebooks(client, 100)? {
            if id_str(nb.get("bookId")) == book_id {
                if let Some(b) = nb.get("book") {
                    book_info = b.clone();
                }
                break;
            }
        }
    }
    let reviews = fetch_my_reviews(client, book_id);
    let md = markdown::build_markdown(book_id, &book_info, &chapters, &bookmarks, &reviews);

    match output {
        Some(path) => {
            std::fs::write(path, md).map_err(|e| GatewayError::Io(format!("写入 {} 失败: {}", path, e)))?;
            println!(
                "已导出 {} 条划线 + {} 条想法 -> {}",
                bookmarks.len(),
                reviews.len(),
                path
            );
        }
        None => println!("{}", md),
    }
    Ok(())
}

fn smart_value(s: &str) -> Value {
    let t = s.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = t.parse::<i64>() {
            return json!(n);
        }
    }
    match t {
        "true" => json!(true),
        "false" => json!(false),
        _ => Value::String(t.to_string()),
    }
}

pub fn cmd_raw(
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
    let data = client.call(api_name, Value::Object(body))?;
    let pretty = serde_json::to_string_pretty(&data)
        .map_err(|e| GatewayError::InvalidJson(e.to_string()))?;
    println!("{}", pretty);
    Ok(())
}

pub fn cmd_apis(client: &WereadClient) -> Result<(), GatewayError> {
    match client.call("/_list", json!({})) {
        Ok(data) => {
            let pretty = serde_json::to_string_pretty(&data)
                .map_err(|e| GatewayError::InvalidJson(e.to_string()))?;
            println!("{}", pretty);
        }
        Err(e) => eprintln!("(/_list 不可用: {})", e),
    }
    Ok(())
}
