//! 服务器集成测试——全进程内：FakeGateway 注入 + tempdir SQLite + oneshot 打 router。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use weread_notes::client::{GatewayApi, GatewayError};
use weread_notes::server::config::ServerConfig;
use weread_notes::server::db;
use weread_notes::server::routes::build_app;
use weread_notes::server::state::AppState;
use weread_notes::server::worker;

// --------------------------------------------------------------------------- //
// FakeGateway
// --------------------------------------------------------------------------- //

/// 注入 fake 的测试 Key（请求头 X-Weread-Key）
const TEST_KEY: &str = "wrk-test-key";

struct FakeGateway {
    handler: Box<dyn Fn(&str, &Value) -> Result<Value, GatewayError> + Send + Sync>,
    calls: Mutex<Vec<String>>,
}

impl FakeGateway {
    fn new<F>(f: F) -> Self
    where
        F: Fn(&str, &Value) -> Result<Value, GatewayError> + Send + Sync + 'static,
    {
        Self {
            handler: Box::new(f),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn count(&self, api_name: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.as_str() == api_name)
            .count()
    }
}

#[async_trait::async_trait]
impl GatewayApi for FakeGateway {
    async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        self.calls.lock().unwrap().push(api_name.to_string());
        (self.handler)(api_name, &params)
    }
}

/// 默认 fake：覆盖常用接口，notebooks 两页（游标 lastSort）。
fn fake_ok(api: &str, params: &Value) -> Result<Value, GatewayError> {
    match api {
        "/user/notebooks" => {
            let last_sort = params.get("lastSort").and_then(|v| v.as_i64()).unwrap_or(0);
            if last_sort == 0 {
                Ok(json!({
                    "books": [
                        {"bookId": "B1", "sort": 100, "bookmarkCount": 3, "noteCount": 0, "reviewCount": 1,
                         "book": {"title": "书一", "author": "作者甲"}},
                        {"bookId": "B2", "sort": 90, "bookmarkCount": 2, "noteCount": 0, "reviewCount": 0,
                         "book": {"title": "书二", "author": "作者乙"}}
                    ],
                    "hasMore": 1, "totalBookCount": 3, "totalNoteCount": 7
                }))
            } else {
                Ok(json!({
                    "books": [
                        {"bookId": "B3", "sort": 80, "bookmarkCount": 1, "noteCount": 0, "reviewCount": 0,
                         "book": {"title": "书三"}}
                    ],
                    "hasMore": 0
                }))
            }
        }
        "/book/bookmarklist" => {
            let book_id = params.get("bookId").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!({
                "updated": [
                    {"chapterUid": 1, "markText": "划线甲", "createTime": 1700000000},
                    {"chapterUid": 1, "markText": "划线乙", "createTime": 1700000100},
                    {"chapterUid": 2, "markText": "划线丙", "createTime": 1700000200}
                ],
                "chapters": [{"chapterUid": 1, "title": "第一章"}, {"chapterUid": 2, "title": "第二章"}],
                "book": if book_id == "B2" { json!({}) } else {
                    json!({"title": format!("书{book_id}"), "author": "作者"})
                }
            }))
        }
        "/review/list/mine" => Ok(json!({
            "reviews": [
                {"reviewId": "r1", "review": {"content": "想法一", "abstract": "划线甲", "createTime": 1700000300}}
            ],
            "hasMore": 0
        })),
        "/shelf/sync" => Ok(json!({
            "books": [
                {"bookId": "B1", "title": "书一", "author": "作者甲", "progress": 42,
                 "finishReading": 0, "readUpdateTime": 1700001000},
                {"bookId": "B9", "title": "只书架的书", "author": "作者丙", "progress": 100,
                 "finishReading": 1, "readUpdateTime": 1700002000}
            ]
        })),
        "/book/info" => Ok(json!({"title": "书一", "author": "作者甲", "isbn": "9780000000000", "newRating": 900})),
        "/book/chapterinfo" => Ok(json!({"chapters": [{"chapterUid": 1, "title": "第一章"}]})),
        "/book/getprogress" => Ok(json!({"progress": 42, "readTime": 3600})),
        "/readdata/detail" => Ok(json!({
            "mode": params.get("mode").cloned().unwrap_or(json!("overall")),
            "totalReadTime": 99999
        })),
        "/store/search" => Ok(json!({"results": [{"title": "电子书", "books": [{"bookId": "S1", "title": "搜索结果书"}]}]})),
        "/_list" => Ok(json!({"apis": ["/user/notebooks"]})),
        _ => Err(GatewayError::Api { errcode: "-1".into(), message: format!("fake 未实现: {api}") }),
    }
}

