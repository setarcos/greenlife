import { http } from './http'
import type { AuthResponse, User } from '../types'

/// POST /auth/login（无权限要求，按 IP 限流：突发 5 次，之后每 2 秒 1 次）。
/// 用户名不存在和密码错误返回的是同一句话，前端不需要区分。
export async function login(username: string, password: string): Promise<AuthResponse> {
  const { data } = await http.post<AuthResponse>('/auth/login', { username, password })
  return data
}

/// GET /staff/me（STAFF 或 ADMIN）。顺带用来验证本地 token 是否还有效。
export async function fetchMe(): Promise<User> {
  const { data } = await http.get<User>('/staff/me')
  return data
}

/// PATCH /staff/me（STAFF 或 ADMIN）：只能改自己的 `name` 和 `password`。
///
/// 目标用户由 token 决定，路径里没有 user_id，所以改不了别人；
/// 请求体里也没有 username / role 字段。
///
/// ⚠️ 带 `password` 调用成功后，后端会把 `token_version` +1，
/// **当前这把 token 立即失效**，客户端必须重新登录。
export async function updateMe(payload: { name?: string; password?: string }): Promise<User> {
  const { data } = await http.patch<User>('/staff/me', payload)
  return data
}
