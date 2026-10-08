# Greenlife 网站

## 仓库结构

```
.
├── backend/    服务端：actix-web + diesel + PostgreSQL，提供 JWT 鉴权与权限管理
└── frontend/   前端（尚未开始）
```

## 部署

### 1. 环境要求

- Rust（edition 2021）—— 当前工具链可用 `rustc 1.98`
- PostgreSQL
- `diesel` CLI（仅用于执行迁移）

  ```bash
  cargo install diesel_cli --no-default-features --features postgres
  ```

  编译需要系统的 libpq 开发头文件（Debian/Ubuntu：`libpq-dev`，Void：`postgresql-libs-devel`）。
  装好后确认 `diesel` 在 `PATH` 里（默认落在 `~/.cargo/bin`）。

### 2. 配置环境变量

```bash
cd backend
cp .env.example .env
```

编辑 `.env`，两个必填项：

| 变量 | 说明 |
|---|---|
| `DATABASE_URL` | 数据库连接串。**线上用 TCP + 密码**：`postgresql://user:password@host:5432/greenlife` |
| `JWT_SECRET` | 签名密钥，建议至少 32 字节。生成：`head -c 48 /dev/urandom \| base64` |

可选：`JWT_EXPIRATION_HOURS`（默认 24）、`SERVER_ADDRESS`（默认 `127.0.0.1:8080`）。

`DATABASE_URL` 或 `JWT_SECRET` 缺失时**进程会直接启动失败**，这是有意的 fail-fast。
`.env` 不入库，密钥只放在部署环境的 `.env` 或环境变量里。

> 本机开发可以用 unix socket 免密写法 `postgresql:///greenlife?host=/tmp`，
> 但它依赖 peer 认证、只在本机同用户下成立，**不要用于线上**。

### 3. 建库并执行迁移

```bash
createdb greenlife        # 若库还不存在
diesel migration run      # 应用 migrations/ 下的全部迁移
diesel migration list     # 确认已应用
```

DDL 由 `backend/migrations/` 版本化管理，`src/schema.rs` 由 CLI 生成，不要手改。

> ⚠️ `diesel migration revert 2026-10-08-190159_init` 会 `DROP TABLE users`，直接删表。
> 生产环境不要执行 revert。

### 4. 创建第一个管理员

`POST /admin/user/add` 需要 ADMIN 权限，所以**空库无法通过 API 引导出第一个管理员**，需手工插入一条。

```bash
pip install bcrypt
HASH=$(python3 -c "import bcrypt;print(bcrypt.hashpw(b'your-password', bcrypt.gensalt(12)).decode())")

psql "$DATABASE_URL" -c "INSERT INTO users (id, name, username, password_hash, role)
  VALUES (gen_random_uuid(), '管理员', 'admin', '$HASH', 1)"
```

（`gen_random_uuid()` 是 PostgreSQL 13+ 的内置函数，无需装 pgcrypto。）

`role` 是位标志：`0` = NONE，`1` = ADMIN，`2` = STAFF，`3` = ADMIN | STAFF。

### 5. 构建与启动

```bash
cargo build --release
./target/release/Greenlife
```

启动日志会打印监听地址。探活：

```bash
curl http://127.0.0.1:8080/health
```

首次登录：

```bash
curl -X POST http://127.0.0.1:8080/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"your-password"}'
```

### 6. 上线前检查

- **反向代理**：登录接口按 peer IP 限流（突发 5 次，之后每 2 秒 1 次）。
  代理会在服务端看到的 IP 变成代理地址，**限流将退化为全局限流**，
  必须让服务端能拿到真实客户端 IP（处理 `X-Forwarded-For`）。
- **监听地址**：`SERVER_ADDRESS` 默认只绑 `127.0.0.1`，由代理转发是本项目预期的部署形态。
- **HTTPS**：服务本身只提供明文 HTTP，证书与 TLS 终止交给反向代理。
- 数据库连接池取不到连接时返回 503，可放心让网关做重试。

## 前端

前端尚未开始，计划放在 `frontend/`。

建议的部署形态是**同源**：由反向代理提供前端静态资源，并把 `/auth`、`/admin`、`/staff`
转发到本服务。这样不涉及跨域。

若前端最终独立部署在其它域名下，需要先给服务端加 CORS 配置（**目前未配置**）。
