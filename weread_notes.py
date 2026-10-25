#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""weread_notes.py — 基于微信读书官方 Agent Gateway 的读书笔记获取工具

协议要点（详见 docs/weread-api-research.md、docs/weread-api-projects-deep-dive.md）:

    POST https://i.weread.qq.com/api/agent/gateway
    Authorization: Bearer wrk-xxxx
    Content-Type: application/json

    {"api_name": "/user/notebooks", "skill_version": "1.0.4", ...平铺参数}

    响应: JSON。errcode != 0 为业务错误; 携带 upgrade_info 时需升级 skill_version;
    HTTP 499 表示上游超时, 可重试。

用法:

    export WEREAD_API_KEY=wrk-xxxx     # https://weread.qq.com/r/weread-skills 获取
    python3 weread_notes.py notebooks              # 笔记本概览(哪些书做过笔记)
    python3 weread_notes.py export <bookId>        # 导出某本书的划线+想法(Markdown)
    python3 weread_notes.py export <bookId> -o out.md
    python3 weread_notes.py apis                   # 网关接口自描述(/_list)
    python3 weread_notes.py raw /user/notebooks count=3   # 任意接口直调(逃生口)

仅依赖 Python 标准库(>=3.9)。
"""

import argparse
import datetime as _dt
import json
import os
import sys
import time
import urllib.error
import urllib.request

GATEWAY_URL = "https://i.weread.qq.com/api/agent/gateway"
SKILL_VERSION = "1.0.5"  # 官方 Skill 当前版本(2026-10 实测); 升级时服务端会随响应返回 upgrade_info
KEY_PAGE = "https://weread.qq.com/r/weread-skills"


class WereadError(Exception):
    """网关返回的业务错误(errcode != 0 或 HTTP 异常)。"""

    def __init__(self, errcode, message):
        super().__init__("[errcode=%s] %s" % (errcode, message))
        self.errcode = errcode


class UpgradeRequiredError(Exception):
    """服务端要求升级 skill_version, 响应携带 upgrade_info。"""


class _Retryable(Exception):
    pass


def _mask(key):
    return (key[:8] + "..." + key[-4:]) if key and len(key) > 16 else "***"


class WereadClient:
    def __init__(self, api_key=None, base_url=None, timeout=30, max_attempts=3):
        self.api_key = (api_key or os.environ.get("WEREAD_API_KEY", "")).strip()
        if not self.api_key:
            sys.exit("缺少 API Key: 请 export WEREAD_API_KEY=wrk-... 或用 --key 传入。\n获取页面: %s" % KEY_PAGE)
        self.base_url = base_url or GATEWAY_URL
        self.timeout = timeout
        self.max_attempts = max_attempts

    def call(self, api_name, params=None):
        """调用一个 api_name, 返回解析后的 JSON(业务字段平铺在顶层)。"""
        body = {"api_name": api_name, "skill_version": SKILL_VERSION}
        if params:
            body.update({k: _smart_value(v) for k, v in params.items()})
        data = None
        for attempt in range(1, self.max_attempts + 1):
            try:
                data = self._post_once(body)
                break
            except _Retryable as e:
                if attempt == self.max_attempts:
                    raise WereadError("network", "多次重试后仍失败: %s" % e)
                time.sleep(0.15 * attempt)
        # 实测(2026-10): 升级提示为伴随性信号, 服务端仍会同时返回业务数据;
        # 仅当响应除 upgrade_info 外无任何内容时才视为阻断。
        if isinstance(data, dict) and data.get("upgrade_info"):
            info = data["upgrade_info"]
            msg = info.get("message") if isinstance(info, dict) else info
            has_payload = any(k for k in data if k not in ("upgrade_info", "errcode"))
            if not has_payload:
                raise UpgradeRequiredError(
                    "服务端要求升级 skill_version(当前 %s): %s" % (SKILL_VERSION, msg))
            sys.stderr.write("[!] 升级提示(已忽略, 数据正常返回): %s\n" % msg)
        errcode = data.get("errcode", 0) if isinstance(data, dict) else 0
        if errcode:
            msg = data.get("errmsg") or data.get("message") or data.get("errMsg") or data.get("msg")
            raise WereadError(errcode, msg or "未知错误")
        return data

    def _post_once(self, body):
        req = urllib.request.Request(
            self.base_url,
            data=json.dumps(body).encode("utf-8"),
            headers={
                "Authorization": "Bearer %s" % self.api_key,
                "Content-Type": "application/json",
                "User-Agent": "weread-notes-py/0.1",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                status, text = resp.status, resp.read().decode("utf-8")
        except urllib.error.HTTPError as e:
            status = e.code
            text = e.read().decode("utf-8", "replace")
            if status == 499:  # 上游超时, 专用可重试状态码
                raise _Retryable("upstream timeout (HTTP 499)")
        except urllib.error.URLError as e:
            raise _Retryable(str(e))
        try:
            data = json.loads(text) if text.strip() else {}
        except ValueError:
            raise WereadError("invalid_json", "HTTP %s 返回非 JSON: %s" % (status, text[:200]))
        if status >= 400 and not (isinstance(data, dict) and data.get("errcode")):
            raise WereadError("http_%s" % status, text[:200] or "HTTP %s" % status)
        return data


def _smart_value(v):
    """CLI 传入的都是字符串, 这里按 JSON 习惯做 int/bool 转换。"""
    if isinstance(v, str):
        s = v.strip()
        if s.isdigit():
            return int(s)
        if s.lower() in ("true", "false"):
            return s.lower() == "true"
    return v


def _fmt_time(ms_or_s):
    v = ms_or_s or 0
    if v <= 0:
        return ""
    if v > 1e12:  # 毫秒时间戳
        v /= 1000.0
    try:
        return _dt.datetime.fromtimestamp(v).strftime("%Y-%m-%d %H:%M")
    except (OverflowError, ValueError, OSError):
        return str(ms_or_s)


# ---------------------------------------------------------------- 笔记本概览

def fetch_notebooks(client, count=100, max_pages=20):
    """翻页拉全 /user/notebooks(游标为最后一条的 sort 字段)。"""
    items, last_sort = [], None
    for _ in range(max_pages):
        params = {"count": count}
        if last_sort is not None:
            params["lastSort"] = last_sort
        data = client.call("/user/notebooks", params)
        books = data.get("books") or []
        items.extend(books)
        if not data.get("hasMore") or not books:
            break
        last_sort = books[-1].get("sort")
        if last_sort is None:
            break
    return items


def cmd_notebooks(args, client):
    items = fetch_notebooks(client, count=args.page_size)
    if not items:
        print("没有做过任何笔记(空笔记本)。")
        return 0
    print("共 %d 本有笔记的书:\n" % len(items))
    print("%-4s %-14s %-34s %-10s %-8s %-8s %s" % ("#", "bookId", "书名", "划线", "笔记", "想法", "最近更新"))
    rows = sorted(
        items,
        key=lambda x: (x.get("bookmarkCount", 0) + x.get("noteCount", 0) + x.get("reviewCount", 0)),
        reverse=True,
    )
    for i, nb in enumerate(rows[: args.limit], 1):
        book = nb.get("book") or {}
        total = nb.get("bookmarkCount", 0) + nb.get("noteCount", 0) + nb.get("reviewCount", 0)
        title = (book.get("title") or "?") + (" *%d*" % total if total else "")
        print("%-4d %-14s %-34s %-10s %-8s %-8s %s" % (
            i,
            nb.get("bookId", "?"),
            _clip(title, 34),
            nb.get("bookmarkCount", 0),
            nb.get("noteCount", 0),
            nb.get("reviewCount", 0),
            _fmt_time(nb.get("updateTime") or book.get("updateTime")),
        ))
    if args.limit and len(rows) > args.limit:
        print("\n(仅显示前 %d 本, 共 %d 本)" % (args.limit, len(rows)))
    return 0


def _clip(s, n):
    s = str(s)
    return s if len(s) <= n else s[: n - 1] + "…"


# ---------------------------------------------------------------- 单书导出

def _chapter_map(chapters_raw):
    out = {}
    for c in chapters_raw or []:
        if isinstance(c, dict) and c.get("chapterUid") is not None:
            out[c.get("chapterUid")] = c.get("title") or ("第 %s 章" % c.get("chapterUid"))
    return out


def fetch_note_detail(client, book_id):
    """一次 /book/bookmarklist 拿全: 划线列表(updated) + 章节表(chapters) + 书籍信息(book)。"""
    data = client.call("/book/bookmarklist", {"bookId": book_id})
    bookmarks = data.get("updated") or []
    chapters = _chapter_map(data.get("chapters"))
    if not chapters:  # 个别响应不带 chapters 时回退专接口
        try:
            chapters = _chapter_map(client.call("/book/chapterinfo", {"bookId": book_id}).get("chapters"))
        except WereadError:
            pass
    return bookmarks, chapters, data.get("book") or {}


def fetch_my_reviews(client, book_id, count=100, max_pages=20):
    """我的想法(/review/list/mine, 注意参数是小写 bookid), synckey 翻页。失败不影响划线导出。"""
    reviews = []
    try:
        data = client.call("/review/list/mine", {"bookid": book_id, "count": count})
        for _ in range(max_pages):
            reviews.extend(data.get("reviews") or [])
            if not data.get("hasMore") or data.get("synckey") is None:
                break
            data = client.call("/review/list/mine",
                               {"bookid": book_id, "count": count, "synckey": data.get("synckey")})
    except (WereadError, UpgradeRequiredError) as e:
        print("(!) 想法列表获取失败, 已跳过: %s" % e, file=sys.stderr)
    return reviews


def build_markdown(book_id, book_info, chapters, bookmarks, reviews):
    lines = []
    title = (book_info or {}).get("title") or book_id
    author = (book_info or {}).get("author", "")
    lines.append("# 《%s》读书笔记" % title)
    lines.append("")
    if author:
        lines.append("> 作者: %s" % author)
    lines.append("> 导出时间: %s | 划线 %d 条 | 想法 %d 条" % (
        _dt.datetime.now().strftime("%Y-%m-%d %H:%M"), len(bookmarks), len(reviews)))
    lines.append("")

    if bookmarks:
        lines.append("## 划线")
        lines.append("")
        by_chapter = {}
        for bm in sorted(bookmarks, key=lambda b: (b.get("chapterUid", 0), str(b.get("createTime", 0)))):
            by_chapter.setdefault(bm.get("chapterUid", 0), []).append(bm)
        for cuid in sorted(by_chapter):
            name = chapters.get(cuid, ("第 %s 章" % cuid) if cuid else "未分章")
            lines.append("### %s" % name)
            lines.append("")
            for bm in by_chapter[cuid]:
                t = _fmt_time(bm.get("createTime"))
                lines.append("- %s%s" % (
                    bm.get("markText", "").strip(),
                    "  \n  `(%s)`" % t if t else "",
                ))
            lines.append("")

    if reviews:
        lines.append("## 我的想法")
        lines.append("")
        # 结构: {reviewId, review: {abstract(划线原文), content(想法文字), createTime, ...}}
        def _rv_key(rv):
            inner = rv.get("review") or {}
            return str(inner.get("createTime") or 0)

        for rv in sorted(reviews, key=_rv_key):
            inner = rv.get("review") or {}
            content = (inner.get("content") or "").strip()
            abstract = (inner.get("abstract") or "").strip()
            t = _fmt_time(inner.get("createTime"))
            if content:
                lines.append("- %s%s" % (content, ("  \n  `(%s)`" % t) if t else ""))
                if abstract:
                    lines.append("  > 原文: %s" % _clip(abstract, 100))
            elif abstract:
                lines.append("- %s%s" % (_clip(abstract, 100), ("  \n  `(%s)`" % t) if t else ""))
        lines.append("")

    if not bookmarks and not reviews:
        lines.append("(这本书没有可导出的划线或想法)")
    lines.append("")
    return "\n".join(lines)


def cmd_export(args, client):
    bookmarks, chapters, book_info = fetch_note_detail(client, args.book_id)
    if not book_info.get("title"):  # 回退: 从笔记本概览补书名
        for nb in fetch_notebooks(client, count=100):
            if str(nb.get("bookId")) == str(args.book_id):
                book_info = nb.get("book") or {}
                break
    reviews = fetch_my_reviews(client, args.book_id)
    md = build_markdown(args.book_id, book_info, chapters, bookmarks, reviews)

    if args.output:
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(md)
        print("已导出 %d 条划线 + %d 条想法 -> %s" % (len(bookmarks), len(reviews), args.output))
    else:
        print(md)
    return 0


# ---------------------------------------------------------------- 逃生口/自描述

def cmd_raw(args, client):
    params = {}
    for kv in args.params:
        if "=" not in kv:
            sys.exit("参数格式应为 k=v, 得到: %r" % kv)
        k, v = kv.split("=", 1)
        params[k] = v
    print(json.dumps(client.call(args.api_name, params), ensure_ascii=False, indent=2))
    return 0


def cmd_apis(args, client):
    try:
        print(json.dumps(client.call("/_list"), ensure_ascii=False, indent=2))
    except (WereadError, UpgradeRequiredError) as e:
        print("(/_list 不可用: %s)" % e, file=sys.stderr)
    return 0


# ---------------------------------------------------------------- 入口

def main(argv=None):
    parser = argparse.ArgumentParser(description="微信读书官方 API 读书笔记工具")
    parser.add_argument("--key", help="API Key(默认读环境变量 WEREAD_API_KEY, wrk- 前缀)")
    parser.add_argument("--base-url", help="覆盖网关地址(调试用)")
    parser.add_argument("--timeout", type=int, default=30)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("notebooks", help="笔记本概览: 哪些书做过笔记")
    p.add_argument("--limit", type=int, default=30)
    p.add_argument("--page-size", type=int, default=100)
    p.set_defaults(fn=cmd_notebooks)

    p = sub.add_parser("export", help="导出某本书的划线+想法为 Markdown")
    p.add_argument("book_id")
    p.add_argument("-o", "--output", help="写入文件(默认打印到终端)")
    p.set_defaults(fn=cmd_export)

    p = sub.add_parser("raw", help="任意 api_name 直调, 参数用 k=v 形式")
    p.add_argument("api_name")
    p.add_argument("params", nargs="*", help="如 count=3 lastSort=100")
    p.set_defaults(fn=cmd_raw)

    p = sub.add_parser("apis", help="网关接口自描述(/_list)")
    p.set_defaults(fn=cmd_apis)

    args = parser.parse_args(argv)
    client = WereadClient(api_key=args.key, base_url=args.base_url, timeout=args.timeout)
    print("[key %s -> %s]" % (_mask(client.api_key), client.base_url), file=sys.stderr)
    try:
        return args.fn(args, client)
    except UpgradeRequiredError as e:
        print("需要升级: %s" % e, file=sys.stderr)
        return 3
    except WereadError as e:
        print("调用失败: %s" % e, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
