#!/usr/bin/env bash
#
# Greenlife 备份：PostgreSQL 全库 + 物种照片目录（UPLOAD_PATH）
#
# 用法：
#   ./utils/backup.sh [-d 备份根目录] [-k 保留份数]
#
#   环境变量优先于 backend/.env，可以临时覆盖：
#   DATABASE_URL=... UPLOAD_PATH=... ./utils/backup.sh
#
# 产物（每次一个目录，db 和 uploads 分开，方便只恢复其中一样）：
#   $BACKUP_DIR/greenlife-YYYYmmdd-HHMMSS/
#   ├── db.dump          pg_dump 自定义格式（自带压缩，pg_restore 可读）
#   ├── uploads.tar.gz   UPLOAD_PATH 的内容，归档内顶层目录就是它的 basename
#   └── manifest.txt     时间、主机、commit、文件数、体积，恢复前先看这个
#
# 默认 BACKUP_DIR=<仓库根>/backups，KEEP=14（0 表示永不清理）。
#
# 定时执行，二选一：
#   a) systemd timer（推荐）：见同目录 greenlife-backup.service / .timer
#   b) crontab，每天 3:10：
#      10 3 * * * /srv/greenlife/utils/backup.sh >> /var/log/greenlife-backup.log 2>&1
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "错误：$*" >&2; exit 1; }
info() { echo "[$(date '+%F %T')] $*"; }

usage() {
    sed -n '3,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

# 读 backend/.env，但不覆盖已经在环境里的变量（调用方临时覆盖的优先）。
load_env_file() {
    local file="$1" line key val
    [ -r "$file" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in '' | '#'*) continue ;; esac
        key="${line%%=*}"
        case "$key" in '' | *[!A-Za-z0-9_]*) continue ;; esac
        val="${line#*=}"
        val="${val%\"}"; val="${val#\"}"
        val="${val%\'}"; val="${val#\'}"
        [ -n "${!key-}" ] && continue
        export "$key=$val"
    done <"$file"
}

# 打印连接串时抹掉密码，避免日志泄露。
mask_url() {
    local url="$1"
    if [[ $url =~ ^([^:]+://[^:/@]*):[^@]*@(.*)$ ]]; then
        printf '%s:***@%s' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    else
        printf '%s' "$url"
    fi
}

BACKUP_DIR="${BACKUP_DIR:-$ROOT_DIR/backups}"
KEEP="${KEEP:-14}"

while [ $# -gt 0 ]; do
    case "$1" in
        -d | --dir) [ $# -ge 2 ] || die "$1 需要一个参数"; BACKUP_DIR="$2"; shift 2 ;;
        -k | --keep) [ $# -ge 2 ] || die "$1 需要一个参数"; KEEP="$2"; shift 2 ;;
        -h | --help) usage 0 ;;
        *) die "未知参数：$1（用 --help 看用法）" ;;
    esac
done

case "$KEEP" in '' | *[!0-9]*) die "KEEP 必须是非负整数，收到：$KEEP" ;; esac

load_env_file "$ROOT_DIR/backend/.env"

[ -n "${DATABASE_URL:-}" ] || die "DATABASE_URL 未设置（backend/.env 或环境变量）"
[ -n "${UPLOAD_PATH:-}" ] || die "UPLOAD_PATH 未设置（backend/.env 或环境变量）"
command -v pg_dump >/dev/null || die "找不到 pg_dump，请先装 PostgreSQL 客户端"
command -v pg_restore >/dev/null || die "找不到 pg_restore，请先装 PostgreSQL 客户端"

# 统一成不带结尾斜杠的绝对路径，后面 dirname/basename 才不会被末尾斜杠骗到。
while [ "${UPLOAD_PATH%/}" != "$UPLOAD_PATH" ]; do UPLOAD_PATH="${UPLOAD_PATH%/}"; done
[ -d "$UPLOAD_PATH" ] || die "UPLOAD_PATH 不存在或不是目录：$UPLOAD_PATH"

mkdir -p "$BACKUP_DIR"
BACKUP_DIR="$(cd "$BACKUP_DIR" && pwd)"

# 备份目录落在 uploads 里面时，tar 会把自己正在写的归档也打进去。
case "$BACKUP_DIR/" in
    "$UPLOAD_PATH"/*) die "BACKUP_DIR 不能位于 UPLOAD_PATH 之内：$BACKUP_DIR" ;;
esac

# 同一时刻只跑一个：并发 pg_dump 既浪费也可能让清理逻辑删错东西。
exec 9>"$BACKUP_DIR/.lock"
flock -n 9 || die "另一个备份正在进行（$BACKUP_DIR/.lock 被占用）"

STAMP="$(date '+%Y%m%d-%H%M%S')"
DEST="$BACKUP_DIR/greenlife-$STAMP"
[ -e "$DEST" ] && die "目标目录已存在：$DEST（同一秒内重复执行？）"
mkdir -p "$DEST"

# 中途失败就删掉半成品，免得清理策略把残缺备份当成一份正常备份留着。
DONE=0
cleanup() {
    if [ "$DONE" -ne 1 ]; then
        echo "备份失败，删除未完成的目录：$DEST" >&2
        rm -rf "$DEST"
    fi
}
trap cleanup EXIT

info "备份到：$DEST"
info "数据库：$(mask_url "$DATABASE_URL")"
info "照片目录：$UPLOAD_PATH"

info "导出数据库…"
pg_dump --format=custom --no-owner --no-acl --file="$DEST/db.dump" "$DATABASE_URL"

# 归档内路径是 "uploads/..." 这种相对路径（-C 到父目录再打 basename），
# 恢复时解到父目录即可，也顺手避免了归档里出现绝对路径条目。
UPLOAD_PARENT="$(dirname "$UPLOAD_PATH")"
UPLOAD_NAME="$(basename "$UPLOAD_PATH")"
UPLOAD_FILES="$(find "$UPLOAD_PATH" -type f | wc -l)"

info "打包照片（$UPLOAD_FILES 个文件）…"
tar -czf "$DEST/uploads.tar.gz" -C "$UPLOAD_PARENT" "$UPLOAD_NAME"

# 体积小，顺手校验一遍，免得留下一份打不开的备份。
info "校验归档…"
pg_restore --list "$DEST/db.dump" >/dev/null || die "db.dump 无法被 pg_restore 读取"
gzip -t "$DEST/uploads.tar.gz" || die "uploads.tar.gz 损坏"

GIT_COMMIT="$(git -C "$ROOT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)"
{
    echo "created_at=$(date '+%F %T %z')"
    echo "host=$(hostname)"
    echo "git_commit=$GIT_COMMIT"
    echo "database_url=$(mask_url "$DATABASE_URL")"
    echo "pg_dump_version=$(pg_dump --version | awk '{print $3}')"
    echo "upload_path=$UPLOAD_PATH"
    echo "upload_files=$UPLOAD_FILES"
    echo "db_dump_bytes=$(stat -c %s "$DEST/db.dump")"
    echo "uploads_archive_bytes=$(stat -c %s "$DEST/uploads.tar.gz")"
} >"$DEST/manifest.txt"

chmod 700 "$DEST"
chmod 600 "$DEST"/*

DONE=1

if [ "$KEEP" -gt 0 ]; then
    # 目录名带时间戳，逆序排列后第 KEEP+1 份起就该删。
    find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name 'greenlife-*' |
        sort -r | tail -n "+$((KEEP + 1))" |
        while IFS= read -r old; do
            info "清理旧备份：$(basename "$old")"
            rm -rf "$old"
        done
fi

info "完成：$(du -sh "$DEST" | cut -f1)  $DEST"
