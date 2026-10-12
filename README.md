# Greenlife 网站

## 仓库结构

```
.
├── backend/    服务端：actix-web + diesel + PostgreSQL，提供 JWT 鉴权与权限管理
└── frontend/   前端：Vue 3 + TypeScript + Vite（SPA，所有后端请求带 `/api` 前缀）
```

## 部署

### 1. 环境要求

- Rust（edition 2021）
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
| `UPLOAD_PATH` | 物种照片的存储根目录，例：`/srv/greenlife/uploads`。运行服务的用户必须对它可写；照片按拍摄年月写到 `$UPLOAD_PATH/yyyy-mm/<uuid>.<ext>` |

可选：`JWT_EXPIRATION_HOURS`（默认 24）、`SERVER_ADDRESS`（默认 `127.0.0.1:8080`）。

`DATABASE_URL`、`JWT_SECRET` 或 `UPLOAD_PATH` 缺失时**进程会直接启动失败**，这是有意的 fail-fast。

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

- **反向代理**：登录接口按客户端 IP 限流（突发 5 次，之后每 2 秒 1 次）。
  客户端 IP 的取法是：直连对端是回环地址（`127.0.0.0/8`、`::1`）时取
  `X-Forwarded-For` 的最后一段，否则就用对端地址（见 `backend/src/rate_limit.rs`）。
  所以 nginx 必须和后端同机（默认部署形态就是如此），中间不能再夹别的代理：
  若前面还有 CDN / tunnel，需要先让 nginx 拿到真实 IP，见文末「Cloudflare Tunnel」。
- **监听地址**：`SERVER_ADDRESS` 默认只绑 `127.0.0.1`，由代理转发是本项目预期的部署形态。
- **HTTPS**：服务本身只提供明文 HTTP，证书与 TLS 终止交给反向代理。
- 数据库连接池取不到连接时返回 503，可放心让网关做重试。

## 前端

`frontend/` 是 Vue 3 + TypeScript + Vite 的单页应用，依赖 Pinia（登录态）和 vue-router（路由守卫）。

### 开发

```bash
cd frontend
npm install
npm run dev            # http://localhost:5173
```

开发服务器会把 `/api` 开头的请求转发给后端（见 `vite.config.ts`，默认目标
`http://127.0.0.1:8080`），并**剥掉 `/api` 前缀** —— 与下面的 nginx 配置一致。
所以开发时不用给后端加 CORS。

```bash
npm run build          # vue-tsc 类型检查 + 打包到 dist/
npm run lint           # eslint 检查（有错时退出码非 0）
npm run format         # prettier 格式化（.prettierrc.json：无分号、单引号、100 列）
npm run preview        # 本地预览 dist/
```

### 部署：所有后端请求都带 `/api` 前缀

前端调用的路径都是 `/api/...`（axios 的 `baseURL` 统一加，见 `src/api/http.ts`）。
于是 nginx 只需要一条规则，`proxy_pass` 结尾的斜杠会**剥掉 `/api/`**，
后端本身完全不需要知道这个前缀存在（`backend/` 的代码无需改动）：

```nginx
root /srv/greenlife/frontend/dist;

location / {
    try_files $uri $uri/ /index.html;   # SPA：刷新 /admin/users 也能落到 index.html
}

location /api/ {
    proxy_pass http://127.0.0.1:8080/;

    # 必须显式设置。nginx 默认只改 Host 和 Connection，客户端发来的
    # X-Forwarded-For 是原样透传的；后端按这个头的末段识别客户端，透传的话
    # 限流要么退化成全局限流（浏览器不发这个头），要么被访问者用伪造的 IP 绕过。
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}

# 物种照片：nginx 直接读磁盘，不经过后端。
# alias 必须指向后端的 UPLOAD_PATH，并以斜杠结尾。
location /uploads/ {
    alias /srv/greenlife/uploads/;
    expires 7d;
    add_header Cache-Control "public";
}
```

文件名是 UUID，内容不会变，所以缓存可以放长。上传接口仍走 `/api/staff/photos`
（POST multipart），只有图片文件的读取由 nginx 接手。

推荐**同源**部署（前端静态资源和 API 同一个域名），这样不涉及跨域。
若前端最终独立部署在其它域名下，需要先给服务端加 CORS 配置（**目前未配置**）。

上线前请一并阅读上一节的反向代理注意事项：登录限流只在直连对端是回环地址时
采信 `X-Forwarded-For` 的末段，因此反向代理要和后端同机，
而且不能让别的代理挡在 nginx 前面，否则限流会退化成全局限流。

### Cloudflare Tunnel

用 `cloudflared` 把域名打到本机 nginx 时，nginx 看到的对端是 `127.0.0.1`
（tunnel 在本机发起连接），它附到 `X-Forwarded-For` 末尾的自然也是回环地址，
登录限流会退化成全局限流。让 nginx 信任 Cloudflare 传来的 `CF-Connecting-IP` 即可
（写在 `http` 或 `server` 块里）：

```nginx
# 只信任本机（cloudflared）给出的 CF-Connecting-IP。公网直连时这个头是
# 客户端可控的，不加 set_real_ip_from 就等于让访问者自报家门。
set_real_ip_from 127.0.0.1;
set_real_ip_from ::1;
real_ip_header CF-Connecting-IP;
```

加上这三行后，`$remote_addr`、访问日志、以及 nginx 转给后端的 `X-Forwarded-For`
就都是真实客户端 IP，限流按人计算。两个细节：

- cloudflared 跑在容器里（而不是直接跑在宿主机上）时，nginx 看到的源地址是容器
  网关（Docker 默认 `172.17.0.1`），要把它一并写进 `set_real_ip_from`。
- nginx 自己跑在容器里时，后端看到的就是容器网关而非回环地址，限流仍会退化成
  全局限流（信任范围写死在 `backend/src/rate_limit.rs`）。
