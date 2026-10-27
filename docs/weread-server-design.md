# weread-notes 服务器化设计方案（v1）

> 状态：**已实现**（v0.1.0，含 25 个 oneshot 集成测试 + 真机冒烟）
> 日期：2026-10-27（设计稿）/ 2026-10-27（实施完成）
> 范本：`~/workspace/douban-api`（axum + tokio + sqlx/SQLite + DB 即队列的 worker 模式，已生产验证）
> 前置阅读：[weread-api-capabilities.md](weread-api-capabilities.md)（官方网关 17 接口清单与能力边界）

---

## 目录

- [0. 目标与约束——与 douban-api 的关键差异](#0-目标与约束与-douban-api-的关键差异)
- [1. 总体架构](#1-总体架构)
- [2. 核心设计决策](#2-核心设计决策)
- [3. API 一览](#3-api-一览)
- [4. 接口详细设计](#4-接口详细设计)
- [5. 数据模型](#5-数据模型)
- [6. 任务状态机](#6-任务状态机)
- [7. 缓存与数据保留](#7-缓存与数据保留)
- [8. 限速与防护](#8-限速与防护)
- [9. 错误响应规范](#9-错误响应规范)
- [10. 认证与安全](#10-认证与安全)
- [11. 前端设计（web/）](#11-前端设计web)
- [12. 代码组织与依赖增量](#12-代码组织与依赖增量)
- [13. 测试策略](#13-测试策略)
- [14. 非目标（v1 明确不做）](#14-非目标v1-明确不做)
- [15. 实施步骤](#15-实施步骤)
- [16. 开放问题](#16-开放问题)

---

## 0. 目标与约束——与 douban-api 的关键差异

把现有 CLI（`weread-notes`）的能力服务化：**通过官方 Agent Gateway 代理/导出本人微信读书数据（书架、笔记、统计），并提供管理前端**。

douban-api 的架构是围绕「慢速 HTML 爬虫」设计的；weread 的上游是**官方 JSON API**，问题域不同，**不能照抄，要按差异重新推导**：

| 维度 | douban-api | weread-api | 对本设计的直接影响 |
|---|---|---|---|
| 上游 | 豆瓣公开页 HTML 抓取 | 官方 Agent Gateway（JSON，Bearer `wrk-` Key） | 无需 HTML 解析；错误是结构化 errcode |
| 上游速度 | 全量 ≥1 分钟（20 页 × ≥2s 限速） | 单接口毫秒~秒级 | **大部分接口可同步代理**，只有全量导出需要异步任务 |
| 上游限速 | 豆瓣反爬，超速封 IP | 官方未公布限额，宽松但未知 | 节流从「生存前提」降级为「礼貌保守」 |
| 认证 | 免登录（可选 Cookie） | 上游 Key = 本人身份 | **单用户服务**；无 uid 维度；两层凭据模型 |
| 数据范围 | 任意公开用户 | 仅 Key 持有者本人 + 公开社区内容 | 缓存键不含 uid；无多租户 |
| 数据形态 | 扁平 12 字段记录 | 嵌套 JSON（书/章节/划线/想法） | 持久化以「导出产物（Markdown）+ 书籍元数据」为主 |
| 写操作 | 无 | 无（官方网关全只读） | 同 douban，纯读服务 |

三条硬约束：

1. **API Key 必须只留在服务端**——前端/客户端永不接触上游 Key（它是本人全部阅读数据的钥匙）
2. **全量导出天然是长操作**——N 本书 × 2~3 次网关调用（300 本 ≈ 千次调用，分钟级），必须异步任务化
3. **官方网关限额未知**——所有上游调用必须经过单一节流点，可配置、可观察

## 1. 总体架构

```
浏览器(web/) ──HTTP──▶ API 层（axum）
                        │  /shelf /notebooks /readdata...  ──▶ TTL 缓存命中? ──▶ 直接返回
                        │  /exports(POST)                  ──▶ 创建异步任务
                        ▼
                  服务层 service
                        │  缓存未命中/refresh ──▶ GatewayClient(async, 全局节流)
                        ▼
                  微信读书 Agent Gateway (i.weread.qq.com, Bearer wrk-...)
                        
                  导出 worker（单 worker 串行，DB 即队列，watch 唤醒）
                        │  逐书: bookmarklist + review/list/mine 翻页 → 入库 + 生成 Markdown
                        ▼
                  SQLite: cache_entries / tasks / export_items / books / exports
```

组件职责（对照 douban-api）：

- **API 层**（routes.rs）：参数校验、双层鉴权、per-IP 限流、SSE
- **服务层**（service.rs）：同步代理 + 缓存判定、任务 CRUD 与视图聚合
- **GatewayClient**（gateway.rs，对应 douban 的 fetcher.rs）：async trait，隔离上游 HTTP；生产 reqwest 实现 + 测试 fake
- **Worker**（worker.rs）：DB 即队列（export_items 表），单 worker 串行节流
- **存储**（db.rs）：SQLite 单库——缓存表 + 任务表 + 长期数据表

## 2. 核心设计决策

### D1：~~单用户 + 两层凭据模型~~ → **BYOK（2026-10-27 变更，按用户要求）**

原设计：上游 `WEREAD_API_KEY` 配在服务端 + 独立访问 token `WEREAD_SERVER_TOKEN` 两层分离。

**现实现**：前端必填微信读书 Key（手动输入，存浏览器 localStorage），逐请求经 `X-Weread-Key` 头（SSE/下载直链走 `?key=`）传给后端；**服务器不读取任何默认 Key**，启动零凭据。配套改造：

- `GatewayRegistry` 按 Key 解析并缓存网关客户端；节流锁全局共享（跨所有 Key 恒低速）
- 存储按 Key 指纹（sha256 前 8 hex）隔离：cache key 前缀、books/exports 复合主键 (owner, book_id)
- tasks 表存原始 Key（本地个人工具可接受）供 worker 续跑；任务查询/删除按 Key 过滤
- 单用户假设弱化为「同时可能多 Key」——数据隔离已处理，限流/熔断仍全局

### D2：混合模型——同步代理 + 异步导出（本方案与 douban 最大的分野）

- **同步代理**：书架、笔记本概览、书籍详情/目录/进度、阅读统计、单书笔记、搜索——这些上游单调用（或 2~3 次）秒级返回的接口，直接同步 GET + SQLite TTL 缓存（`?refresh=true` 强制回源）。**不任务化**——douban 任务化的原因是豆瓣慢，这里没有这个约束
- **异步导出**：只有「批量导出多本书的笔记（Markdown）」走任务模型（POST /exports → 202 → 轮询/SSE）。单书导出 = 单书任务 + `wait_timeout` 同步逃生门（douban 已验证的模式），前端「导出此书」按钮秒级拿到结果
- 理由：任务模型的全套基础设施（队列、worker、状态机、SSE）只为长操作支付；短操作走代理保持 API 简单直接

### D3：持久化双轨——缓存（可再生）vs 导出产物（长期）

- `cache_entries`：代理响应的原样 JSON，TTL 过期即失效，可随时清空重建，**不是资产**
- `exports`：导出任务的产物（Markdown 文本 + 计数 + 元数据），长期保留、脱网可用，**是资产**
- `books`：shelf/notebooks 代理调用时顺手 upsert 的书籍元数据汇总（跨两个数据源的联查视图），全量导出时补全
- 划线/想法的**正规化明细表 v1 不做**（见非目标）：单书笔记浏览走代理缓存，历史回看走 exports 的 Markdown

### D4：client 全面 async 化 + trait 化

- 现有 `WereadClient`（blocking）改造为 async（reqwest::Client），CLI 改 `#[tokio::main]`，**CLI 与 server 共享同一实现**
- `GatewayApi` async trait（`call(api_name, params) -> Result<Value, GatewayError>`）+ `FakeGateway`（测试注入构造 JSON，对应 douban 的 fixture fetcher）
- `GatewayError` 升级为 thiserror 枚举，新增 `is_transient()` 分类：Network/Timeout/HTTP 499 → transient（退避重试）；errcode 业务错误（无效 bookId 等）→ permanent（快速失败）
- 现有编排逻辑（`fetch_notebooks` 翻页、`fetch_note_detail`、`fetch_my_reviews`）从 commands.rs 提为 lib 的 `fetch.rs`，CLI 与 server worker 共用——对应 douban 的 crawler.rs 共享

### D5：简化版 worker——保留骨架，去掉爬虫特有的重型机制

照搬 douban 验证过的骨架：**DB 即队列**（export_items 表 = 队列单位）、单 worker 串行（天然全局节流）、watch channel 即时唤醒 + 60s 兜底扫描、broadcast 通道驱动 SSE。

按 weread 实情裁剪：

| douban 机制 | weread 取舍 | 理由 |
|---|---|---|
| 维度缓存 (uid,cat,status) 跨任务复用 | ❌ 不做 | 单用户；重抓单书成本低（2~3 调用） |
| single-flight 同维度去重 | ❌ 不做 | 网关快，穿透代价小；per-IP 限流 + TTL 缓存兜底 |
| 断点续爬 checkpoint | ❌ 不做 | 单书调用少，整书重试即可 |
| 全局令牌桶（生存前提） | ✅ 简化保留：调用间隔 ≥500ms（可配） | 限额未知，保守礼貌 |
| 熔断（连续失败全局暂停） | ✅ 保留 | 成本低，防 Key 失效/网关故障时疯狂重试 |
| 指数退避重试 | ✅ 保留 | 沿用 client 现有 3 次重试 + worker 书级退避 |

### D6：进度模型比 douban 简单

douban 有页面级进度（fetched/total/page）。weread 的 `bookmarklist` 一次返回全量、`review/list/mine` 翻页无 total——书内进度不可知也不重要。**进度单位 = 书**：`fetched_books / total_books`，书内不再细分。前端展示「第 N/总 M 本」。

## 3. API 一览

Base URL：`http://{host}:8081/api/v1`（默认端口 8081，避免与 douban 的 8080 冲突；可配置）

| # | 方法 | 路径 | 上游接口 | 缓存 | 用途 |
|---|---|---|---|---|---|
| 1 | GET | `/shelf` | `/shelf/sync` | TTL | 书架全量（书/听书/公众号/书单） |
| 2 | GET | `/notebooks` | `/user/notebooks` | TTL | 笔记本概览（服务端翻页合并） |
| 3 | GET | `/books/{bookId}` | `/book/info` | TTL | 书籍详情 |
| 4 | GET | `/books/{bookId}/chapters` | `/book/chapterinfo` | TTL | 章节目录 |
| 5 | GET | `/books/{bookId}/progress` | `/book/getprogress` | TTL | 单书进度 |
| 6 | GET | `/books/{bookId}/notes` | `/book/bookmarklist` + `/review/list/mine` | TTL | 单书划线+想法（编排合并） |
| 7 | GET | `/books/{bookId}/notes/markdown` | — | 长期 | 已导出的 Markdown（仅读 exports 表） |
| 8 | GET | `/readdata?mode=&baseTime=` | `/readdata/detail` | TTL | 阅读统计 |
| 9 | GET | `/search?keyword=&scope=&count=` | `/store/search` | 不缓存 | 书城搜索（免登录接口） |
| 10 | GET | `/gateway/apis` | `/_list` | TTL 24h | 网关接口自描述 |
| 11 | POST | `/gateway/call` | 任意 api_name | 不缓存 | 逃生门：透传任意网关调用 |
| 12 | POST | `/exports` | — | — | 创建导出任务（核心入口） |
| 13 | GET | `/exports/{task_id}` | — | — | 任务状态与进度 |
| 14 | GET | `/exports` | — | — | 任务列表 |
| 15 | DELETE | `/exports/{task_id}` | — | — | 删除任务（产物保留） |
| 16 | GET | `/exports/{task_id}/result` | — | — | 结果汇总 |
| 17 | GET | `/exports/{task_id}/events` | — | — | 进度 SSE 推流 |
| 18 | GET | `/server/info` | — | — | 服务元信息（含网关连通性/升级提示） |
| 19 | GET | `/healthz` | — | — | 健康检查（前缀外，不鉴权） |

## 4. 接口详细设计

### 4.1 同步代理的统一约定

所有代理端点（#1~#6, #8, #10）响应信封：

```json
{
  "cached_at": "2026-10-27T10:00:00+08:00",
  "fresh": true,
  "data": { ...上游原样 JSON... }
}
```

- `fresh`：fetched_at 仍在 TTL 内；过期后**后台仍可读旧值**（stale-while-revalidate 不做，v1 过期即回源阻塞返回）
- `?refresh=true`：跳过 TTL 强制回源并刷新缓存
- `data` 原样透传上游 JSON——前端按官方结构取字段，服务端不重新发明数据形状
- 上游失败的错误码映射见第 9 节

`/notebooks` 与 `/books/{id}/notes` 是**编排端点**（服务端翻页/合并多接口），`data` 形状在服务端定义（ notebooks 为 books 数组；notes 为 `{book, chapters, bookmarks, reviews}`）。

### 4.2 创建导出任务

```
POST /api/v1/exports
{ "book_ids": ["3300028061"], "refresh": false, "wait_timeout": 0 }
```

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `book_ids` | string[] | 空 = 全部有笔记的书 | 缺省时服务端先拉 notebooks 全量再入队 |
| `refresh` | bool | false | true 时已导出的书也重抓重生成 |
| `wait_timeout` | int | 0 | 0–30s；单书导出建议 30（秒级完成，直接拿终态） |

响应 `202`（或 wait_timeout 内完成返回 `200` 终态）：

```json
{
  "task_id": "wexp_01JD6Z8W4R9TQK5M2VX3YB7C8D",
  "status": "queued",
  "total_books": 128,
  "skipped_cached": 120,
  "created_at": "2026-10-27T10:00:00+08:00",
  "url": "/api/v1/exports/wexp_..."
}
```

`refresh=false` 时已导出且未过期的书直接标记完成（`skipped_cached`），只抓增量——这是 douban「维度缓存秒回」在单用户场景的对应物。

### 4.3 查询任务 / SSE / 结果

```json
GET /api/v1/exports/{task_id}
{
  "task_id": "wexp_...",
  "status": "running",
  "progress": { "done_books": 45, "total_books": 128, "percent": 35,
                "current": {"book_id": "3300028061", "title": "..."} },
  "items_count": 5230,
  "error": null,
  "created_at": "...", "started_at": "...", "finished_at": null,
  "result_url": "/api/v1/exports/wexp_.../result"
}
```

- SSE（`/events`）与任务列表/删除语义与 douban 完全一致：先推快照、仅变更推送、`?token=` 兜底鉴权、15s 心跳；删除任务不删 exports 产物
- `/result` 返回汇总：

```json
{
  "task_id": "wexp_...",
  "summary": { "books": 128, "bookmarks": 5230, "reviews": 412 },
  "books": [ {"book_id": "...", "title": "...", "bookmarks": 40, "reviews": 3,
              "markdown_url": "/api/v1/books/.../notes/markdown"} ]
}
```

### 4.4 Markdown 端点

```
GET /api/v1/books/{bookId}/notes/markdown
```

只读 exports 表；未导出返回 `404 EXPORT_NOT_FOUND` + hint 引导 `POST /exports {"book_ids":["..."],"wait_timeout":30}`（沿用 douban「只读缓存 + 404 引导」决策，不让浏览端点隐式触发网关风暴）。`?download=1` 时加 `Content-Disposition` 作为文件下载。

## 5. 数据模型

```sql
-- 代理缓存（可再生，非资产）
CREATE TABLE cache_entries (
  cache_key  TEXT PRIMARY KEY,     -- 如 "shelf/sync", "notebooks", "book/info:{id}", "readdata:overall"
  payload    TEXT NOT NULL,        -- 上游原样 JSON（编排端点存合成后的 JSON）
  fetched_ms INTEGER NOT NULL,
  expires_ms INTEGER NOT NULL
);

-- 书籍元数据汇总（shelf/notebooks 代理时顺手 upsert；跨源联查视图）
CREATE TABLE books (
  book_id        TEXT PRIMARY KEY,
  title TEXT NOT NULL DEFAULT '', author TEXT NOT NULL DEFAULT '', cover TEXT NOT NULL DEFAULT '',
  isbn TEXT, word_count INTEGER, rating INTEGER,
  progress INTEGER,               -- 阅读进度（0-100）
  finish_reading INTEGER NOT NULL DEFAULT 0,
  read_update_time INTEGER,       -- 最近阅读时间（书架侧）
  bookmark_count INTEGER, note_count INTEGER, review_count INTEGER,  -- 笔记本侧
  updated_ms INTEGER NOT NULL
);

-- 导出任务
CREATE TABLE tasks (
  task_id     TEXT PRIMARY KEY,   -- wexp_ + 时间有序随机后缀
  request_json TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'queued',   -- queued/running/completed/partial/failed
  items_count INTEGER NOT NULL DEFAULT 0,       -- 累计划线+想法条数
  error_json  TEXT,
  created_ms INTEGER NOT NULL, started_ms INTEGER, finished_ms INTEGER,
  expires_ms INTEGER                            -- 任务记录保留 30d（产物不删）
);

-- 导出队列单位 = 书（DB 即队列；state 为队列状态机）
CREATE TABLE export_items (
  task_id    TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
  book_id    TEXT NOT NULL,
  state      TEXT NOT NULL DEFAULT 'waiting',   -- waiting/running/done/skipped/error
  bookmarks  INTEGER, reviews INTEGER,
  error      TEXT,
  attempts   INTEGER NOT NULL DEFAULT 0,
  next_sync_ms INTEGER,             -- 重试退避到期时间
  updated_ms INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (task_id, book_id)
);
CREATE INDEX idx_export_items_due ON export_items(state, next_sync_ms);

-- 导出产物（长期资产）
CREATE TABLE exports (
  book_id     TEXT PRIMARY KEY,
  markdown    TEXT NOT NULL,
  book_json   TEXT NOT NULL,        -- 书籍元数据快照（渲染 Markdown 所需）
  bookmarks   INTEGER NOT NULL DEFAULT 0,
  reviews     INTEGER NOT NULL DEFAULT 0,
  exported_ms INTEGER NOT NULL
);
```

## 6. 任务状态机

```
POST /exports ──▶ queued ──(全部书命中缓存)──▶ completed
                    │ worker 取出
                    ▼
                 running ──全部书 done/skipped──▶ completed
                    │  任一书 error ───────────▶ partial（成功书的产物已入库）
                    │  全部书 error ───────────▶ failed
```

书级（export_items.state）：`waiting → running → done | skipped(缓存命中) | error`（attempts 超限或 permanent 错误）。书级重试：transient 错误退避重试（20s×n + 抖动，上限 3 次尝试），超限标记 error 不阻塞其他书。

## 7. 缓存与数据保留

| 项 | 默认 | 说明 |
|---|---|---|
| 代理缓存 TTL | 3600s（1h） | 网关回源便宜，TTL 比 douban 的 24h 短；`/gateway/apis` 单独 24h；搜索不缓存 |
| 导出产物 | 永久 | exports 表不随任务过期清理；`refresh` 或带 `refresh:true` 重导时整书重写 |
| 任务记录 | 30 天 | 后台清理循环（douban 的 run_cleanup_loop 照搬） |
| 失败不缓存 | — | 上游错误不写 cache_entries，下次请求直接重试 |

## 8. 限速与防护

1. **客户端**：per-IP 固定窗口限流，默认 60 req/min（douban 同款中间件）
2. **任务**：全局运行中任务 ≤ 4，队列书数 ≤ 1000
3. **网关侧**：所有上游调用收敛到 GatewayClient 单点，最小间隔 500ms（可配 `WEREAD_GATEWAY_MIN_INTERVAL_MS`）+ 0~200ms 抖动；单 worker 串行天然全局串行
4. **熔断**：书级连续失败 ≥5 → 暂停 worker 10 分钟（Key 失效/网关故障的保险丝），`/server/info` 可见

## 9. 错误响应规范

信封与 douban 一致：`{"error": {"code", "message", ...}}`。

| HTTP | code | 场景 |
|---|---|---|
| 400 | `INVALID_ARGUMENT` | bookId 非法 / 枚举值错误 / 分页越界 |
| 401 | `UNAUTHORIZED` | 访问 token 缺失/错误 |
| 404 | `TASK_NOT_FOUND` / `EXPORT_NOT_FOUND` | 资源不存在 |
| 409 | `TASK_NOT_FINISHED` | 未终态取 result |
| 429 | `TOO_MANY_REQUESTS` / `TOO_MANY_TASKS` | 限流 / 队列满 |
| 500 | `INTERNAL` | 未预期错误 |
| 502 | `UPSTREAM_ERROR` | 网关 errcode ≠ 0（附 errcode/message） |
| 502 | `UPSTREAM_TIMEOUT` / `UPSTREAM_UNAVAILABLE` | 网关超时（HTTP 499/网络） |
| 503 | `SKILL_UPGRADE_REQUIRED` | 官方要求升级 skill_version（附提示） |
| 503 | `CIRCUIT_OPEN` | 熔断期间 |

上游业务错误**不吞**：errcode 与 message 原样附在 502 响应体里，便于排查。

## 10. 认证与安全（2026-10-27 起 BYOK）

- **凭据 = 微信读书 API Key 本身**：所有 `/api/v1` 端点要求 `X-Weread-Key` 头或 `?key=` 参数（EventSource / 下载直链兜底）；缺失 401，响应体附获取页面引导
- **服务器零默认凭据**：不读任何环境变量 Key，启动无需配置；Key 只活在请求与浏览器 localStorage
- tasks 表存原始 Key 供 worker 异步续跑（本地 SQLite，与 `~/.zshrc` 同信任域）；cache/books/exports 只存指纹
- Key 有效性不做事前校验——首次网关调用时由官方网关判定（errcode -2013 映射 502）
- `POST /gateway/call` 逃生门透传任意只读调用；公网部署需自行评估（任何人都能用自己的 Key 使用本服务）
- CORS：默认关闭，`WEREAD_SERVER_CORS_ORIGINS` 配置

## 11. 前端设计（web/）

完全复用 douban-api 的 web 骨架：Vite + React 18 + TypeScript + antd 5 + ProComponents（ProLayout）+ pnpm；`api.ts`（localStorage 存 apiBase/token）、连接设置 Popover、SSE 订阅模式照搬；vite proxy 指向 :8081。

页面（4 菜单）：

1. **概览** `/`
   - 阅读统计卡片：readdata overall（总时长/天数/日均/最爱作者/最长纪录/勋章）
   - 最近在读：shelf 按 readUpdateTime 倒序前 N（封面、进度条）
   - 笔记汇总：notebooks 的 totalBookCount / totalNoteCount
2. **书架** `/shelf`
   - 表格：封面缩略图、书名/作者、进度（progress bar，100% 标记已读完）、最近阅读时间；tab 切换 书/听书专辑/书单
   - 行点击 → 抽屉：书籍详情（/books/{id}）+ 当前进度 + 章节目录（折叠面板）
3. **笔记** `/notes`
   - 左：笔记本列表（书名、划线/笔记/想法计数、最近更新，倒序）
   - 右（选中书）：按章节分组的划线与想法时间线（引用块样式区分划线/想法）；已导出的书显示「已导出」标记 + Markdown 下载按钮
   - 「导出此书」按钮 → POST /exports（wait_timeout=30）→ 完成后行内提示 + 下载
   - 「全量导出」按钮 → 跳转导出页创建任务
4. **导出任务** `/exports`
   - 任务列表（状态 tag、进度条、书数/划线数、时间）；创建任务弹窗（全部/指定书、refresh 开关）
   - 任务详情抽屉：SSE 实时进度（douban TaskDetailDrawer 同款模式）、每本书状态列表、完成后按书下载 Markdown
5. 顶栏「连接」设置（apiBase + token，照抄 douban）

## 12. 代码组织与依赖增量

```
weread-api/
├── Cargo.toml              # lib + 双 bin
├── src/
│   ├── lib.rs              # pub mod client/fetch/markdown/server
│   ├── client.rs           # WereadClient 全面 async 化；GatewayError → thiserror + transient 分类
│   ├── fetch.rs            # 编排（现 commands.rs 的翻页逻辑提出）：notebooks/bookmarklist/reviews
│   ├── markdown.rs         # 保留
│   ├── server/
│   │   ├── mod.rs          # run_server 装配（thin main）
│   │   ├── config.rs       # ServerConfig（clap derive + env）
│   │   ├── error.rs        # AppError → IntoResponse 信封
│   │   ├── gateway.rs      # GatewayApi trait + ReqwestGateway + FakeGateway
│   │   ├── db.rs           # SQLite bootstrap（内嵌 SQL，不用 sqlx migrate）
│   │   ├── state.rs        # AppState：wakeup watch / progress broadcast / 熔断 / 限流
│   │   ├── service.rs      # 代理缓存、任务 CRUD、视图聚合、清理循环
│   │   ├── worker.rs       # 导出 worker（泛型 F: GatewayApi）
│   │   └── routes.rs       # axum 路由 + 中间件 + SSE
│   └── bin/
│       └── weread-notes.rs # 现 main.rs 移入，#[tokio::main]，命令不变
├── tests/server_tests.rs   # oneshot + FakeGateway + tempdir
└── web/                    # 见第 11 节
```

bin 命名：`weread-notes`（CLI，保留）/ `weread-notes-server`（新增）。

依赖新增（对齐 douban 的生产版本组合）：`axum 0.8`、`tokio(full)`、`sqlx 0.8(sqlite)`、`tower-http(cors)`、`thiserror`、`tracing` + `tracing-subscriber`、`tokio-stream`、`rand`；`reqwest` 去 `blocking` feature 转 async；dev：`tempfile`、`tower`。

## 13. 测试策略

douban 同款——全进程内，无 testcontainer：

- `FakeGateway`：按 api_name 返回构造 JSON（notebooks 两页、bookmarklist 含 chapters、review 翻页、errcode 错误、超时），并**计数调用次数**（验证缓存命中与节流）
- oneshot 打 router + tempdir SQLite：
  - 代理端点：首次回源、TTL 内命中缓存（fake 计数不变）、refresh 强制回源
  - 任务：多书 partial/failed 状态机、缓存命中跳过（skipped_cached）、wait_timeout 同步返回
  - Markdown：导出后 404 → 200、下载头
  - SSE：快照 + 终态 done
  - 鉴权/限流中间件
- CLI 回归：改造后四个子命令行为不变

## 14. 非目标（v1 明确不做）

- ❌ 划线/想法正规化明细表与跨书时间线浏览（v2 候选：做「全部划线按时间线」页面时再加）
- ❌ synckey 增量同步（v1 全量重抓；300 本书分钟级，可接受；定时同步是 v2 主题）
- ❌ 多 API Key / 多用户（单 Key 单用户；表结构预留演化空间）
- ❌ 公开社区内容（书评/热门划线/好友动态）与推荐流的**专用端点**——v1 经 `POST /gateway/call` 逃生门可用；前端页面化留给 v2
- ❌ 写操作、正文获取——官方网关根本不支持
- ❌ 部署打包（Docker 等）——先本地/内网可用

## 15. 实施步骤

| 阶段 | 内容 | 验证 |
|---|---|---|
| 1 | client async 化 + GatewayError thiserror 化 + lib 重组（fetch.rs 提出）+ CLI 迁移 | CLI 四命令回归（真实 Key 冒烟） |
| 2 | server 骨架：config/db/error/state/routes + 同步代理端点（缓存）+ 测试 | cargo test + curl 冒烟 |
| 3 | 导出任务：service/worker + SSE + Markdown 入库 + 测试 | 单书导出 + 全量导出（真 Key） |
| 4 | web 前端四页面 | pnpm dev 联调 |
| 5 | README 更新 + 设计文档状态改「已实现」 | — |

## 16. 开放问题（实施前拍板）

1. **网关真实限额**：500ms 间隔是拍脑袋的保守值——实施后用一个真实 Key 做一次全量导出观察是否有 errcode 限流反馈，再校准默认值
2. **`/books/{id}/notes` 是否顺手入库**：当前设计「浏览走缓存、导出才入库」边界清晰；若实际使用中发现离线浏览需求强，可改为浏览也 upsert exports（成本低）
3. **search 要不要缓存**：当前不缓存；若发现前端搜索框高频重复查询可加 5min 短缓存
4. **gateway/call 的 api_name 白名单**：当前全放开（网关本身只读）；若公网部署担心被当作免费代理，可收敛为白名单枚举

---

## 附：实施备忘（与设计稿的偏差/细化）

1. **缓存降级（超出设计稿）**：代理端点回源失败但存在旧缓存时，返回过期数据并置 `fresh=false`，而非直接报错——单用户场景下可用性优先；设计稿第 7 节「失败不缓存」仍成立（不写缓存表）
2. **exports 跳过判定（开放问题 2 的落地）**：导出产物是长期资产、无 TTL，「已导出即跳过」，`refresh:true` 才重抓覆盖；单书笔记浏览继续走代理缓存，两者边界保持清晰
3. **worker 队列包含 `running` 项**：进程重启后中断的书直接重试（单书 2~3 次调用，重抓成本低于断点机制的复杂度）
4. **`?download=true`**（非 1）：serde_urlencoded 的 bool 反序列化只接受 true/false
5. **upgrade 识别层级**：`upgrade_info` 判定在真实 client 的 `parse_response` 里，FakeGateway 绕过协议层——集成测试直接以 `GatewayError::Upgrade` 模拟 client 已识别出的阻断
6. **CORS**：`build_app_with_cors` 在基础 router 上叠加 CorsLayer（范本 douban 实现中该配置实际未生效，此处修正）
7. **真机冒烟（2026-10-27）**：假 Key 启动 → healthz/鉴权/server/info 正常；`/shelf` 打通真实网关并正确映射官方 `errcode -2013 鉴权失败` 为 502 信封——协议链路端到端验证通过
8. **BYOK 变更（2026-10-27 当日，按用户要求）**：D1 两层凭据模型改为前端手输 Key 逐请求传递（`X-Weread-Key` / `?key=`）；`GatewayApi` trait 转 async_trait 支持动态分发；`GatewayRegistry` 按 Key 建客户端（共享全局节流锁）；存储加 owner 指纹隔离（旧库检测到无 owner/api_key 列自动整体重建）；25 个集成测试同步改造并通过；真实 Key 经头/参数两条路径均验证打网关成功
