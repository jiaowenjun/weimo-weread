# 微信读书官方 Agent Gateway 能力清单（分门别类）

> **数据来源**：网关 `/_list` 接口自描述（2026-10-25 实测拉取），17 个业务接口 + 1 个元接口
> **适用对象**：基于官方 API Key（`wrk-`）开发生态工具的开发者
> **配套实现**：[`weread_notes.py`](../weread_notes.py)（Python 参考版）与 [`src/`](../src/)（Rust 版，`cargo run --` 同名子命令），`raw` 子命令可直调以下任意接口

---

## 总览

| 分类 | 接口数 | 能干什么 |
|---|---|---|
| [1. 书籍检索与元信息](#1-书籍检索与元信息) | 3 | 找书、看详情、看目录 |
| [2. 个人阅读库](#2-个人阅读库) | 2 | 书架全量、单书进度 |
| [3. 笔记与想法（个人产出）](#3-笔记与想法个人产出) | 3 | 划线、想法的完整导出 |
| [4. 公开社区内容](#4-公开社区内容) | 5 | 书评、热门划线、他人想法 |
| [5. 推荐与发现](#5-推荐与发现) | 3 | 个性化推荐、相似书、好友动态 |
| [6. 阅读统计与画像](#6-阅读统计与画像) | 1 | 时长/天数/偏好/勋章/排行 |
| [7. 系统元接口](#7-系统元接口) | 1 | 接口自描述 |
| [8. 组合能力（接口编排）](#8-组合能力接口编排) | — | 报告生成、知识画像等高阶玩法 |
| [9. 能力边界（做不到的事）](#9-能力边界做不到的事) | — | 明确禁区，避免设计返工 |

**通用调用约定**：`POST https://i.weread.qq.com/api/agent/gateway`，`Authorization: Bearer wrk-...`，body 为 `{"api_name": "...", "skill_version": "1.0.5", ...平铺参数}`；响应 `errcode != 0` 为业务错误；除 `/book/info` 和 `/book/chapterinfo` 外均需登录态（API Key）。

---

## 1. 书籍检索与元信息

解决"找书"和"认识书"。这是仅有的两个**免登录**接口（元数据是公开的）。

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/store/search` | 书城搜索 | `keyword`（必填）、`scope`、`maxIdx`、`count` | scope 八种：0=全部 10=电子书 14=听书 6=作者 12=全文 13=书单 2=公众号 4=文章。⚠️ 响应顶层是 `results[]` 分组（每组 title+books），不是 `books[]` |
| `/book/info` | 书籍详情 | `bookId`（必填） | 返回书名/作者/译者/出版社/ISBN/字数/评分(`newRating` 满分 1000)/评分人数/分类/简介/封面/详情页深链 |
| `/book/chapterinfo` | 章节目录 | `bookId`（必填） | 返回 `chapters[]`（chapterUid + title），是按章查笔记/热门划线的前置依赖 |

**典型用法**：书名 → `/store/search` 拿 bookId（多候选时取作者完全匹配 + `readingCount` 最高者）→ `/book/info` 看详情。实例见[上一轮演示](./weread-api-projects-deep-dive.md#25-实测补充2026-10-25用真实-key-直连网关验证)。

## 2. 个人阅读库

解决"我有哪些书、读到哪了"。

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/shelf/sync` | 书架全量同步 | 无 | 一次返回四块：`books[]`（电子书，含 `readUpdateTime` 最近阅读时间、`finishReading`）、`albums[]`（听书专辑）、`mp`（公众号文章收藏入口）、`archive[]`（自建书单分组） |
| `/book/getprogress` | 单书阅读进度 | `bookId`（必填） | 返回进度百分比、当前章节、累计阅读时长（进度是整数百分比，`100` 才是读完） |

**典型用法**："我在读哪几本" = `/shelf/sync` 按 `readUpdateTime` 倒序取前 N → 逐本 `/book/getprogress`。

## 3. 笔记与想法（个人产出）

核心价值区——个人知识数据的完整导出链路，三个接口正好构成流水线：

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/user/notebooks` | 笔记本概览 | `count`、`lastSort`（游标） | 有笔记的书全列表，每本含 `bookmarkCount`/`noteCount`/`reviewCount`/`readingProgress`；响应头部有 `totalBookCount`/`totalNoteCount` 汇总 |
| `/book/bookmarklist` | 单书划线列表 | `bookId`（必填） | 一次返回 `updated[]`（划线）+ `chapters`（章节表）+ `book`（书籍信息），**单书导出只需再补一次 `/review/list/mine`**；还有 `removed[]`（已删划线）和 `synckey`（增量） |
| `/review/list/mine` | 我的想法 | `bookid`（⚠️ 小写 d，必填）、`synckey`、`count` | 列表项双层嵌套：文字在内层 `review.content`，对应划线原文在 `review.abstract`，`createTime`/`chapterUid` 也都在内层 |

**典型用法**（已在 `weread_notes.py` 实现）：`notebooks` 选书 → `bookmarklist` + `review/list/mine` → 生成按章节分组、带时间戳和原文引用的 Markdown。

## 4. 公开社区内容

解决"别人怎么读这本书"。全部需要 bookId，多数还要 chapterUid（先过 `/book/chapterinfo`）。

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/review/list` | 公开书评列表 | `bookId`、`reviewListType`（0=全部 1=推荐 2=不行 3=最新 4=一般）、`count`、`maxIdx`、`synckey` | 响应还带好友评论数、大 V 推荐度等社交元数据 |
| `/review/single` | 单条想法详情 | `reviewId`（必填）、评论/点赞数量与排序、`synckey` | 拿一条想法的完整评论区和点赞列表 |
| `/book/bestbookmarks` | 全书热门划线 | `bookId`、`chapterUid`（0=全书）、`synckey` | 最多 20 条，含划线文本和标注人数，按热度排序 |
| `/book/underlines` | 章节划线热度统计 | `bookId`、`chapterUid`（必填）、`synckey` | 每条划线的人数/得分/类型，**不含文本**——适合画"章节热度地图" |
| `/book/readreviews` | 划线范围下的想法 | `bookId`、`chapterUid`、`reviews[]`（range 数组，必填） | "这句划线下面大家说什么"，每个划线最多 20 条 |

**典型用法**：读书时看"全书最被共鸣的 20 句话"（bestbookmarks）；对标他人理解（readreviews）。

## 5. 推荐与发现

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/book/recommend` | 个性化推荐（为你推荐） | `count`、`maxIdx` | 基于个人阅读历史的官方推荐流 |
| `/book/similar` | 相似书推荐 | `bookId`、`count`、`maxIdx`、`sessionId` | "读完这本读什么"，翻页需回传 `sessionId` |
| `/discover/interact/type3` | 好友在读动态 | `count`、`maxIdx`、`synckey` | 发现页朋友读书卡片流，按时间倒序 |

**典型用法**：书荒时拉 recommend；读书顾问类工具用"书架+笔记交叉分析"过滤掉已读书目后再推荐（huashu-weread 的核心玩法）。

## 6. 阅读统计与画像

单接口但信息量最大的一个：

| 接口 | 功能 | 关键参数 | 要点 |
|---|---|---|---|
| `/readdata/detail` | 阅读统计 | `mode`（weekly/monthly/annually/overall）、`baseTime`（传历史时间戳可查历史周期） | 返回 30+ 字段：时长/天数/日均/同比(`compare`)/最长纪录/时间分布(`preferTime`)/分类偏好/最爱作者/最爱出版社/常读书目(`preferBooks`)/阅读节奏(`readRate`)/听书时长/读书会排名(`rank`)/勋章(`medals`)/注册时间 |

**典型用法**：年度阅读报告、阅读热力图、偏好画像的原始数据源，一个接口几乎够用。

## 7. 系统元接口

| 接口 | 功能 | 要点 |
|---|---|---|
| `/_list` | 网关接口自描述 | 返回所有接口的参数名/类型/必填/默认值/响应字段。**官方新增接口时这里是第一发现入口**，建议封装层启动时拉取缓存 |

## 8. 组合能力（接口编排）

单接口是原子，编排起来才是产品。社区已验证的组合玩法：

| 高阶功能 | 编排方式 | 验证项目 |
|---|---|---|
| 单书笔记导出（Markdown/Obsidian/Notion） | notebooks → bookmarklist + review/list/mine | weread_notes.py、obsidian 系插件 |
| 阅读概况卡片 | shelf/sync → 排序 → N × getprogress | OpenWeRead 的 `profile` 模块 |
| 阅读年度报告/自画像 | readdata/detail(overall+annually) + notebooks 汇总 | weread-report、read-persona 等 |
| 知识地图与缺口推荐 | shelf/sync × user/notebooks 交叉（书架=兴趣，笔记=真读过）→ store/search 验证上架 | huashu-weread 的 advisor 工作流 |
| 书架清单管理 | shelf/sync 的 archive[] + books[] 全量导出 | 各类书单导出工具 |
| 社交读书理解 | bestbookmarks + readreviews + review/list 聚合 | 社区尚未充分开发 |
| 好友动态时间线 | discover/interact/type3 翻页 | 生态中最少人做的区域 |

## 9. 能力边界（做不到的事）

设计产品前先明确禁区，避免返工：

1. **全部只读**：不能加/移书架、不能写笔记想法、不能收藏书单、不能订阅。写操作只在墨水屏逆向接口存在（weread-omni 路线，官方未开放）
2. **无书籍正文**：所有接口都不返回章节正文。全文搜索（scope=12）只返回命中位置，不返回内容；导出整本书是官方明确不支持的能力
3. **仅本人数据**：书架/笔记/统计都是 API Key 持有者本人的；他人的数据只有"主动公开"部分（书评、热门划线、好友在读卡片）
4. **公众号文章只有目录**：`/shelf/sync` 的 `mp` 只是收藏入口，无文章正文接口
5. **听书数据有限**：专辑元数据在书架里有，但无单独的听书进度/笔记接口
6. **无账户与订阅管理**：会员状态、付费购书、体验卡等均不在网关范围内
7. **无推送/Webhook**：只有轮询；增量同步靠各接口的 `synckey` 字段自行实现

---

## 附：分页游标速查表

各接口翻页游标字段不统一，封装时最容易出错的地方：

| 接口 | 游标 | 语义 |
|---|---|---|
| `/user/notebooks` | `lastSort` | 上一页最后一条的 `sort` 值 |
| `/store/search` | `maxIdx` | 偏移量 |
| `/review/list` | `maxIdx` + `synckey` | 偏移 + 会话 |
| `/review/list/mine` | `synckey` | 单游标 |
| `/book/recommend` | `maxIdx` | 偏移量 |
| `/book/similar` | `maxIdx` + `sessionId` | 偏移 + 会话 |
| `/discover/interact/type3` | `maxIdx`(=回包 `nextMaxIdx`) + `synckey` | 偏移 + 刷新游标 |
| `/book/bestbookmarks`、`/book/underlines`、`/review/single` | `synckey` | 增量同步（0=全量） |

> 注：`synckey` 在笔记类接口中兼作**增量同步**用途——保存上次响应的 synckey，下次带上即可只拿变化部分（配合 `removed[]` 处理删除），这是做定时同步工具的关键机制。
