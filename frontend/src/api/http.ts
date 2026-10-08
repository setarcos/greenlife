import axios from 'axios'

const TOKEN_KEY = 'greenlife.token'

/// token 存 localStorage，刷新页面后仍然保持登录。
///
/// 用模块级的存取函数而不是 Pinia store，是为了让 http 层不依赖 store：
/// 否则会形成 store → api → http → store 的循环导入。
let token: string | null = localStorage.getItem(TOKEN_KEY)

export function getToken(): string | null {
  return token
}

export function setToken(next: string | null): void {
  token = next
  if (next === null) {
    localStorage.removeItem(TOKEN_KEY)
  } else {
    localStorage.setItem(TOKEN_KEY, next)
  }
}

/// 所有后端接口都挂在 `/api` 下。
///
/// 线上交给 nginx 反代，注意 `proxy_pass` 结尾那个斜杠——它会把 `/api/` 前缀剥掉：
///
/// ```nginx
/// location /api/ {
///     proxy_pass http://127.0.0.1:8080/;
/// }
/// ```
///
/// 于是 `GET /api/staff/me` 到后端就成了 `GET /staff/me`（后端不认识 /api 前缀，
/// 所以后端代码不用改）。开发环境由 vite 的 proxy 做同样的事（见 vite.config.ts）。
///
/// 用 baseURL 而不是在每个调用点上写 `/api/...`：前缀只有这一个来源，
/// api/ 下的函数里写的仍然是后端真实路径（`/auth/login`、`/admin/users`），
/// 与 backend/src/main.rs 里的 scope 一一对应。
export const http = axios.create({ baseURL: '/api' })

http.interceptors.request.use((config) => {
  if (token !== null) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

/// 401 的清理动作由 main.ts 注册。这样 http 层不需要知道 store 和 router 的存在。
let onUnauthorized: (() => void) | null = null

export function setUnauthorizedHandler(handler: () => void): void {
  onUnauthorized = handler
}

http.interceptors.response.use(
  (response) => response,
  (error: unknown) => {
    // 后端在这些情况下返回 401：token 过期/签名不对、改密码后版本号对不上、
    // 账号已被删除。共同的处理方式是丢掉本地登录态。
    if (axios.isAxiosError(error) && error.response?.status === 401) {
      onUnauthorized?.()
    }
    return Promise.reject(error)
  },
)

/// 后端所有错误响应的形状都是 `{ "error": "..." }`（见 backend/src/errors.rs）。
export function errorMessage(error: unknown): string {
  if (!axios.isAxiosError(error)) {
    return '发生未知错误'
  }
  const data = error.response?.data as { error?: string } | undefined
  if (typeof data?.error === 'string' && data.error !== '') {
    return data.error
  }
  if (error.response) {
    return `请求失败（HTTP ${error.response.status}）`
  }
  return '无法连接到服务器'
}

/// 403 = 已登录但权限不足（见 middleware.rs::PermissionGuard）。
export function isForbidden(error: unknown): boolean {
  return axios.isAxiosError(error) && error.response?.status === 403
}
