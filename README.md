# weread-notes

基于**微信读书官方 Agent Gateway**（`wrk-` API Key）的读书笔记服务：书架/笔记/统计的代理服务 + 笔记原始数据落库，形态为 **HTTP 服务器 + 管理后台**。

- 协议与坑：[docs/weread-api-projects-deep-dive.md](docs/weread-api-projects-deep-dive.md) §2/§2.5
- 官方 17 接口能力清单：[docs/weread-api-capabilities.md](docs/weread-api-capabilities.md)
- 服务器设计（架构决策、API 规范、数据模型）：[docs/weread-server-design.md](docs/weread-server-design.md)（已实现）
- 前端说明：[frontend/admin/README.md](frontend/admin/README.md)

## 使用（服务器 + 管理后台）

**BYOK 模式**：服务器启动不需要（也不读取）任何 Key；微信读书 API Key 由用户在
前端「连接」处手动输入（存浏览器 localStorage），逐请求经 `X-Weread-Key` 头传给后端
（SSE 走 `?key=` 参数）。不同 Key 的缓存与已同步笔记按指纹相互隔离。

```bash
./backend/target/release/weread-notes-server    # 默认 0.0.0.0:8081，无需任何凭据环境变量
```

```bash
# 同步代理（TTL 缓存，?refresh=true 强制回源）
curl -H "X-Weread-Key: wrk-xxxx" http://127.0.0.1:8081/api/v1/shelf
curl -H "X-Weread-Key: wrk-xxxx" http://127.0.0.1:8081/api/v1/notebooks
curl -H "X-Weread-Key: wrk-xxxx" http://127.0.0.1:8081/api/v1/readdata?mode=overall

# 异步同步（全部/指定书；单书建议 wait_timeout=30 秒级拿终态；
# 笔记原始 JSON——书/章节/划线/想法——写入后端 SQLite 的 notes 表）
curl -X POST -H "X-Weread-Key: wrk-xxxx" -H "Content-Type: application/json" \
     -d '{"book_ids":["3300028061"],"wait_timeout":30}' http://127.0.0.1:8081/api/v1/syncs

# 进度（轮询或 SSE）/ 结果
curl -H "X-Weread-Key: wrk-xxxx" http://127.0.0.1:8081/api/v1/syncs/{task_id}
curl -H "X-Weread-Key: wrk-xxxx" http://127.0.0.1:8081/api/v1/syncs/{task_id}/result
```

管理后台（Vite + React + antd，概览/书架/笔记/同步任务四页面）：

```bash
cd frontend/admin && pnpm install && pnpm dev    # http://localhost:5174（代理到 :8081）
pnpm build                            # 纯静态 dist/，同源或跨源部署
```

## 架构（一图）

```
浏览器(frontend/admin/) ─▶ axum API 层
                 ├─ 同步代理（/shelf /notebooks /readdata /books/...）─▶ TTL 缓存(SQLite) ─▶ ThrottledGateway ─▶ 官方 Agent Gateway
                 └─ 异步同步（/syncs）─▶ DB 即队列 ─▶ 单 worker 逐书抓取 → 原始 JSON 入库(notes 表) → SSE 进度
```

与范本 douban-api 的关键差异：上游是快速官方 API 而非慢速爬虫 → 快接口同步代理、仅批量同步任务化；单 Key 单用户 → 无 uid 维度；持久化双轨（cache_entries 可再生 / notes 长期资产）。详见设计文档第 0/2 节。

## 内置防护

服务器不持有任何默认 Key（BYOK，凭据只经请求传递，本地 SQLite 仅任务表存 Key 供 worker 续跑）；网关调用全局限速（默认 ≥500ms + 抖动，跨所有 Key 共享一把节流锁）；单 worker 串行；书级失败退避重试（3 次）+ 连续 5 次失败熔断 10 分钟；per-IP 限流（60 req/min）；bookId 白名单字符校验；任务/缓存/已同步笔记按 Key 指纹隔离。

## 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `WEREAD_SERVER_BIND` | 0.0.0.0:8081 | 监听地址 |
| `WEREAD_SERVER_DATA_DIR` | data | SQLite 目录 |
| `WEREAD_SERVER_TOKEN` | 空 | 访问鉴权（空=不鉴权） |
| `WEREAD_CACHE_TTL_SECS` | 3600 | 代理缓存 TTL |
| `WEREAD_GATEWAY_MIN_INTERVAL_MS` | 500 | 网关调用最小间隔 |
| `WEREAD_CIRCUIT_THRESHOLD / _COOLDOWN_SECS` | 5 / 600 | 熔断阈值/冷却 |
| `WEREAD_TASK_RETENTION_DAYS` | 30 | 任务记录保留（notes 笔记数据永久） |
| `WEREAD_RATE_PER_MINUTE` | 60 | 每 IP 每分钟请求上限 |
| `WEREAD_SERVER_CORS_ORIGINS` | 空 | 允许的跨源来源（逗号分隔） |

## 测试

```bash
cd backend && cargo test    # 25 个集成测试：FakeGateway + tempdir SQLite + oneshot，全进程内
```

## 已知限制

- 官方网关**全只读**、仅本人数据、无书籍正文（详见能力清单第 9 节）
- 网关真实限额未知，默认 500ms 间隔是保守值（可用环境变量调整）
- 笔记以原始 JSON 整体存于 notes 表（未正规化为明细表）；已同步数据暂无读回端点，查看笔记走实时代理
