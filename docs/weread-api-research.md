# 微信读书 API 与开源生态调研报告

> **调研日期**：2026-10-25
> **调研范围**：微信读书官方开放能力、非官方接口、GitHub 相关开源项目生态
> **适用读者**：准备开发微信读书数据类应用 / 工具 / 同步服务的开发者

---

## 目录

- [1. 一页速览（TL;DR）](#1-一页速览tldr)
- [2. 背景](#2-背景)
- [3. 官方 API 详解](#3-官方-api-详解)
- [4. 非官方接口现状](#4-非官方接口现状)
- [5. 三条接入通道对比](#5-三条接入通道对比)
- [6. 开源项目生态汇总](#6-开源项目生态汇总)
- [7. 选型建议](#7-选型建议)
- [8. 风险与合规提示](#8-风险与合规提示)
- [附录：全部参考链接](#附录全部参考链接)

---

## 1. 一页速览（TL;DR）

1. **微信读书没有传统意义上的"公开 REST API 开放平台"**。官方正式开放的唯一开发者能力是 **2026 年 5 月上线的「微信读书 Skill」（Agent Gateway）**——一套面向 AI 助手 / Agent 的**只读** API，通过个人 API Key（`wrk-` 前缀）认证。
2. **官方文档入口只有三个**：
   - Skill 介绍页 + API Key 获取：<https://weread.qq.com/r/weread-skills>
   - 官方 Skill 仓库（事实上的接口文档）：<https://github.com/Tencent/WeChatReading>
   - 面向内容方（非开发者）的开放平台：<https://cp.weread.qq.com>
3. **官方能力范围**：书籍搜索、书籍详情/进度、书架、笔记划线想法、阅读统计、书评、个性化推荐——共 8 类技能，**全部只读、仅限本人数据**。
4. **写操作、书籍导出、公众号阅读等能力官方未开放**，社区项目依靠逆向 Web 版接口（cookie 鉴权）和墨水屏版接口实现，存在封号与失效风险。
5. **开源生态分两代**：GitHub 上搜索 "weread" 共约 **993 个仓库**。老一代基于逆向接口（star 数高，如 wewe-rss 9.6k、weread2notion-pro 3.4k）；新一代基于官方 API Key（2026 年 5 月后涌现约 45+ 个，索引见 [awesome-weread](https://github.com/BENZEMA216/awesome-weread)）。
6. **快速选型**：做数据查询 / 统计 / 同步 → 官方 Skill 通道；需要写操作或导出 → 逆向通道（自担风险）；直接复用现成能力 → 见 [第 7 节](#7-选型建议)。

---

## 2. 背景

### 2.1 微信读书简介

微信读书（WeRead）是腾讯于 2015 年推出的阅读 App，提供电子书、有声书、网文、公众号文章阅读，以"阅读时长兑体验卡"的社交化阅读运营著称。

### 2.2 API 开放历程

| 时间 | 事件 |
|---|---|
| 2015 – 2026.4 | **无任何官方开放 API**。社区依靠抓包逆向 Web 版内部接口（`weread.qq.com/web/*`，cookie + vid 鉴权）开发同步、导出工具，长期处于灰色地带 |
| 2026-05-17 | 官方发布 **Agent Gateway / 微信读书 Skill**，开放个人 API Key（`wrk-` 前缀），首批支持 Claude 等 AI Agent 接入，官方仅提供 6-8 项**只读**技能 |
| 2026-05 之后 | 社区基于官方 Key 快速涌现 SDK、CLI、MCP 服务、同步与可视化工具（约 45+ 个），并有项目（如 weread-omni）结合墨水屏 API 把能力扩展到 40 项读写操作 |

---

## 3. 官方 API 详解

### 3.1 微信读书 Skill（Agent Gateway）——唯一官方开发者通道

#### 定位

官方页面（[weread.qq.com/r/weread-skills](https://weread.qq.com/r/weread-skills)）的表述是："让 AI 成为你的阅读搭档——连接微信读书账号，让 AI 助手随时查阅你的阅读记录"。它**不是**给传统后端服务调用的开放平台，而是**面向 AI Agent 的技能包**：安装后 Agent 可用自然语言驱动，底层是 REST 接口。

#### 六大宣传功能

| # | 功能 | 说明 |
|---|---|---|
| 1 | 查阅书架 | 浏览个人书架、了解藏书全貌 |
| 2 | 阅读统计 | 时长、天数、偏好分析 |
| 3 | 笔记和划线 | 查看、导出笔记与想法 |
| 4 | 书籍搜索 | 书城搜索，获取书名、作者、评分 |
| 5 | 书籍详情 | 详情、章节目录、阅读进度 |
| 6 | 推荐好书 | 基于偏好的个性化推荐 |

#### 接入方式

```bash
# 1. 安装官方 Skill
npx skills add Tencent/WeChatReading -g

# 2. 配置环境变量（Key 在官方页面登录后获取）
export WEREAD_API_KEY=wrk-xxxxxxxx
```

- **API Key 获取**：登录 <https://weread.qq.com/r/weread-skills>，按引导完成"快速配置 → 登录微信读书 → 复制 API Key"
- **Key 格式**：`wrk-` 前缀
- **数据边界**：API Key 绑定个人账号，"数据仅你可见"
- **升级机制**：服务端通过 `upgrade_info` 字段通知 Skill 升级（当前版本 1.0.4）

#### 认证与调用模型

- 用户身份通过 API Key 自动识别，官方 README 明确"所有需要用户身份的接口会自动注入，无需手动传入"
- 接口为 REST 风格路径，技能文档以相对路径描述（如 `/shelf/sync`），实际 base URL 由 Skill 运行时配置
- 调用失败时有统一的兜底话术（如"书架信息暂时没拉到，请稍后再试~"）

#### 官方技能（接口）清单

来自官方仓库 `Tencent/WeChatReading` 的 `skills/` 目录（Apache-2.0 协议）：

| 技能 | 文档 | 覆盖接口 | 典型返回内容 |
|---|---|---|---|
| 搜索书籍 | `search.md` | 书城搜索（电子书 / 有声书 / 网文 / 作者 / 全文） | 书名、作者、评分 |
| 书籍信息 | `book.md` | `/book/info`（详情+目录）、`/book/getprogress`（进度） | 详情、章节、阅读进度 |
| 书架管理 | `shelf.md` | `/shelf/sync` | `books[]`（电子书）、`albums[]`（有声书）、`mp`（文章收藏入口）、`archive[]`（书单）、`bookCount` |
| 笔记划线 | `notes.md` | 笔记概览、划线、想法、热门划线 | 划线与想法列表 |
| 阅读统计 | `readdata.md` | 时长、天数、排行、偏好分析 | 统计数据 |
| 书籍点评 | `review.md` | 公开点评浏览与筛选 | 书评 |
| 推荐发现 | `discover.md` | 个性化 / 相似书推荐 | 推荐列表 |

`/shelf/sync` 返回结构示例（骨架）：

```json
{
  "books": [
    {
      "bookId": "…", "title": "…", "author": "…", "cover": "…",
      "category": "…", "readUpdateTime": 0, "finishReading": 1,
      "updateTime": "…", "isTop": 0, "secret": 0
    }
  ],
  "albums": [ { "albumInfo": { "albumId": "…", "name": "…", "trackCount": 0 }, "albumInfoExtra": { } } ],
  "mp": { },
  "archive": [ { "name": "书单名", "bookIds": ["…"] } ],
  "bookCount": 0
}
```

字段要点：`books[]` 不含专辑和文章收藏；`mp` 仅是文章收藏的目录入口；`bookCount` 通常等于 `books.length`。

#### 能力限制（重要）

- **只读**：无任何写入接口（不能写入笔记、不能订阅、不能导出书籍正文）
- **仅本人数据**：不能查询他人书架 / 笔记
- **面向 Agent**：接口设计以"AI 助手对话"场景为主，不是通用开放平台
- **无正文**：书籍正文、公众号文章内容不在开放范围内

### 3.2 内容开放平台（cp.weread.qq.com）

<https://cp.weread.qq.com> 是微信读书官方开放平台，但**面向出版社 / 内容合作方**（"我要合作"入口），用于内容上架与分成，**不提供面向开发者的数据 API**。开发者调研时可忽略。

### 3.3 其他官方资源

| 资源 | 链接 | 性质 |
|---|---|---|
| 官方 Skill 安装包直链 | <https://cdn.weread.qq.com/skills/weread-skills.zip> | 官方分发的 Skill 包 |
| 微信读书团队博客 | <https://wereadteam.github.io> | 官方技术博客 |
| 微信读书 Web 版 | <https://weread.qq.com> | 官方站点（其内部接口即社区逆向对象） |

---

## 4. 非官方接口现状

官方 Skill 之外，微信读书存在两路**无官方文档**的内部接口，是传统社区项目的基础：

### 4.1 Web 版接口

- **形态**：`weread.qq.com/web/*` 下的 JSON 接口（书架、笔记、进度、热门标注等）
- **鉴权**：登录 cookie（`wr_vid` + `wr_skey` 等），`wr_skey` 会过期需刷新
- **风险**：接口无稳定性承诺，随时可能改动；频繁调用有封号风险
- **现状**：依然可用，大量老项目（weread2notion 系列、obsidian-weread-plugin 等）依赖它

### 4.2 墨水屏版接口

- **形态**：墨水屏设备版客户端使用的接口，能力比 Web 版与官方 Skill 更全
- **代表项目**：[teng-lin/weread-omni](https://github.com/teng-lin/weread-omni) 基于它实现了 **40 项读写操作**（书架/笔记读写、书籍导入、AI 权限等，官方仅 6-8 项只读）
- **风险**：同样属于逆向，无稳定性承诺

---

## 5. 三条接入通道对比

| 维度 | 官方 Skill（Agent Gateway） | Web 版接口（逆向） | 墨水屏接口（逆向） |
|---|---|---|---|
| **官方支持** | ✅ 官方开放 | ❌ 无文档 | ❌ 无文档 |
| **认证方式** | API Key（`wrk-`） | cookie（`wr_vid`/`wr_skey`，会过期） | 设备登录凭据 |
| **能力范围** | 8 类只读（搜索/书架/笔记/统计/推荐等） | 读为主（书架/笔记/进度/热门标注） | 最全，约 40 项**含写操作** |
| **书籍正文导出** | ❌ | ⚠️ 需额外手段（爬虫/截图拼接） | ⚠️ 部分 |
| **公众号文章阅读** | ❌ | ⚠️ 有限 | ✅（weread-omni 支持） |
| **稳定性** | 高（官方维护，有版本升级机制） | 低（接口随时可能变动） | 低 |
| **合规风险** | 低（官方授权个人数据） | 中-高（违反用户协议，有封号案例） | 中-高 |
| **适用场景** | 数据查询、统计、同步、报告 | 笔记同步、老项目维护 | 写操作、导出、深度定制 |

---

## 6. 开源项目生态汇总

GitHub 搜索 "weread" 共约 **993 个仓库**。生态明显分为两代：

- **第一代（2026 年 5 月前）**：基于逆向 Web/墨水屏接口，star 数普遍更高，经受住长期检验
- **第二代（2026 年 5 月后）**：基于官方 `wrk-` API Key，涌现约 45+ 个项目，索引见 [BENZEMA216/awesome-weread](https://github.com/BENZEMA216/awesome-weread)（该列表明确只收录基于官方 Key 的项目，cookie 抓取类不在范围内）

> ⭐ 为 2026-10-25 时的大致 star 数，仅供参考。

### 6.1 官方与索引资源

| 项目 | Stars | 说明 |
|---|---|---|
| [Tencent/WeChatReading](https://github.com/Tencent/WeChatReading) | ~265 | **官方 Skill 仓库**（Apache-2.0），skills/ 目录即接口文档 |
| [BENZEMA216/awesome-weread](https://github.com/BENZEMA216/awesome-weread) | - | 官方 Key 生态的 awesome 索引（约 45 项） |
| [gandli/weread-skills](https://github.com/gandli/weread-skills) | - | 官方 Skill 包的自动同步镜像 |

### 6.2 第二代：基于官方 API Key

#### SDK / CLI

| 项目 | 语言 | 说明 |
|---|---|---|
| [Ceelog/OpenWeRead](https://github.com/Ceelog/OpenWeRead) | TypeScript | 覆盖最全的 SDK + npm CLI（`openweread`） |
| [nlimpid/weread](https://github.com/nlimpid/weread) | Rust | LLM 友好的 CLI 与库 |
| [lucis-yg/weread-skill-api](https://github.com/lucis-yg/weread-skill-api) | Node | 封装 17 个 REST 接口的本地 API 网关 |
| [shiquda/weread-cli](https://github.com/shiquda/weread-cli) | Python | CLI + Skill manifest |
| [ipfans/weread-cli](https://github.com/ipfans/weread-cli) | Python | CLI + Agent 插件（搜书/书架/笔记/推荐） |
| [marcus776957/weread-master](https://github.com/marcus776957/weread-master) | Python | 全功能 CLI + Agent Skill |

#### MCP 服务（Model Context Protocol）

| 项目 | 说明 |
|---|---|
| [wong2/weread-mcp](https://github.com/wong2/weread-mcp) | 远程 MCP 服务，Bearer 鉴权 |
| [xJogger/weread-mcp-worker](https://github.com/xJogger/weread-mcp-worker) | 部署于 Cloudflare Workers、面向 ChatGPT 的 MCP |
| [freestylefly/mcp-server-weread](https://github.com/freestylefly/mcp-server-weread) ⭐583 | 老牌微信读书 MCP（cookie 方式） |

#### 同步导出

| 项目 | 目标 | 说明 |
|---|---|---|
| [KevinChen1994/weread2notion](https://github.com/KevinChen1994/weread2notion) | Notion | `wrk-` Key 鉴权 |
| [uuavv/weread-notion-worker](https://github.com/uuavv/weread-notion-worker) | Notion | 书架和划线同步（Worker 部署） |
| [a3100821009/weread-to-notion](https://github.com/a3100821009/weread-to-notion) | Notion | 书架/划线/统计同步 |
| [f1603206034/weread-notes](https://github.com/f1603206034/weread-notes) | GitHub+Notion | GitHub Actions 定时同步 |
| [ZhongJiaqi/weread-to-obsidian](https://github.com/ZhongJiaqi/weread-to-obsidian) | Obsidian | 含 Dataview frontmatter |
| [gnixner/weread-import](https://github.com/gnixner/weread-import)、[Yant2023/weread-obsidian](https://github.com/Yant2023/weread-obsidian) | Obsidian | 划线想法导出 |
| [huangcheng/weread-to-flomo](https://github.com/huangcheng/weread-to-flomo) | flomo | 划线/想法导出为 memo（走 flomo MCP） |
| [chucan1/weread-to-getnote](https://github.com/chucan1/weread-to-getnote) | Get 笔记 | 划线想法整理导入 |
| [TianLanhe/weread-readtime-for-lark](https://github.com/TianLanhe/weread-readtime-for-lark) | 飞书 | 每日阅读时长写入多维表格 |
| [TianLanhe/weread-readdata-for-tencent-doc](https://github.com/TianLanhe/weread-readdata-for-tencent-doc) | 腾讯文档 | 书架数据写入智能表格 |
| [chanity256/weread-export](https://github.com/chanity256/weread-export) | 本地 | macOS 每日定时导出器 |

#### 可视化 / 阅读画像

| 项目 | 说明 |
|---|---|
| [ZiGmaX809/Weread_ReadTime_Heatmap](https://github.com/ZiGmaX809/Weread_ReadTime_Heatmap) | GitHub 风格阅读时长热力图 |
| [lucis-yg/weread-dashboard](https://github.com/lucis-yg/weread-dashboard) | Vue 3 数据可视化看板 |
| [AstrophelXD/WeReadAura](https://github.com/AstrophelXD/WeReadAura) | neo-brutalism 风格只读 dashboard |
| [ktKongTong/wereto](https://github.com/ktKongTong/wereto) | 一键部署 Cloudflare 的公开阅读数据页 |
| [Kalmaegi/weread-analyzer](https://github.com/Kalmaegi/weread-analyzer) | 阅读画像分析 Web 项目 |
| [monsignorlaw1015/weread-report](https://github.com/monsignorlaw1015/weread-report)、[viewer12/weread-mirror](https://github.com/viewer12/weread-mirror)、[kejixiaoliang/read-persona](https://github.com/kejixiaoliang/read-persona) | 生成 HTML 阅读自画像/人格报告 |
| [LoloChak/weread-golden-quotes](https://github.com/LoloChak/weread-golden-quotes) | 提取金句生成每日展示页 |
| [SpaceTrave1/weread-deep-insights](https://github.com/SpaceTrave1/weread-deep-insights) | 从书架笔记挖掘阅读模式 |

#### 工作流 / AI 助手类

| 项目 | 说明 |
|---|---|
| [alchaincyf/huashu-weread](https://github.com/alchaincyf/huashu-weread) | 读书顾问 Skill（advisor/path/alchemy/review 四工作流） |
| [cocovs/weread-agent-chat](https://github.com/cocovs/weread-agent-chat) | 基于 API Key 的聊天应用（Pi Agent） |
| [Llxsfd/weread-review-web](https://github.com/Llxsfd/weread-review-web) | 划线复习工具（FastAPI + Vue3 + Edge-TTS 朗读） |
| [stefanxfy/weread-essay-skill](https://github.com/stefanxfy/weread-essay-skill) | 用书架与笔记生成读书 essay |
| [Eleven1111/weread-insight-skill](https://github.com/Eleven1111/weread-insight-skill) | 解释段落并沉淀为 Obsidian 理解卡片 |

#### Skill 增强变体

| 项目 | 说明 |
|---|---|
| [ChenyqThu/openclaw-weread-skill](https://github.com/ChenyqThu/openclaw-weread-skill)、[zhongyi-byte/openclaw-weread-skill](https://github.com/zhongyi-byte/openclaw-weread-skill) | OpenClaw 标准移植版 |
| [lovekeji-ai/agent-weread-skill](https://github.com/lovekeji-ai/agent-weread-skill) | 增量笔记导出（最近 N 天过滤） |
| [taxueseek/taxue-weread](https://github.com/taxueseek/taxue-weread) | CLI + 缓存 + 合并命令优化版 |
| [jerlinn/jerlin-weread](https://github.com/jerlinn/jerlin-weread) | 按需查单个接口，节省上下文 |

### 6.3 第一代：基于逆向接口（star 数更高）

#### 书籍导出 / 下载（⚠️ 版权敏感）

| 项目 | Stars | 说明 |
|---|---|---|
| [drunkdream/weread-exporter](https://github.com/drunkdream/weread-exporter) | ⭐2.4k | 导出 epub / pdf / mobi |
| [Algebra-FUN/WeReadScan](https://github.com/Algebra-FUN/WeReadScan) | ⭐1.0k | 扫描已购图书并下载 PDF |
| [lbq110/weread-exporter](https://github.com/lbq110/weread-exporter) | ⭐328 | Playwright + Canvas Hook 全本导出 Markdown |
| [magicdawn/weread-spy](https://github.com/magicdawn/weread-spy) | - | 用 Web 版生成 ePub |

#### 笔记同步（最成熟的品类）

| 项目 | Stars | 说明 |
|---|---|---|
| [malinkang/weread2notion-pro](https://github.com/malinkang/weread2notion-pro) | ⭐3.4k | 划线同步 Notion 增强版 |
| [malinkang/weread2notion](https://github.com/malinkang/weread2notion) | ⭐2.9k | 前作，划线同步 Notion |
| [zhaohongxuan/obsidian-weread-plugin](https://github.com/zhaohongxuan/obsidian-weread-plugin) | ⭐2.3k | Obsidian 官方社区插件 |
| [arry-lee/wereader](https://github.com/arry-lee/wereader) | ⭐958 | 功能全面的笔记助手（Python） |
| [Higurashi-kagome/wereader](https://github.com/Higurashi-kagome/wereader) | ⭐775 | 笔记导出浏览器扩展 |
| [sancijun/weread-toolbox](https://github.com/sancijun/weread-toolbox) | ⭐355 | 图文 Markdown 笔记导出工具箱 |

#### 墨水屏设备（KOReader / Kindle / Kobo）

| 项目 | Stars | 说明 |
|---|---|---|
| [finlater/weread.koplugin](https://github.com/finlater/weread.koplugin) | ⭐964 | KOReader 插件：读书/公众号 + 进度同步 + 时长上报 |
| [miumiupy98-art/miuread-koreader](https://github.com/miumiupy98-art/miuread-koreader) | ⭐673 | "觅阅"：Kindle 非官方 KOReader 插件 |

#### 公众号 RSS

| 项目 | Stars | 说明 |
|---|---|---|
| [cooderl/wewe-rss](https://github.com/cooderl/wewe-rss) | ⭐9.6k | 基于微信读书的公众号 RSS 生成（MIT，已归档） |
| [rachelos/we-mp-rss](https://github.com/rachelos/we-mp-rss) | ⭐5.0k | wewe-rss 后继：转 Markdown/PDF、Webhook/AI Agent 接入 |
| [able8/weread-hot-booklists](https://github.com/able8/weread-hot-booklists) | ⭐915 | 热门书单榜 / 热门笔记（Go） |

#### 增强工具包（官方 + 墨水屏双通道）

| 项目 | Stars | 说明 |
|---|---|---|
| [teng-lin/weread-omni](https://github.com/teng-lin/weread-omni) | ⭐678 | **生态内能力最全**：Agent、API、SDK、CLI、插件一体，40 项读写操作（基于墨水屏 API），支持公众号阅读、书架笔记读写、书籍导入 |

#### 自动阅读器（⚠️ 刷时长，违反用户协议，仅列事实存在）

| 项目 | Stars | 说明 |
|---|---|---|
| [DoooReyn/WxRead-WebAutoReader](https://github.com/DoooReyn/WxRead-WebAutoReader) | ⭐459 | Web 自动阅读刷时长 |
| [DoooReyn/WxReader](https://github.com/DoooReyn/WxReader) | ⭐335 | PySide6 桌面自动阅读器 |

---

## 7. 选型建议

### 7.1 按需求选通道

| 你的需求 | 推荐通道 | 参考实现 |
|---|---|---|
| 个人阅读数据查询、统计、报告 | **官方 Skill** | weread-cli、weread-dashboard |
| 划线/笔记同步 Notion、Obsidian、flomo、飞书 | 官方 Skill（新项目）或 Web 逆向（老项目） | weread2notion 系列、obsidian-weread-plugin |
| 给 AI Agent / MCP / Claude 接入读书能力 | **官方 Skill** 或 MCP | Tencent/WeChatReading、wong2/weread-mcp |
| 写操作（写入笔记、订阅管理、书籍导入） | 仅墨水屏逆向 | weread-omni |
| 书籍正文 / 已购图书导出 | 仅逆向（版权风险自担） | weread-exporter |
| 在 Kindle/Kobo 等设备上读微信读书的书 | 墨水屏逆向 | weread.koplugin |
| 公众号订阅 RSS | Web 逆向 | we-mp-rss |

### 7.2 自研 API 项目（本项目场景）的建议

若要在 `weread-api` 中自建 API 层：

1. **底座选官方 Key 通道**：稳定、合规、有版本升级机制；接口语义参考官方 `skills/` 文档（`/shelf/sync`、`/book/info` 等）
2. **SDK 实现参考**：[OpenWeRead](https://github.com/Ceelog/OpenWeRead)（TS、覆盖最全）、[weread-omni](https://github.com/teng-lin/weread-omni)（40 项操作、含读写）
3. **需要写操作再引入墨水屏通道**，做成可插拔 provider，与官方通道隔离，降低失效影响面
4. **缓存**：官方接口按个人账号维度返回全量数据（如书架全量），适合做本地缓存 + 增量刷新（参考 taxue-weread 的缓存设计）
5. **Key 安全**：`wrk-` Key 等同账号凭据，只放服务端环境变量，不要进前端/日志

---

## 8. 风险与合规提示

1. **官方通道**：API Key 绑定个人身份、数据仅本人可见；只读能力不会触碰平台数据红线，合规风险最低。但官方未提供 SLA，接口以 Agent 场景为主，未来策略可能调整。
2. **逆向通道**（Web / 墨水屏）：
   - 违反微信读书用户协议，**存在账号封禁风险**，历史上接口多次变动导致社区项目集体失效
   - 书籍导出类项目涉及**版权问题**，仅建议用于个人已购内容的备份场景
   - 自动刷时长类项目明确违规，不建议使用
3. **数据隐私**：书架、笔记、阅读时长均为个人敏感数据，自建服务对外暴露时务必做好鉴权（参考 wong2/weread-mcp 的 Bearer 鉴权设计）。

---

## 附录：全部参考链接

### 官方资源

- 微信读书 Skill 介绍页 / API Key 获取：<https://weread.qq.com/r/weread-skills>
- 官方 Skill 仓库（接口文档）：<https://github.com/Tencent/WeChatReading>
- 官方 Skill 安装包：<https://cdn.weread.qq.com/skills/weread-skills.zip>
- 内容开放平台（面向出版方）：<https://cp.weread.qq.com>
- 微信读书团队博客：<https://wereadteam.github.io>

### 分析文章

- InfoQ：微信读书开放了 API，但 90% 的用户没把 Skill 用到这种程度：<https://xie.infoq.cn/article/a282b19a23d07794bffebb464>
- QClaw 产品文档：微信读书 API Key 获取指南：<https://qclaw.qq.com/docs/211404800074477568>
- CSDN：微信读书 API 分析（2021，早期逆向）：<https://blog.csdn.net/mighty13/article/details/119257353>

### 生态索引

- awesome-weread（官方 Key 生态索引）：<https://github.com/BENZEMA216/awesome-weread>
- GitHub Topic: wechat-reading：<https://github.com/topics/wechat-reading>
- GitHub 搜索 "weread"（约 993 个仓库）：<https://github.com/search?q=weread&type=repositories>
