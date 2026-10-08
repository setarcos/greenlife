import { http } from './http'
import type { Role, User } from '../types'

/// 以下三个接口都要求 ADMIN（见 backend/src/main.rs 的 /admin 作用域）。

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

/// DELETE /admin/user/{user_id}。后端拒绝管理员删除自己（400），
/// 前端已经把「自己」那一行的删除按钮藏掉了。
export async function deleteUser(userId: string): Promise<void> {
  await http.delete(`/admin/user/${userId}`)
}