// --------------------------------------------------------------------------- //
// 测试基建
// --------------------------------------------------------------------------- //

struct TestApp {
    app: axum::Router,
    state: AppState,
    gw: Arc<FakeGateway>,
    _dir: tempfile::TempDir,
}

fn test_config(dir: &std::path::Path) -> ServerConfig {
    ServerConfig {
        bind: "127.0.0.1:0".into(),
        data_dir: dir.to_path_buf(),
        gateway_url: "http://fake-gateway".into(),
        gateway_timeout_secs: 5,
        cors_origins: "".into(),
        cache_ttl_secs: 3600,
        gateway_min_interval_ms: 0,
        circuit_threshold: 5,
        circuit_cooldown_secs: 600,
        task_retention_days: 30,
        queue_limit: 1000,
        max_running_tasks: 4,
        rate_per_minute: 100_000,
    }
}

async fn setup() -> TestApp {
    setup_with(fake_ok).await
}

async fn setup_with<F>(handler: F) -> TestApp
where
    F: Fn(&str, &Value) -> Result<Value, GatewayError> + Send + Sync + 'static,
{
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join("test.db")).await.unwrap();
    let config = test_config(dir.path());
    let state = AppState::new(config, pool);
    let gw = Arc::new(FakeGateway::new(handler));
    // BYOK 注入：请求携带 X-Weread-Key: <TEST_KEY> 时命中 fake
    state.registry.set(TEST_KEY, gw.clone());
    let app = build_app(state.clone());
    TestApp { app, state, gw, _dir: dir }
}

fn req(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-weread-key", TEST_KEY)
        .body(Body::empty())
        .unwrap()
}

