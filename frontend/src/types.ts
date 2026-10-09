/// 与后端 DTO 一一对应的类型（见 backend/src/models.rs）。
///
/// 关于 `role`：后端用的是 bitflags + serde，JSON 里是 `|` 分隔的 flag 字符串。
/// 两点实测结论（`bitflags-2.x/src/parser.rs::from_str`）：
///
/// - **输出**：空权限（NONE）序列化成空字符串 `""`，而不是 `"NONE"`。
///   所以下面 `ROLE_OPTIONS` 里「无权限」的值必须写 `''`，才能和接口返回的值对上。
/// - **输入**：`""` 和 `"NONE"` 都会被解析成 0，未知名字（如 `"SUPERUSER"`）才报错。
///   我们只发 `""`，与输出格式保持一致。
export type Role = '' | 'ADMIN' | 'STAFF' | 'ADMIN | STAFF'

export const ROLE_OPTIONS: ReadonlyArray<{ value: Role; label: string }> = [
  { value: '', label: '无权限' },
  { value: 'STAFF', label: '员工' },
  { value: 'ADMIN', label: '管理员' },
  { value: 'ADMIN | STAFF', label: '管理员 + 员工' },
]

export function roleLabel(role: Role): string {
  return ROLE_OPTIONS.find((option) => option.value === role)?.label ?? role
}

export function isAdmin(role: Role | undefined): boolean {
  return role !== undefined && role.includes('ADMIN')
}

/// 分类树 / 名录的写接口要求 STAFF 或 ADMIN（见 backend/src/main.rs 的 /staff 作用域）。
export function canManageTaxonomy(role: Role | undefined): boolean {
  return isAdmin(role) || (role !== undefined && role.includes('STAFF'))
}

/// `GET /staff/me`、`GET /admin/users` 等返回的用户对象。
/// 后端刻意用独立的响应 DTO，不含 `password_hash` 和 `token_version`。
export interface User {
  id: string
  name: string
  username: string
  role: Role
  /// chrono 的 NaiveDateTime，形如 "2026-10-08T20:00:00"（无时区信息）。
  created_at: string
}

export interface AuthResponse {
  token: string
  user: User
}

/// 展示用：只把 ISO 的 "T" 换成空格，**不做时区换算**。
/// 数据库里是 `TIMESTAMP`（不带时区），后端的 NaiveDateTime 也不带时区，
/// 前端硬套一个时区去解析反而会把时间显示错。
export function formatTimestamp(value: string): string {
  return value.replace('T', ' ').slice(0, 16)
}
