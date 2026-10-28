#!/usr/bin/env python3
"""把 record/ 目录里的鸟调 GPX 逐份导入网站。

走的是前端同款的后台接口（全部带 `/api` 前缀）：

    POST /api/auth/login                      登录，换 JWT
    GET  /api/bird-surveys                    列出已有鸟调（用 source 去重）
    POST /api/staff/bird-surveys/import       上传 GPX 生成一次鸟调（STAFF / ADMIN）

文件名形如 `20250304北大鸟调晴.gpx`：8 位数字是鸟调日期，`北大鸟调` 后面是天气。
领队文件名里没有，所以由 `--leader` 统一指定。

去重按「原始文件名」（后端存进 bird_surveys.source 的那一列）比对：已经导入过的
文件直接跳过，重复执行不会产生重复鸟调。确实要重导时加 `--force`。

只用标准库：服务器上不必先装 requests。

用法：

    export GREENLIFE_PASSWORD='...'
    ./utils/import_records.py --username admin --leader 张三
    ./utils/import_records.py --username admin --leader 张三 --dry-run

    # 线上经 nginx 暴露在域名下时
    ./utils/import_records.py --base-url https://example.org/api --username admin --leader 张三
"""

import argparse
import getpass
import json
import os
import re
import secrets
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from pathlib import Path

# 8 位日期。用前后非数字的断言，避免把文件名前缀里「2026年」之类的数字粘进来。
SURVEY_DATE_RE = re.compile(r"(?<!\d)(20\d{6})(?!\d)")
# 天气：`北大鸟调` 和 `.gpx` 之间的部分（晴 / 阴转小雨 / 多云 …）。
WEATHER_RE = re.compile(r"北大鸟调(.*?)\.gpx$")

# 列表接口单页上限就是 1000（backend/src/taxonomy_handlers.rs 的 MAX_LIMIT）。
PAGE_LIMIT = 1000

# 线上前面可能挂着 Cloudflare（见 README 的「Cloudflare Tunnel」）。它的 Browser
# Integrity Check 会拦掉非浏览器的 User-Agent —— urllib 默认的
# `Python-urllib/3.x` 会收到 `403 error code: 1010`，请求根本到不了 nginx。
# 这里固定用一条常规桌面 Chrome 的 UA，与浏览器发出的请求同形。
USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"
)


class ApiError(Exception):
    """接口调用失败。消息里带上方法、URL 和响应体，方便直接定位。"""


