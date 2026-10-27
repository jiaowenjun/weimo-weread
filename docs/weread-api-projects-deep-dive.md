# 微信读书官方 API Key 生态：三大项目深度调研

> **调研日期**：2026-10-25
> **前置阅读**：[weread-api-research.md](./weread-api-research.md)（官方 API 全貌与开源生态总览）
> **数据口径**：GitHub REST API 实时数据 + npm 最近 30 天下载量（`api.npmjs.org`），采集于 2026-10-25

---

## 目录

- [1. 筛选方法与结果](#1-筛选方法与结果)
- [2. 共同技术底座：Agent Gateway 协议](#2-共同技术底座agent-gateway-协议)
- [3. 项目一：alchaincyf/huashu-weread（159★）——读书顾问工作流层](#3-项目一alchaincyfhuashu-weread159读书顾问工作流层)
- [4. 项目二：shiquda/weread-cli（24★）——面向人类与 Agent 的 CLI](#4-项目二shiqudaweread-cli24面向人类与-agent-的-cli)
- [5. 项目三：Ceelog/OpenWeRead（23★）——TypeScript SDK 与 CLI](#5-项目三ceelogopenweread23typescript-sdk-与-cli)
- [6. 三项目横向对比](#6-三项目横向对比)
- [7. 对 weread-api 项目的启示](#7-对-weread-api-项目的启示)
- [8. 参考链接](#8-参考链接)

---

## 1. 筛选方法与结果

### 1.1 筛选口径

- **范围**：[总览报告](./weread-api-research.md) 6.2 节"第二代：基于官方 API Key"全部 **41 个项目**（即 awesome-weread 收录的官方 Key 生态）
- **条件**：star 数最高 + **近半年有更新**（最后推送时间 ≥ 2026-04-25），未归档
- **方法**：GitHub API 逐仓查询 `stargazers_count` 与 `pushed_at`，按 star 降序

### 1.2 筛选过程的三个发现

1. **第二代项目整体 star 数偏低**。最高的也只有 159★，与第一代逆向生态（wewe-rss 9.6k、weread2notion-pro 3.4k）差两个数量级——官方 Key 生态尚在早期。
2. **部分项目已消失**：`lovekeji-ai/agent-weread-skill`、`cocovs/weread-agent-chat`、`a3100821009/weread-to-notion` 已 404（删除或改名）；`wong2/weread-mcp` 已并入 `wong2/mcp-servers` 单仓（仅 2★）；`f1603206034/weread-notes` 作者改名后仍活跃（现 `f-a8bfe3fa/weread-notes`，7★）。
3. **678★ 的 weread-omni 不在筛选范围**：它同时依赖官方 Key 与墨水屏逆向接口，在总览报告中归入 6.3 节"双通道"类，不属于纯官方 Key 生态。

### 1.3 全量 Top 10（star 降序）

| # | 仓库 | ★ | 最后推送 | 语言 | 备注 |
|---|---|---|---|---|---|
| 1 | **alchaincyf/huashu-weread** | **159** | 2026-08-25 | Markdown（纯 Skill） | ✅ 入选 |
| 2 | **shiquda/weread-cli** | **24** | 2026-07-21 | TypeScript | ✅ 入选 |
| 3 | **Ceelog/OpenWeRead** | **23** | 2026-05-17 | TypeScript | ✅ 入选 |
| 4 | ZiGmaX809/Weread_ReadTime_Heatmap | 13 | 2026-10-06 | Python | |
| 5 | jerlinn/jerlin-weread | 11 | 2026-05-17 | Shell | |
| 6 | xJogger/weread-mcp-worker | 10 | 2026-07-12 | TypeScript | |
| 7 | zhongyi-byte/openclaw-weread-skill | 8 | 2026-02-06 | Python | ⚠️ 推送在半年线外 |
| 8 | f-a8bfe3fa/weread-notes | 7 | 2026-10-12 | Python | 原作者已改名 |
| 9 | wong2/mcp-servers（含 weread-mcp） | 2 | 2026-08-25 | TypeScript | weread-mcp 并入此仓 |
| 10 | shiquda 以下多为 0-3★ | ≤3 | — | — | 生态长尾 |

> 三个入选项目恰好代表了官方 Key 生态的三种典型形态：**纯提示词工作流层**、**CLI 工具**、**SDK 库**。

---

## 2. 共同技术底座：Agent Gateway 协议

三个项目封装的都是同一个官方网关。先讲清协议本身，后面按项目分析时就不再重复。

### 2.1 基本约定（由 weread-cli 与 OpenWeRead 的客户端源码交叉验证）

```http
POST https://i.weread.qq.com/api/agent/gateway
Authorization: Bearer wrk-xxxxxxxx
Content-Type: application/json

{
  "api_name": "/shelf/sync",       // 要调用的接口名，路径形式
  "skill_version": "1.0.4",        // 必带，服务端据此校验/提示升级
  ...                              // 业务参数直接平铺在 body 顶层，不嵌套在 params 里
}
```

- **单一 POST 端点**：所有能力通过 `api_name` 区分，不是 REST 多路径
- **认证**：`Authorization: Bearer <API Key>`，Key 在官方页面登录后获取（`wrk-` 前缀）
- **响应约定**：
  - 业务错误：顶层 `errcode`（非 0 即失败）+ `errmsg`/`message`
  - **版本升级**：响应携带 `upgrade_info` 字段（含 `message`）时必须先升级再重试，不可忽略
  - **HTTP 499**：上游超时的专用状态码，可重试
- **内省接口**：`api_name: /_list` 可查询网关上所有可用接口及参数定义（OpenWeRead 的 `listApis()` 即此）

### 2.2 已知的 api_name 清单（从三个项目的代码与文档汇总）

| 域 | api_name | 说明 |
|---|---|---|
| 搜索 | `/store/search` | 书城搜索，支持 scope（电子书/有声/网文/作者/全文/公众号等） |
| 书籍 | `/book/info`、`/book/chapterinfo`、`/book/getprogress` | 详情 / 章节目录 / 阅读进度 |
| 书架 | `/shelf/sync` | 全量书架（books/albums/mp/archive） |
| 笔记 | `/user/notebooks` | 笔记本概览（分页：`lastSort`） |
| 笔记 | `/book/bookmarklist` | 某本书的划线列表 |
| 笔记 | `/review/list/mine` | 我的想法/点评 |
| 笔记 | `/book/bestbookmarks` | 全书热门划线（含原文与人数） |
| 笔记 | `/book/underlines` | 章节划线热度统计（不含文本） |
| 笔记 | `/book/readreviews` | 某条热门划线下的想法 |
| 笔记 | `/review/single` | 单条想法详情 |
| 点评 | `/review/list` | 公开书评（type 0-4 筛选） |
| 推荐 | `/book/recommend`、`/book/similar` | 个性化推荐 / 相似书 |
| 统计 | `/readdata/detail` | 阅读统计（weekly/monthly/annually/overall） |
| 元 | `/_list` | 网关接口自描述 |

**分页约定**（各接口游标不同）：搜索用 `searchIdx`、笔记本用 `lastSort`、书评用 `idx` + `synckey`、相似书用 `idx` + `sessionId`。

### 2.3 两个著名的"坑"（huashu-weread 用实战踩出来的）

1. **`/store/search` 响应顶层不是 `books[]`**：实际结构是 `results[]` 按 section 分组，取已上架电子书要过滤 `results[?title=='电子书'].books[*].bookInfo`，未上架的在 `title=='待上架'` 组。用 `res.get('books', [])` 解析会静默得到 0 结果。
2. **多候选误命中**：关键词搜出多本书时，要取 `readingCount` 最高**且作者完全匹配**的（否则 "Co-Intelligence" 会误命中 "Collaborative Intelligence"）。

### 2.4 深度链接

推荐结果应附 `weread://reading?bId={bookId}` 深链，用户可一键打开。

### 2.5 实测补充（2026-10-25，用真实 Key 直连网关验证）

本节为实测结论，修正了上文从项目源码推断的几处细节：

| # | 实测发现 | 与上文推断的差异 |
|---|---|---|
| 1 | `/_list` 接口**可用且无需登录态校验**，返回 17 个接口的完整参数定义（名称/类型/required/默认值/响应字段） | OpenWeRead 的 `listApis()` 确实可用，是发现官方接口变化的第一手来源 |
| 2 | 官方 Skill 实际版本已是 **1.0.5**（`upgrade_info.latest_version`），1.0.4 会触发升级提示 | weread-cli v0.1.4 硬编码的 1.0.4 已落后 |
| 3 | **`upgrade_info` 是伴随信号而非阻断**：响应同时携带业务数据 + 升级提示；仅版本号过大/过小都不会被拒（`9.9.9` 也能正常返回） | weread-cli 将 `upgrade_info` 实现为抛错中断（exit 3），实测过严 |
| 4 | `/review/list/mine` 的书籍参数名是小写 **`bookid`**（其余接口是 `bookId`），翻页游标为 `synckey` | 大小写混用是官方接口的历史包袱，封装时容易踩坑 |
| 5 | `/book/bookmarklist` 响应自带 `chapters`（章节表）和 `book`（书籍信息），**一次调用即可完成导出所需的全量上下文**，无需再调 `/book/chapterinfo` | 单书导出的最少请求数是 2 次（bookmarklist + review/list/mine），不是 4 次 |
| 6 | `/review/list/mine` 的列表项为 `{reviewId, review: {...}}` 双层嵌套：`createTime`/`chapterUid`/`abstract`（划线原文）/`content`（想法文字）都在**内层** `review` 对象 | 顶层取 `createTime`/`content` 会得到 null，导出渲染会落空 |
| 7 | `/user/notebooks` 响应含 `totalBookCount`/`totalNoteCount` 汇总字段；该实测账号 108 本有笔记的书、1604 条笔记 | — |

复现代码：[`src/`](../src/)（Rust 实现，`cargo run -- apis|notebooks|export <bookId>`；早期的 Python 原型已在纯 Rust 重构时移除）。

---

## 3. 项目一：alchaincyf/huashu-weread（159★）——读书顾问工作流层

### 3.1 概览

| 维度 | 数据 |
|---|---|
| 仓库 | <https://github.com/alchaincyf/huashu-weread> |
| Stars / Forks | **159** / 9（官方 Key 生态第一） |
| 形态 | **纯 Skill（提示词工程），无一行运行时代码**，仅 12 个 Markdown/JSON 文件，35KB |
| 创建 | 2026-05-17（官方 Skill 发布当天） |
| 最近推送 | 2026-08-25（共 2 个 commit：初版 + 版本自检机制） |
| 贡献者 | 1（花叔，公众号「花叔」/ @AlchainHust，知名 AI 博主——159★ 主要来自其公众号流量） |
| License | MIT（含 LICENSE 文件） |
| 安装 | `npx skills add alchaincyf/huashu-weread` |

### 3.2 定位：官方 Skill 之上的一层"工作流编排"

作者的判断：官方 weread skill 只是"自然语言包装的搜索接口"——让它推荐书，它不会看你读过什么，会把用户做了 68 条笔记的书当新书推回来。**huashu-weread 不替代官方 skill，而是在其 8 个 API 之上加一层读书顾问方法论**，两者叠加使用。

README 给出的对比很有说服力：同一个问题"推荐产品经理方向的书"，裸官方 skill 返回 14 本通识书单（含用户已重度阅读的书）；huashu-weread 返回 5 本基于用户实际阅读数据的"拼图补缺"建议，并给出"如果只读一本"的明确取舍。

### 3.3 组成结构

```
SKILL.md                        # 入口：路由、通用约定、异常表、展示规范
workflows/advisor.md            # 推荐下一本读什么
workflows/path.md               # 领域学习路径规划（入门→进阶→前沿）
workflows/alchemy.md            # 零散笔记 → 结构化读书笔记
workflows/review.md             # 生成可发布的阅读复盘文章
shared/knowledge-map.md         # 方法论：怎么读懂一个人的知识地图
shared/shelf-cross-notes.md     # 书架×笔记交叉分析的 Python 代码模板 + 主题关键词组
examples/advisor-neuroscience.md # 完整实战案例复盘
test-prompts.json               # 触发话术测试集
```

### 3.4 核心方法论：书架 × 笔记交叉分析

三个数据源 × 三种交叉信号：

| 数据源 | 接口 | 揭示什么 |
|---|---|---|
| 书架分类 | `/shelf/sync` 的 `archive[]` | 用户**主动归类**的兴趣方向 |
| 书架书目 | `/shelf/sync` 的 `books[]` | 收藏了什么 + 最近翻动时间 |
| 笔记列表 | `/user/notebooks` | **真读过**的书 + 深度（笔记条数） |

| 交叉信号 | 判定 | 含义 |
|---|---|---|
| 真读了 | 书架有 + 笔记 ≥ 5 | 吃进去了的书 |
| 放着没动 | 书架有 + 无笔记 | 收藏癖，不是真兴趣 |
| **隐藏深读** | 书架无 + 笔记 ≥ 10 | 借阅/试读但深读——**最强兴趣信号**，只看书架会漏 |
| 浅尝即止 | 笔记 1-3 条 | 翻了几页放下 |

四个 workflow 各自的触发判断也基于数据：advisor 与 path 的分流规则是"先查 `/user/notebooks`，该方向笔记多的书 ≥ 3 本走 advisor（有基础做延展），否则走 path（从零规划）"。

### 3.5 工程亮点（即使在"纯提示词"形态下）

这是本项目最有参考价值的部分——**把软件工程的防御性设计搬进了 Skill 文档**：

1. **异常全枚举 + fallback 明示**：SKILL.md 列了 13 种实操异常（key 未设置、errcode≠0、upgrade_info、/store/search 解析坑、多候选误命中、notebooks 为空、书架为空、分页未拉完、readUpdateTime=0……），每种都有明确处理动作，"绝不静默失败"。
2. **检查点设计原则**：只在"分叉影响输出本质"处插入用户确认（推荐数量、发布平台、段位判断），日常小决策 AI 自己定；若用户原始 prompt 已明确指定则全部跳过。
3. **无人值守 fallback**：检测到没有交互通道时（被自动化流水线调用），不卡在检查点，走默认值并在输出中明示"无人值守模式，按默认参数推荐"。
4. **数据展示规范强制化**：Unix 时间戳必须转 `YYYY-MM-DD`、时长秒转"X 小时 Y 分钟"、bookId 一律渲染为 weread:// 深链"绝不裸数字"。
5. **版本自检机制**（2026-08-25 增补）：每次使用前读本目录 `.last-update-check`，30 天内不重复检查；到期后静默 `git ls-remote` 对比版本，落后也先完成当前任务，结束后附一句升级提示——把"skill 版本治理"做成了零打扰。
6. **skill_version 权威来源约定**：不硬编码版本号，而是声明"以已安装官方 skill 的 SKILL.md frontmatter version 字段为准，别从 prompt 或老模板里抄"。

### 3.6 局限

- 无代码即无测试，方法论正确性完全依赖 LLM 遵循指令的效果
- 单人项目、2 个 commit，方法论迭代慢（但 SKILL.md 内容密度极高，属于"少而精"）
- 依赖先安装官方 skill；官方 SKILL.md 路径写死为 `~/.claude/skills/weread/SKILL.md`（Claude Code 中心）
- 输出风格绑定"花叔语言风格"，个人化强

### 3.7 适用人群

想让 Claude Code / Cursor / Codex / OpenClaw 变成"个人读书顾问"的终端用户；以及想学习"如何在原子 API 之上写工作流层 Skill"的开发者（其 README 最后一节就是写给 skill 开发者的设计原则）。

---

## 4. 项目二：shiquda/weread-cli（24★）——面向人类与 Agent 的 CLI

### 4.1 概览

| 维度 | 数据 |
|---|---|
| 仓库 | <https://github.com/shiquda/weread-cli> |
| Stars / Forks | 24 / 6 |
| 形态 | TypeScript CLI（npm 包 `weread-agent-cli`，v0.1.4，命令名 `weread`）+ 配套 Skill |
| 创建 | 2026-05-16（官方发布前 1 天，反应极快） |
| 最近推送 | 2026-07-21（v0.1.4：支持官方 Skill 1.0.4 的 discovery 新接口） |
| Commits | 15；合并过外部贡献者 PR（zizhen01 的模块化重构） |
| npm 下载 | **186 次/月**（官方 Key 生态工具中实际使用量最高） |
| License | package.json 声明 MIT，**但仓库无 LICENSE 文件** ⚠️ |
| 工程设施 | GitHub Actions CI、ESLint、node:test + 本地 mock 服务（测试不依赖真实接口）、运行时依赖仅 commander 一个 |
| Node 要求 | ≥ 20（原生 fetch） |

### 4.2 定位

"面向人类和 Agent 的微信读书命令行工具"——把官方网关封装成稳定本地命令，日常使用不需要手写 curl；同时内置 Skill 指导 Agent 优先调用 CLI 而不是自己拼请求。

### 4.3 命令全景

| 域 | 命令 |
|---|---|
| 体检 | `weread doctor`（检查 CLI 健康 + 凭据配置） |
| 配置 | `weread config set-key / list / path / set-timeout`（存 `~/.weread-cli/config.json`） |
| 搜索 | `weread search "三体" --scope book|audio|fiction|author|fulltext|list|mp|article|all` |
| 书籍 | `weread book resolve "书名"`（书名→bookId）、`book info / chapters / progress` |
| 书架 | `weread shelf list`、`shelf recent --limit 10` |
| 笔记 | `notes notebooks`（概览）、`notes top`（笔记最多的书）、`notes bookmarks / mine / best / underlines / readreviews`、**`notes export <bookId> --format markdown --output notes.md`**（单书 Markdown 导出） |
| 统计 | `readdata detail / summary --mode weekly|monthly|annually|overall` |
| 点评 | `reviews list --type 0..4`、`reviews single` |
| 推荐 | `discover recommend / similar / friends` |
| **逃生口** | `weread api call /store/search --param keyword=三体`（任意 api_name 直调，新接口未封装时不受阻） |

Agent 使用约定：默认 `--json`（结构化）、大输出先写文件再摘要、`--compact` 裁剪元数据。

### 4.4 客户端实现深度分析（`src/client.ts`，工程完成度三者最高）

1. **配置解析四级优先 + 溯源**：constructor 参数 > 环境变量（`WEREAD_API_KEY` / `WEREAD_API_BASE_URL` / `WEREAD_TIMEOUT_MS`）> 配置文件 > 默认值；且每个配置项都记录来源（`api_key_source: "env"|"config"|...`），`doctor` 可直接诊断"key 从哪来"。Key 展示走 `maskSecret` 脱敏。
2. **重试策略**：最多 3 次 attempt，退避 `150ms × attempt`；HTTP 499（上游超时）与网络类错误（AbortError/fetch failed/timeout）可重试，其余不重试。
3. **错误分类学**：8 种错误类型（`missing_auth` / `network_error` / `upstream_timeout` / `http_error` / `invalid_json` / `api_error` / `upgrade_required` / `invalid_input`），统一 JSON 输出 `{ok:false, error:{type, message, retryable, attempts}}`，并映射语义化退出码（2=未配置凭据，3=需升级 skill 版本）——Agent 与脚本可以精确分支处理。
4. **升级协议处理**：检测到响应含 `upgrade_info` 即抛 `upgrade_required`，错误信息透传官方 `upgrade_info.message`。
5. **`skill_version` 管理**：常量 `SKILL_VERSION = "1.0.4"` 硬编码在 client 与 Skill frontmatter 双处，v0.1.4 专门修过一致性（commit: "fix SKILL_VERSION consistency"）——**官方升级时需要发版跟进**，这是该协议下所有封装方的共同维护负担。

### 4.5 Skill 设计（`skills/weread/SKILL.md`，Agent 工程范本)

- **按需加载**：SKILL.md 只保留命令地图与意图路由，细节拆到 `references/first-use.md`（首次配置）与 `references/domain-rules.md`（字段口径、深链格式、导出限制），并明确标注"何时才读哪个文件"——控制 Agent 启动上下文体积
- **意图路由表**：中文口语（搜书/听书/网文/书里提到）→ 具体 scope 参数的映射全覆盖
- **分页纪律**："默认浅拉取，用户明确要完整导出/排行时才翻页"，每种接口的游标用法写明
- **错误处理约定**：每种 CLI 错误类型对应的 Agent 动作（如 `upgrade_required` → 立即停止当前任务，先升级）
- 配套文档 `docs/agent-cli-skill-patterns.md` 总结了 "CLI + Skill" 双层模式的方法论

### 4.6 局限

- 仓库缺 LICENSE 文件（package.json 虽声明 MIT，严格意义上授权存疑，二次使用者需注意）
- 单人主导维护，官方 skill_version 升级依赖作者发版
- 2026-07-21 后无新提交（3 个月），但功能覆盖已完整、npm 包可直接用
- 无 SDK 形态输出（不可作为库嵌入，只能 spawn CLI）

### 4.7 适用人群

想要"开箱即用命令行 + Agent 自动调用"两全的个人用户；想研究"CLI + Skill 双层 Agent 工具链怎么设计"的开发者。**是三个项目里实际可用性最高的**。

---

## 5. 项目三：Ceelog/OpenWeRead（23★）——TypeScript SDK 与 CLI

### 5.1 概览

| 维度 | 数据 |
|---|---|
| 仓库 | <https://github.com/Ceelog/OpenWeRead> |
| Stars / Forks | 23 / 5 |
| 形态 | **SDK 优先**（npm 包 `openweread`，v0.1.1）+ CLI（`openweread` 命令）+ 自带 Skill |
| 创建 | 2026-05-16 |
| 最近推送 | **2026-05-17 后无更新**（全部 6 个 commit 集中在两天内，之后沉寂 5 个月） |
| npm 下载 | 43 次/月 |
| License | README 末尾写 MIT，**仓库无 LICENSE 文件** ⚠️（package.json 声明 MIT） |
| 工程设施 | vitest、biome、pnpm、可注入 fetch 的测试设计；包含 CLAUDE.md（AI 辅助开发配置） |
| Node 要求 | ≥ 18 |

### 5.2 定位

三个项目中**唯一的库形态**：先 SDK（可 `import` 嵌入自己的应用），CLI 与 Skill 是 SDK 之上的薄封装。

### 5.3 SDK 模块与接口覆盖

```ts
import { OpenWeRead } from "openweread";
const weread = new OpenWeRead({ apiKey: "" });   // 空则回落读 WEREAD_API_KEY

const result = await weread.search.search({ keyword: "三体", scope: 10 });
const shelf = await weread.shelf.sync();
for await (const book of weread.notes.notebooksAll()) { ... }   // 内置异步迭代器自动翻页
```

| SDK 模块 | 覆盖接口 |
|---|---|
| `weread.search` | `/store/search` |
| `weread.book` | `/book/info`、`/book/chapterinfo`、`/book/getprogress` |
| `weread.shelf` | `/shelf/sync` |
| `weread.readData` | `/readdata/detail` |
| `weread.notes` | `/user/notebooks`、`/book/bookmarklist`、`/review/list/mine`、`/book/underlines`、`/book/bestbookmarks`、`/book/readreviews`、`/review/single`（**7 个接口，笔记域覆盖最全**） |
| `weread.review` | `/review/list` |
| `weread.discover` | `/book/recommend`、`/book/similar` |
| `weread.profile` | **组合接口**（非网关原生）：`/shelf/sync` + 并发 N 次 `/book/getprogress`，输出"书架总数 + 最近在读进度"的阅读概况 |

两个独有设计：

1. **`/_list` 网关内省**：`weread.client.listApis()` 可拉取网关全部接口定义——官方新增接口时可先于文档发现（探测官方能力演进的前哨）。
2. **`notebooksAll()` 异步迭代器**：把 `hasMore`/`lastSort` 翻页封装成 `for await`，调用方零心智负担（weread-cli 要求手动传 `--last-sort`）。

### 5.4 客户端实现（简洁风格）

与 weread-cli 同一协议、不同取舍：无重试、无配置文件（仅 env/构造参数）、构造时无 key 直接抛 `WereadAuthError`；**可注入 `fetch` 实现**（`options.fetch`），测试无需 mock 整个网络栈。类型定义完整（`src/types.ts` 独立成型），错误体系分 `WereadAuthError / WereadError / WereadHttpError` 三类。

### 5.5 文档策略

仓库直接**内置了官方 8 份技能文档的镜像**（`weread-skills/*.md`），并另建自己的 `openweread-skills/SKILL.md`（含 `agents/openai.yaml`）指导 Agent 走 CLI。README 的"能力覆盖"表把文档 ↔ SDK 模块 ↔ 接口三者对齐，是目前对官方接口**清单化最完整**的公开梳理。

### 5.6 局限

- **沉寂风险最高**：2026-05-17 后无任何提交；`SKILL_VERSION` 停在 **1.0.3**（weread-cli 已跟进 1.0.4），若官方强制校验旧版本可能触发 `upgrade_required`
- 客户端无重试/退避，499 场景直接失败
- CLI 命令较薄（部分能力只在 SDK 层，如 profile 组合接口 CLI 未暴露完全）
- 同样缺 LICENSE 文件

### 5.7 适用人群

想在**自己的 Node/TS 应用里编程调用**微信读书数据的开发者（唯一选择）；想拿它当类型定义与接口参考书的人。不适合直接当"开箱工具"用（更新停滞）。

---

## 6. 三项目横向对比

### 6.1 总览

| 维度 | huashu-weread | weread-cli | OpenWeRead |
|---|---|---|---|
| **抽象层次** | 工作流/方法论层 | CLI 工具层 | SDK 库层 |
| **形态** | 纯 Markdown Skill | npm CLI + Skill | npm SDK + CLI + Skill |
| **语言** | — | TypeScript | TypeScript |
| **Stars** | **159** | 24 | 23 |
| **最近推送** | 2026-08-25 | 2026-07-21 | 2026-05-17 ⚠️ |
| **npm 月下载** | — | **186** | 43 |
| **接口覆盖** | 不直接封装（复用官方） | 全 + `api call` 逃生口 | 全 + `/_list` 内省 + profile 组合 |
| **错误处理** | 文档级 fallback 表 | 8 类错误 + 退出码 + 重试 | 3 类异常，无重试 |
| **测试** | 无（test-prompts.json 话术集） | mock 服务单测 + CI | vitest 单测（可注入 fetch） |
| **License** | ✅ MIT 文件 | ⚠️ 声明 MIT 无文件 | ⚠️ 声明 MIT 无文件 |
| **skill_version** | 动态读官方 skill | 1.0.4（已跟进） | 1.0.3（落后） |
| **核心价值** | 把 API 变成"懂你的顾问" | 把 API 变成"可靠命令" | 把 API 变成"可编程库" |

### 6.2 客户端实现细节对比（同为 TS 的两者）

| 细节 | weread-cli | OpenWeRead |
|---|---|---|
| Key 来源 | 构造参数 > env > 配置文件（带来源溯源 + 脱敏） | 构造参数 > env（缺失即抛异常） |
| 重试 | 3 次，150ms 线性退避，499/网络错误可重试 | 无 |
| 超时 | 可配置（env/配置文件/参数），默认 30s | 可配置（参数），默认 30s |
| 升级检测 | `upgrade_info` → 专用错误 + 退出码 3 | `upgrade_info` 挂到错误对象 |
| 可测性 | mock 本地服务跑集成式测试 | 依赖注入 fetch，纯单测 |
| 翻页 | 手动传游标（Skill 文档教 Agent 何时翻） | `notebooksAll()` 迭代器自动翻 |

### 6.3 一句话画像

- **huashu-weread**：不写代码，把"读书顾问的方法论"写成 Agent 能严格执行的 SOP——**提示词工程的标杆**
- **weread-cli**：把网关协议的所有脏活（认证/版本/重试/错误归一化）收进一个可靠二进制——**工程完成度的标杆**
- **OpenWeRead**：给出最完整的类型定义与接口清单，是官方文档之外最好的"接口参考书"——**资料价值的标杆（但需注意时效）**

---

## 7. 对 weread-api 项目的启示

若在本仓库自建 API 层，三个项目给出了完整的外部经验：

### 7.1 自建网关客户端必须处理的 8 件事（协议清单）

1. `Authorization: Bearer wrk-...` 认证与缺失时的友好报错
2. 请求体组装：`api_name` + `skill_version` + **参数平铺**（不能嵌 `params`）
3. `errcode ≠ 0` 的业务错误识别（错误消息字段兼容 `errmsg/message/errMsg/msg` 四种）
4. `upgrade_info` 升级信号：必须中断并透传官方提示，不得静默忽略
5. HTTP 499 = 上游超时，纳入可重试集合
6. 超时控制（AbortController，默认 30s 足够）
7. 错误归一化（统一 type/message/retryable/attempts 结构 + 语义化退出码）
8. 每种接口各自的分页游标（`lastSort`/`searchIdx`/`synckey`/`sessionId`）

### 7.2 值得直接借鉴的模式

| 模式 | 来源 | 用法 |
|---|---|---|
| 配置四级优先 + 来源溯源 + 脱敏展示 | weread-cli | 任何持有用户凭据的工具 |
| `api call <api_name>` 逃生口 | weread-cli | 官方加新接口时封装层不成为瓶颈 |
| `/_list` 接口内省 | OpenWeRead | 自动发现官方能力变化，比盯文档更早 |
| 组合接口（profile = shelf + N×progress） | OpenWeRead | 在只读 API 上构造高阶语义 |
| 按需加载的 references 分层 | weread-cli Skill | 控制 Agent 上下文成本 |
| 检查点 + 无人值守 fallback | huashu-weread | 任何交互式 Agent 工作流 |
| `.last-update-check` 30 天静默版本自检 | huashu-weread | 单机分发工具的升级治理 |
| 异常全枚举 + "绝不静默失败"表 | huashu-weread | API 封装层的错误文档写法 |

### 7.3 风险警示

1. **skill_version 是协议级耦合**：三项目中一升一停一动态，说明官方升级时封装方必须跟进；自建实现应支持从环境/配置注入版本，并保留 `upgrade_info` 的透传链路
2. **生态均为单人维护**：star 低、贡献者 1 人、部分项目三个月内已消失——**自建实现不宜深度依赖任何单一社区包**，SDK 参考其接口定义后自行维护更稳妥
3. **两个 TS 项目都缺 LICENSE 文件**：照抄代码前需注意授权合规（huashu-weread 是唯一有正式 MIT LICENSE 的，但它是纯文档）
4. **`/store/search` 响应结构**与直觉不符（`results[]` 分组），解析代码要按 2.3 节处理并写测试

---

## 8. 参考链接

- alchaincyf/huashu-weread：<https://github.com/alchaincyf/huashu-weread>（SKILL.md、workflows/、shared/ 为核心阅读材料）
- shiquda/weread-cli：<https://github.com/shiquda/weread-cli>（`src/client.ts`、`skills/weread/SKILL.md`、`docs/agent-cli-skill-patterns.md`）
- Ceelog/OpenWeRead：<https://github.com/Ceelog/OpenWeRead>（`src/`、`weread-skills/` 内置官方文档镜像）
- npm：[weread-agent-cli](https://www.npmjs.com/package/weread-agent-cli)（186 下载/月）、[openweread](https://www.npmjs.com/package/openweread)（43 下载/月）
- 官方 Skill 页面：<https://weread.qq.com/r/weread-skills>
- 总览报告：[weread-api-research.md](./weread-api-research.md)
