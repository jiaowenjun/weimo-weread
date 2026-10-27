# weread-notes-server · 管理后台

基于 [Ant Design Pro Components](https://procomponents.ant.design/)（ProLayout / ProTable / ModalForm）+ Vite + React + pnpm 的管理页面，对接 `weread-notes-server` 的 `/api/v1` 接口。

## 功能

| 页面 | 功能 |
|---|---|
| **概览** `/` | 服务状态、导出队列、已导出书数、熔断器；阅读统计（时长/天数/读完数/最爱作者，来自官方 `/readdata/detail`）；最近在读封面卡片；最近任务 |
| **书架** `/shelf` | 官方 `/shelf/sync` 代理：封面、进度条、读完标记、最近阅读时间排序；行点击打开书籍详情抽屉（元信息 + 目录） |
| **笔记** `/notes` | 笔记本列表（划线/笔记/想法计数 + 最近更新，支持搜索）→ 单书笔记面板（按章节分组的划线时间线 + 我的想法）；「保存此书」秒级完成（wait_timeout 同步等待），笔记原始 JSON 长期保存在服务端 SQLite |
| **导出任务** `/exports` | 任务列表（状态筛选、游标翻页）、新建保存（全部书/指定 bookId/强制重抓）、详情抽屉（SSE 进度推流 + 轮询降级、按书结果）、删除任务（已保存笔记保留） |

右上角「连接」配置 API 地址与**微信读书 API Key**（BYOK，必填；存本浏览器 localStorage，逐请求经 `X-Weread-Key` 头传给后端，服务器不保存 Key）：

- API 地址默认 `/api/v1`（同源部署或开发期 Vite 代理，无需 CORS）；跨源填完整地址（如 `http://192.168.1.10:8081/api/v1`），需服务器设置 `WEREAD_SERVER_CORS_ORIGINS`
- Key 获取页面: https://weread.qq.com/r/weread-skills；SSE 链接自动走 `?key=` 传参
- 未配置 Key 时概览页会出现引导提示

## 开发

```bash
# 1. 启动后端（仓库根目录，默认 127.0.0.1:8081；BYOK 无需任何 Key 环境变量）
cargo run --bin weread-notes-server

# 2. 启动前端（另一个终端，Vite 已配置 /api 与 /healthz 代理到 8081）
cd web && ppm install && pnpm dev   # 打开 http://localhost:5174
```

## 构建与部署

```bash
pnpm build   # tsc 类型检查 + vite 打包到 dist/
```

`dist/` 是纯静态文件：同源部署（推荐，nginx 托管 dist 并把 `/api`、`/healthz` 反代到 8081，SPA 配历史回退）或跨源部署（服务器开 CORS + 前端「连接」填完整地址）。

## 代码结构

```
src/
  api.ts                    # fetch 封装：base/微信读书Key（BYOK）、错误信封解析、SSE 直链
  types.ts                  # 与 src/server/service.rs 响应结构一一对应
  labels.tsx                # 状态中文标签、时间戳/时长格式化
  App.tsx                   # ProLayout 布局 + 路由 + 连接设置
  pages/
    Overview.tsx            # 概览（统计字段按别名容错取值）
    Shelf.tsx               # 书架 + 书籍详情抽屉
    Notes.tsx               # 笔记本列表 + 单书笔记面板 + 一键保存
    Exports.tsx             # 导出任务（ProTable + SSE 详情抽屉）
  components/
    CreateExportModal.tsx   # 新建保存任务（全部书 / 指定 bookId / 强制重抓）
    TaskDetailDrawer.tsx    # 任务详情抽屉（SSE 进度 + 按书结果）
```

## 已知取舍

- 阅读统计的官方字段名未列全 schema，概览页按别名容错匹配，取不到显示 `—`
- 已保存的笔记原始 JSON 暂无读回端点，笔记浏览走实时代理（TTL 缓存）