def request(url, *, method="GET", token=None, body=None, content_type=None):
    """发一个请求并把响应按 JSON 解析。"""
    req = urllib.request.Request(url, data=body, method=method)
    req.add_header("User-Agent", USER_AGENT)
    req.add_header("Accept", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    if content_type:
        req.add_header("Content-Type", content_type)
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as err:
        detail = err.read().decode("utf-8", "replace").strip()
        raise ApiError(f"{method} {url} -> HTTP {err.code} {detail}") from None
    except urllib.error.URLError as err:
        raise ApiError(f"{method} {url} 连不上：{err.reason}") from None


def login(base_url, username, password):
    """POST /auth/login，返回 token。"""
    body = json.dumps({"username": username, "password": password}).encode("utf-8")
    data = request(
        f"{base_url}/auth/login",
        method="POST",
        body=body,
        content_type="application/json",
    )
    token = data.get("token")
    if not token:
        raise ApiError("登录接口没有返回 token")
    return token


def existing_sources(base_url, token):
    """翻完所有鸟调，收集已导入的原始文件名。"""
    sources = set()
    offset = 0
    while True:
        query = urllib.parse.urlencode({"limit": PAGE_LIMIT, "offset": offset})
        data = request(f"{base_url}/bird-surveys?{query}", token=token)
        items = data.get("items") or []
        for item in items:
            source = item.get("source")
            if source:
                sources.add(source)
        offset += len(items)
        if not items or offset >= data.get("total", 0):
            return sources


def parse_filename(path):
    """从文件名解析出 (鸟调日期, 天气)。日期解析不出来时抛 ValueError。"""
    date_match = SURVEY_DATE_RE.search(path.stem)
    if date_match is None:
        raise ValueError("文件名里没有 8 位日期（形如 20250304）")
    raw = date_match.group(1)
    try:
        survey_date = datetime.strptime(raw, "%Y%m%d").date()
    except ValueError:
        raise ValueError(f"文件名里的日期不是合法日期：{raw}") from None

    weather_match = WEATHER_RE.search(path.name)
    weather = (weather_match.group(1).strip() if weather_match else "") or None
    return survey_date.isoformat(), weather


def multipart_body(fields, filename, content):
    """拼 multipart/form-data 请求体：文本字段 + 一个 `file`。"""
    boundary = "----greenlife" + secrets.token_hex(16)
    body = bytearray()
    for name, value in fields:
        body += f"--{boundary}\r\n".encode()
        body += f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode()
        body += value.encode("utf-8") + b"\r\n"
    body += f"--{boundary}\r\n".encode()
    # filename 直接写 UTF-8 原文：浏览器就是这么发的，后端（actix-multipart）按
    # RFC 7578 原样解析，source 里存下来的才是中文原名。
    body += (
        f'Content-Disposition: form-data; name="file"; filename="{filename}"\r\n'
    ).encode("utf-8")
    body += b"Content-Type: application/gpx+xml\r\n\r\n"
    body += content + b"\r\n"
    body += f"--{boundary}--\r\n".encode()
    return bytes(body), f"multipart/form-data; boundary={boundary}"


def import_survey(base_url, token, path, leader, survey_date, weather, note):
    """POST /staff/bird-surveys/import，返回这次鸟调的详情。"""
    fields = [("leader", leader), ("survey_date", survey_date)]
    if weather:
        fields.append(("weather", weather))
    if note:
        fields.append(("note", note))
    body, content_type = multipart_body(fields, path.name, path.read_bytes())
    return request(
        f"{base_url}/staff/bird-surveys/import",
        method="POST",
        token=token,
        body=body,
        content_type=content_type,
    )


def main():
    parser = argparse.ArgumentParser(
        description="把 record/ 里的鸟调 GPX 导入网站（走 /api 后台接口）。"
    )
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:8080/api",
        help="接口前缀，默认 %(default)s（线上经 nginx 时写 https://域名/api）",
    )
    parser.add_argument("--username", required=True, help="有 STAFF / ADMIN 权限的账号")
    parser.add_argument(
        "--password",
        help="密码；不给就读环境变量 GREENLIFE_PASSWORD，再没有就交互输入",
    )
    parser.add_argument("--leader", required=True, help="「领队」，导入接口必填，所有文件共用")
    parser.add_argument("--note", help="可选的统一备注，写进每次鸟调的表头")
    parser.add_argument(
        "--record-dir",
        default=str(Path(__file__).resolve().parent.parent / "record"),
        help="GPX 目录，默认 %(default)s",
    )
    parser.add_argument("--force", action="store_true", help="不跳过已导入的文件，强制重导")
    parser.add_argument("--dry-run", action="store_true", help="只解析文件名并打印，不调接口")
    args = parser.parse_args()

    base_url = args.base_url.rstrip("/")
    record_dir = Path(args.record_dir)
    if not record_dir.is_dir():
        parser.error(f"目录不存在：{record_dir}")

    files = sorted(record_dir.glob("*.gpx"))
    if not files:
        parser.error(f"{record_dir} 里没有 .gpx 文件")

    # 先全部解析一遍：日期解析不出来的文件直接报出来，不浪费登录和上传。
    planned = []
    failures = 0
    for path in files:
        try:
            survey_date, weather = parse_filename(path)
        except ValueError as err:
            print(f"✗ {path.name}：{err}", file=sys.stderr)
            failures += 1
            continue
        planned.append((survey_date, weather, path))
    planned.sort(key=lambda item: (item[0], item[2].name))

    if args.dry_run:
        for survey_date, weather, path in planned:
            print(f"· {survey_date} {weather or '-'} {path.name}")
        print(f"\n共 {len(planned)} 个文件（dry-run，未调用接口）")
        return 1 if failures else 0

    password = args.password or os.environ.get("GREENLIFE_PASSWORD")
    if not password:
        password = getpass.getpass(f"{args.username} 的密码：")

    try:
        token = login(base_url, args.username, password)
        print(f"登录成功：{base_url}")
        imported = existing_sources(base_url, token) if not args.force else set()
    except ApiError as err:
        print(f"✗ {err}", file=sys.stderr)
        return 1

    skipped = 0
    created = 0
    for survey_date, weather, path in planned:
        if path.name in imported:
            print(f"- 跳过（已导入）：{path.name}")
            skipped += 1
            continue
        try:
            detail = import_survey(
                base_url, token, path, args.leader, survey_date, weather, args.note
            )
        except ApiError as err:
            print(f"✗ {path.name}：{err}", file=sys.stderr)
            failures += 1
            continue
        created += 1
        survey = detail.get("survey") or {}
        print(
            f"✓ {survey_date} {weather or '-'} {path.name}："
            f"{len(detail.get('records') or [])} 条明细，"
            f"{survey.get('species_count', 0)} 种"
        )

    print(f"\n导入 {created} 个，跳过 {skipped} 个，失败 {failures} 个")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