fn req_json(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-weread-key", TEST_KEY)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn req_nokey(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// 驱动 worker 直至队列排空（测试不起后台 worker，直接同步驱动）
async fn drain_worker(t: &TestApp) {
    while worker::process_next_due(&t.state).await.unwrap() {}
}

async fn create_sync(t: &TestApp, body: Value) -> (StatusCode, Value) {
    let resp = t
        .app
        .clone()
        .oneshot(req_json("POST", "/api/v1/syncs", body))
        .await
        .unwrap();
    let status = resp.status();
    (status, body_json(resp).await)
}

// --------------------------------------------------------------------------- //
// 基础与鉴权
// --------------------------------------------------------------------------- //

#[tokio::test]
async fn healthz_ok_without_auth() {
    let t = setup().await;
    let resp = t.app.clone().oneshot(req("GET", "/healthz")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn weread_key_enforced() {
    let t = setup().await;
    // 无 Key → 401
    let resp = t
        .app
        .clone()
        .oneshot(req_nokey("GET", "/api/v1/shelf"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("UNAUTHORIZED"));

    // ?key= 查询参数兜底（EventSource / 下载直链场景）→ 200
    let resp = t
        .app
        .clone()
        .oneshot(req_nokey("GET", &format!("/api/v1/shelf?key={TEST_KEY}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 其他 Key 未注入 fake：注册表为其建真实客户端打假地址 → 网络错误 502，
    // 证明网关客户端按 Key 解析、互不影响
    let resp = t
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/shelf")
                .header("x-weread-key", "wrk-other-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

// --------------------------------------------------------------------------- //
// 同步代理与缓存
// --------------------------------------------------------------------------- //

#[tokio::test]
async fn shelf_proxy_with_cache_and_refresh() {
    let t = setup().await;
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["fresh"], json!(true));
    assert_eq!(v["data"]["books"].as_array().unwrap().len(), 2);
    assert_eq!(t.gw.count("/shelf/sync"), 1);

    // TTL 内命中缓存，不再调网关
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(t.gw.count("/shelf/sync"), 1);

    // refresh 强制回源
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/shelf?refresh=true"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(t.gw.count("/shelf/sync"), 2);
}

#[tokio::test]
async fn notebooks_merges_pages() {
    let t = setup().await;
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/notebooks")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["data"]["books"].as_array().unwrap().len(), 3);
    assert_eq!(v["data"]["totalBookCount"], json!(3));
    assert_eq!(v["data"]["totalNoteCount"], json!(7));
    assert_eq!(t.gw.count("/user/notebooks"), 2);
}

#[tokio::test]
async fn book_info_cached_per_book() {
    let t = setup().await;
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/books/B1")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["data"]["title"], json!("书一"));
    // 再请求同书命中缓存；另一书是不同的缓存键
    let _ = t.app.clone().oneshot(req("GET", "/api/v1/books/B1")).await.unwrap();
    let _ = t.app.clone().oneshot(req("GET", "/api/v1/books/B2")).await.unwrap();
    assert_eq!(t.gw.count("/book/info"), 2);
}

#[tokio::test]
async fn readdata_validates_mode_and_passes_through() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/readdata?mode=annually"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["data"]["mode"], json!("annually"));

    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/readdata?mode=daily"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn book_notes_orchestration_with_title_fallback() {
    let t = setup().await;
    // B1：bookmarklist 自带书籍信息
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/books/B1/notes"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["data"]["book"]["title"], json!("书B1"));
    assert_eq!(v["data"]["bookmarks"].as_array().unwrap().len(), 3);
    assert_eq!(v["data"]["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(v["data"]["reviews"].as_array().unwrap().len(), 1);

    // B2：bookmarklist 缺 book → 从笔记本概览回查书名
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/books/B2/notes"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["data"]["book"]["title"], json!("书二"));
}

#[tokio::test]
async fn invalid_book_id_rejected() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/books/B1%2F..%2Fevil/notes"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn search_passthrough_without_cache() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/search?keyword=test&scope=10"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert!(v["data"]["results"].is_array() || v["results"].is_array());
    // 重复搜索不缓存
    let _ = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/search?keyword=test"))
        .await
        .unwrap();
    assert_eq!(t.gw.count("/store/search"), 2);
    // 缺 keyword → 400
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/search")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gateway_call_passthrough() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req_json(
            "POST",
            "/api/v1/gateway/call",
            json!({"api_name": "/book/getprogress", "params": {"bookId": "B1"}}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["progress"], json!(42));
    // 空 api_name → 400
    let resp = t
        .app
        .clone()
        .oneshot(req_json("POST", "/api/v1/gateway/call", json!({"api_name": "  "})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --------------------------------------------------------------------------- //
// 上游错误映射
// --------------------------------------------------------------------------- //

#[tokio::test]
async fn upstream_api_error_maps_502() {
    let t = setup_with(|_, _| {
        Err(GatewayError::Api {
            errcode: "-201014".into(),
            message: "无效的 bookId".into(),
        })
    })
    .await;
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("UPSTREAM_ERROR"));
    assert_eq!(v["error"]["errcode"], json!("-201014"));
}

#[tokio::test]
async fn skill_upgrade_maps_503() {
    // upgrade_info 的识别在真实 client 的 parse_response 里（fake 绕过协议层），
    // 这里直接以 GatewayError::Upgrade 模拟 client 已识别出的升级阻断
    let t = setup_with(|_, _| Err(GatewayError::Upgrade("请升级 skill 到 1.1.0".into()))).await;
    let resp = t.app.clone().oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("SKILL_UPGRADE_REQUIRED"));
}

#[tokio::test]
async fn upstream_failure_falls_back_to_stale_cache() {
    let t = setup().await;
    // 先成功缓存
    let _ = t.app.clone().oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(t.gw.count("/shelf/sync"), 1);

    // 手动把缓存置为过期，然后换一个总是失败的网关……
    // （FakeGateway 的 handler 不可变，改用直接过期缓存 + refresh 的方式验证降级路径：
    //   把 expires_ms 置 0 后 refresh=true 回源失败 → 旧值降级返回 fresh=false）
    sqlx::query("UPDATE cache_entries SET expires_ms = 0")
        .execute(&t.state.pool)
        .await
        .unwrap();

    // 构造一个网关故障的副本 app：直接复用 state（共享 DB 与缓存），
    // 覆盖注册表里该 Key 的网关实现
    t.state.registry.set(
        TEST_KEY,
        Arc::new(FakeGateway::new(|_, _| {
            Err(GatewayError::Network {
                message: "boom".into(),
                attempts: 3,
            })
        })),
    );
    let app2 = build_app(t.state.clone());
    let resp = app2.oneshot(req("GET", "/api/v1/shelf")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["fresh"], json!(false));
    assert_eq!(v["data"]["books"].as_array().unwrap().len(), 2);
}

// --------------------------------------------------------------------------- //
// 同步任务
// --------------------------------------------------------------------------- //

#[tokio::test]
async fn markdown_endpoint_removed() {
    let t = setup().await;
    // Markdown 导出功能已删除：路由不存在（axum 默认 404，无错误信封）
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/books/B1/notes/markdown"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sync_single_book_persists_raw_json() {
    let t = setup().await;
    let (status, v) = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(v["status"], json!("queued"));
    let task_id = v["task_id"].as_str().unwrap().to_string();

    drain_worker(&t).await;

    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["status"], json!("completed"));
    assert_eq!(v["percent"], json!(100));
    assert_eq!(v["progress"]["done_books"], json!(1));
    assert_eq!(v["items_count"], json!(4)); // 3 划线 + 1 想法

    // result 汇总（结果视图不再携带 markdown_url）
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}/result")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["summary"]["bookmarks"], json!(3));
    assert_eq!(v["summary"]["reviews"], json!(1));
    assert_eq!(v["books"][0]["state"], json!("done"));
    assert_eq!(v["books"][0]["title"], json!("书B1"));
    assert!(v["books"][0].get("markdown_url").is_none());

    // 原始 JSON 直接入库：书/章节/划线/想法全量同步（独立临时库，单 owner，按 book_id 查）
    let row: (String,) =
        sqlx::query_as("SELECT data_json FROM notes WHERE book_id = 'B1'")
            .fetch_one(&t.state.pool)
            .await
            .unwrap();
    let data: Value = serde_json::from_str(&row.0).unwrap();
    assert_eq!(data["book"]["title"], json!("书B1"));
    assert_eq!(data["chapters"].as_array().unwrap().len(), 2);
    let marks: Vec<&str> = data["bookmarks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["markText"].as_str().unwrap())
        .collect();
    assert_eq!(marks, vec!["划线甲", "划线乙", "划线丙"]);
    assert_eq!(data["reviews"][0]["review"]["content"], json!("想法一"));
}

#[tokio::test]
async fn sync_all_books_and_skip_cached_on_second_run() {
    let t = setup().await;
    let (status, v) = create_sync(&t, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(v["progress"]["total_books"], json!(3));
    let task_id = v["task_id"].as_str().unwrap().to_string();

    drain_worker(&t).await;

    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["status"], json!("completed"));

    let bm_calls = t.gw.count("/book/bookmarklist");

    // 第二次全量同步：全部命中已有 notes → 直接终态，不再回源
    let (status, v) = create_sync(&t, json!({})).await;
    assert_eq!(status, StatusCode::OK); // 立即完成
    assert_eq!(v["status"], json!("completed"));
    assert_eq!(v["skipped_cached"], json!(3));
    assert_eq!(t.gw.count("/book/bookmarklist"), bm_calls);

    // refresh=true 才会重抓
    let (status, v) = create_sync(&t, json!({"refresh": true})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(v["queued_books"], json!(3));
    drain_worker(&t).await;
    assert!(t.gw.count("/book/bookmarklist") > bm_calls);
}

#[tokio::test]
async fn partial_when_one_book_fails_permanently() {
    let t = setup_with(
        |api, params| {
            if api == "/book/bookmarklist"
                && params.get("bookId").and_then(|v| v.as_str()) == Some("B3")
            {
                return Err(GatewayError::Api {
                    errcode: "-201014".into(),
                    message: "无效的 bookId".into(),
                });
            }
            fake_ok(api, params)
        },
    )
    .await;

    let (status, v) = create_sync(&t, json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let task_id = v["task_id"].as_str().unwrap().to_string();

    drain_worker(&t).await;

    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["status"], json!("partial"));
    assert_eq!(v["progress"]["failed_books"], json!(1));
    let failed = v["error"]["failed_books"].as_array().unwrap();
    assert_eq!(failed[0]["book_id"], json!("B3"));

    // result 只含成功书
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}/result")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["summary"]["succeeded"], json!(2));
    assert_eq!(v["summary"]["failed"], json!(1));
}

#[tokio::test]
async fn transient_error_backs_off_then_succeeds() {
    let attempts = Arc::new(Mutex::new(0u32));
    let attempts_clone = attempts.clone();
    let t = setup_with(
        move |api, params| {
            if api == "/book/bookmarklist"
                && params.get("bookId").and_then(|v| v.as_str()) == Some("B1")
            {
                let mut n = attempts_clone.lock().unwrap();
                *n += 1;
                if *n == 1 {
                    return Err(GatewayError::Timeout(1));
                }
            }
            fake_ok(api, params)
        },
    )
    .await;

    let (_, v) = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    let task_id = v["task_id"].as_str().unwrap().to_string();

    // 第一轮：瞬时失败 → 退避（next_sync_ms 在未来，pick 不到）
    drain_worker(&t).await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["status"], json!("running"));
    assert_eq!(*attempts.lock().unwrap(), 1);

    // 模拟退避到期
    sqlx::query("UPDATE task_items SET next_sync_ms = 0")
        .execute(&t.state.pool)
        .await
        .unwrap();
    drain_worker(&t).await;

    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}")))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["status"], json!("completed"));
    assert_eq!(*attempts.lock().unwrap(), 2); // 第二次成功
}

#[tokio::test]
async fn wait_timeout_returns_terminal_when_all_skipped() {
    let t = setup().await;
    // 先同步一本书
    let _ = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    drain_worker(&t).await;

    // 再次同步同书：全部命中已有 notes，wait_timeout 内立即终态 → 200
    let (status, v) = create_sync(&t, json!({"book_ids": ["B1"], "wait_timeout": 5})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["status"], json!("completed"));
}

#[tokio::test]
async fn task_list_and_delete_keeps_synced_notes() {
    let t = setup().await;
    let (_, v1) = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    let (_, v2) = create_sync(&t, json!({"book_ids": ["B2"]})).await;
    drain_worker(&t).await;

    let resp = t.app.clone().oneshot(req("GET", "/api/v1/syncs?limit=10")).await.unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["tasks"].as_array().unwrap().len(), 2);

    let id1 = v1["task_id"].as_str().unwrap().to_string();
    let resp = t
        .app
        .clone()
        .oneshot(req("DELETE", &format!("/api/v1/syncs/{id1}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 删除后查询 404
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{id1}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 数据保留：notes 表仍在
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes WHERE book_id = 'B1'")
        .fetch_one(&t.state.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let _ = v2;
}

#[tokio::test]
async fn result_before_finish_is_409() {
    let t = setup().await;
    let (_, v) = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    let task_id = v["task_id"].as_str().unwrap().to_string();
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}/result")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], json!("TASK_NOT_FINISHED"));
}

#[tokio::test]
async fn sse_emits_done_for_terminal_task() {
    let t = setup().await;
    let (_, v) = create_sync(&t, json!({"book_ids": ["B1"]})).await;
    let task_id = v["task_id"].as_str().unwrap().to_string();
    drain_worker(&t).await;

    // 终态任务：连接即推快照（对终态而言即 done 事件本身，携带完整任务视图）后关流
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", &format!("/api/v1/syncs/{task_id}/events")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let text = body_text(resp).await;
    assert!(text.contains("event: done"), "终态应推 done 并关流: {text}");
    assert!(text.contains(&task_id));
    assert!(text.contains("\"percent\":100"));
}

#[tokio::test]
async fn unknown_task_404() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/syncs/wsync_nonexistent"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_json(resp).await["error"]["code"],
        json!("TASK_NOT_FOUND")
    );
}

#[tokio::test]
async fn empty_book_ids_array_rejected() {
    let t = setup().await;
    let (status, v) = create_sync(&t, json!({"book_ids": []})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], json!("INVALID_ARGUMENT"));
}

#[tokio::test]
async fn server_info_shape() {
    let t = setup().await;
    let resp = t
        .app
        .clone()
        .oneshot(req("GET", "/api/v1/server/info"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["name"], json!("weread-notes-server"));
    assert_eq!(v["key_mode"], json!("byok"));
    assert_eq!(v["gateway"]["skill_version"], json!("1.0.5"));
    // BYOK：服务端不持有 Key，info 不回显任何 Key 信息
    assert!(v["gateway"].get("api_key").is_none());
}
