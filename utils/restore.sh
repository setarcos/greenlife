#!/usr/bin/env bash
#
# Greenlife 恢复：把 backup.sh 的产物写回数据库和 uploads 目录
#
# 用法：
#   ./utils/restore.sh <备份目录> [选项]
#
# 选项：
#   -y, --yes        跳过交互确认（脚本/无人值守时用）
#   --db-only        只恢复数据库
#   --uploads-only   只恢复照片目录
#   --wipe-uploads   解包前先删空 UPLOAD_PATH（默认是合并覆盖，不会删掉多出来的文件）
#
#   环境变量优先于 backend/.env：
#   DATABASE_URL=... UPLOAD_PATH=... ./utils/restore.sh backups/greenlife-20260101-031000 -y
#
# ⚠️ 数据库恢复用的是 pg_restore --clean --if-exists，会把目标库里当前所有对象
#    删掉再按备份重建。执行前先停服务：
#        systemctl stop greenlife
#      恢复完再看一眼迁移状态（备份里的 __diesel_schema_migrations 是备份那一刻的，
#      比代码旧的话需要补跑迁移）：
#        cd backend && diesel migration run
#        systemctl start greenlife
#
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "错误：$*" >&2; exit 1; }
info() { echo "[$(date '+%F %T')] $*"; }

usage() {
    sed -n '3,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

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

mask_url() {
    local url="$1"
    if [[ $url =~ ^([^:]+://[^:/@]*):[^@]*@(.*)$ ]]; then
        printf '%s:***@%s' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    else
        printf '%s' "$url"
    fi
}

SRC=""
ASSUME_YES=0
DO_DB=1
DO_UPLOADS=1
WIPE_UPLOADS=0

while [ $# -gt 0 ]; do
    case "$1" in
        -y | --yes) ASSUME_YES=1; shift ;;
        --db-only) DO_UPLOADS=0; shift ;;
        --uploads-only) DO_DB=0; shift ;;
        --wipe-uploads) WIPE_UPLOADS=1; shift ;;
        -h | --help) usage 0 ;;
        -*) die "未知参数：$1（用 --help 看用法）" ;;
        *) [ -n "$SRC" ] && die "只能指定一个备份目录"; SRC="$1"; shift ;;
    esac
done

[ -n "$SRC" ] || { usage 1; }
[ -d "$SRC" ] || die "备份目录不存在：$SRC"
SRC="$(cd "$SRC" && pwd)"

[ "$DO_DB" -eq 0 ] && [ "$DO_UPLOADS" -eq 0 ] && die "--db-only 和 --uploads-only 不能同时用"

load_env_file "$ROOT_DIR/backend/.env"

if [ "$DO_DB" -eq 1 ]; then
    [ -n "${DATABASE_URL:-}" ] || die "DATABASE_URL 未设置（backend/.env 或环境变量）"
    [ -f "$SRC/db.dump" ] || die "备份里没有 db.dump：$SRC"
    command -v pg_restore >/dev/null || die "找不到 pg_restore，请先装 PostgreSQL 客户端"
fi

if [ "$DO_UPLOADS" -eq 1 ]; then
    [ -n "${UPLOAD_PATH:-}" ] || die "UPLOAD_PATH 未设置（backend/.env 或环境变量）"
    [ -f "$SRC/uploads.tar.gz" ] || die "备份里没有 uploads.tar.gz：$SRC"
    while [ "${UPLOAD_PATH%/}" != "$UPLOAD_PATH" ]; do UPLOAD_PATH="${UPLOAD_PATH%/}"; done
    UPLOAD_PARENT="$(dirname "$UPLOAD_PATH")"
    UPLOAD_NAME="$(basename "$UPLOAD_PATH")"
fi

info "备份目录：$SRC"
[ -f "$SRC/manifest.txt" ] && sed 's/^/    /' "$SRC/manifest.txt"
[ "$DO_DB" -eq 1 ] && info "目标数据库：$(mask_url "$DATABASE_URL")"
[ "$DO_UPLOADS" -eq 1 ] && info "目标照片目录：$UPLOAD_PATH$([ "$WIPE_UPLOADS" -eq 1 ] && echo '（会先删空）' || echo '（合并覆盖）')"

if [ "$ASSUME_YES" -ne 1 ]; then
    [ -t 0 ] || die "非交互环境请加 -y 确认"
    echo
    echo "这会删除并重建目标库里的全部对象，且照片目录会被写入。确认请输入 yes："
    read -r reply
    [ "$reply" = "yes" ] || die "已取消"
fi

if [ "$DO_DB" -eq 1 ]; then
    info "恢复数据库…"
    # --single-transaction + --exit-on-error：要么整体成功，要么回滚到恢复前，
    # 不会留下删了一半的库。--no-owner/--no-acl 让备份在任何用户下都能导回去。
    pg_restore \
        --clean --if-exists \
        --no-owner --no-acl \
        --single-transaction --exit-on-error \
        --dbname="$DATABASE_URL" "$SRC/db.dump"
    info "数据库已恢复"
fi

if [ "$DO_UPLOADS" -eq 1 ]; then
    if [ "$WIPE_UPLOADS" -eq 1 ]; then
        info "清空 $UPLOAD_PATH …"
        rm -rf "$UPLOAD_PATH"
    fi
    mkdir -p "$UPLOAD_PARENT"
    info "解包照片到 $UPLOAD_PARENT …"
    # 归档里是 "uploads/..." 这样的相对路径；-C 到父目录即可。
    tar -xzf "$SRC/uploads.tar.gz" -C "$UPLOAD_PARENT"
    [ -d "$UPLOAD_PATH" ] || die "解包后没有 $UPLOAD_NAME，备份归档结构可能不对"
    info "照片已恢复（$(find "$UPLOAD_PATH" -type f | wc -l) 个文件）"
fi

cat <<EOF

完成。
EOF

if [ "$DO_UPLOADS" -eq 1 ]; then
    cat <<EOF
若解包者不是服务运行用户，照片属主会变成当前用户，请确认服务进程仍可读写：
    chown -R <服务用户> "$UPLOAD_PATH"
EOF
fi

cat <<EOF

还需要确认：
  1. 备份里的迁移版本可能比代码旧（或新）：cd backend && diesel migration list
  2. 起服务：systemctl start greenlife && curl http://127.0.0.1:8080/health
EOF
