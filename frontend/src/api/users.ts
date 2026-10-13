import { http } from './http'
import type { Role, User } from '../types'

/// 以下几个接口都要求 ADMIN（见 backend/src/main.rs 的 /admin 作用域）。

/// GET /admin/users。后端目前不做分页，返回全表。
export async function listUsers(): Promise<User[]> {
  const { data } = await http.get<User[]>('/admin/users')
  return data
}

/// POST /admin/user/add。
export async function createUser(payload: {
  name: string
  username: string
  password: string
  role: Role
}): Promise<User> {
  const { data } = await http.post<User>('/admin/user/add', payload)
  return data
}

/// POST /admin/user/{user_id}/approve：给自助注册（无权限）的账号授予 STAFF 权限。
/// 后端用的是按位或，所以已经拥有 ADMIN 的账号不会被降级。
export async function approveUser(userId: string): Promise<User> {
  const { data } = await http.post<User>(`/admin/user/${userId}/approve`)
  return data
}

/// PATCH /admin/user/{user_id}：管理员改别人的权限 / 密码。
///
/// 只发要改的字段：`password` 省略 = 不改密码（发空串会被后端当「提供了空密码」拒掉）；
/// 后端还会拒绝管理员改自己的权限（400），所以页面上自己那一行不给编辑入口。
export interface UpdateUserPayload {
  role?: Role
  password?: string
}

export async function updateUser(userId: string, payload: UpdateUserPayload): Promise<User> {
  const { data } = await http.patch<User>(`/admin/user/${userId}`, payload)
  return data
}

/// DELETE /admin/user/{user_id}。后端拒绝管理员删除自己（400），
/// 前端已经把「自己」那一行的删除按钮藏掉了。
export async function deleteUser(userId: string): Promise<void> {
  await http.delete(`/admin/user/${userId}`)
}
