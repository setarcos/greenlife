import { http } from './http'
import type { AuthResponse, User } from '../types'

/// POST /auth/login（无权限要求，按 IP 限流：突发 5 次，之后每 2 秒 1 次）。
/// 用户名不存在和密码错误返回的是同一句话，前端不需要区分。
export async function login(username: string, password: string): Promise<AuthResponse> {
  const { data } = await http.post<AuthResponse>('/auth/login', { username, password })
  return data
}

/// 登录页上的自助注册请求体。
///
/// 没有 `role`：注册出来的账号恒为无权限，权限只能由管理员审批时赋予。
/// 选填字段传空串即可，后端会归一成 NULL。
export interface SignupPayload {
  username: string
  real_name: string
  /// 昵称（后端 users.name）。
  name: string
  password: string
  student_id: string
  organization: string
  contact: string
  wechat: string
}

/// POST /auth/register（无需登录，与登录共用同一套按 IP 限流）。
/// 成功后拿到的是无权限账号，需要管理员在「用户管理」里点「通过」。
export async function register(payload: SignupPayload): Promise<User> {
  const { data } = await http.post<User>('/auth/register', payload)
  return data
}

/// GET /me：读自己的账号资料。只要已登录就能访问（无权限账号也可以），
/// 顺带用来验证本地 token 是否还有效。
export async function fetchMe(): Promise<User> {
  const { data } = await http.get<User>('/me')
  return data
}

/// PATCH /me：只能改自己的注册信息和 `password`。任何已登录用户都可以调用。
///
/// 目标用户由 token 决定，路径里没有 user_id，所以改不了别人；
/// 请求体里也没有 username / role 字段。
///
/// 选填字段的语义是「字段缺失 = 不修改，空串 = 置空」，所以想清空某个字段
/// 就传空串。
///
/// ⚠️ 带 `password` 调用成功后，后端会把 `token_version` +1，
/// **当前这把 token 立即失效**，客户端必须重新登录。
export interface UpdateMePayload {
  name?: string
  real_name?: string
  password?: string
  student_id?: string
  organization?: string
  contact?: string
  wechat?: string
}

export async function updateMe(payload: UpdateMePayload): Promise<User> {
  const { data } = await http.patch<User>('/me', payload)
  return data
}
