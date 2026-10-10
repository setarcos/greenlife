/// 与 backend/src/models.rs 顶部的常量保持一致，改后端记得同步这里。
///
/// 这里是**即时反馈**，不是安全边界：真正的把关在服务端
/// （models.rs 的 `validate_name` / `validate_username` / `validate_password`）。
export const NAME_MAX_CHARS = 10
export const USERNAME_MAX_CHARS = 50
export const PASSWORD_MIN_CHARS = 8
export const PASSWORD_MAX_BYTES = 72

/// 与 backend/src/taxonomy_models.rs 顶部的常量对齐。
export const TAXON_NAME_MAX_CHARS = 200 // 分类节点的 scientific_name / chinese_name
export const LIST_NAME_MAX_CHARS = 100 // 名录名
export const RECORD_NO_MAX_CHARS = 50 // 记录的编号

/// 与 backend/src/maintenance_models.rs 顶部的常量对齐。
export const ENTRY_DATE_MAX_CHARS = 50 // 维护日志的「修订笔记」
export const AUTHOR_MAX_CHARS = 200 // 维护日志的「修订人」

/// Rust 的 `chars().count()` 数的是字符，JS 的 `.length` 数的是 UTF-16 码元
/// —— 一个 emoji 在后端算 1 个字符、在前端会算成 2 个。
export function charCount(value: string): number {
  return [...value].length
}

/// bcrypt 只哈希前 72 字节，所以上限按字节算。
export function byteCount(value: string): number {
  return new TextEncoder().encode(value).length
}

/// 各返回第一条错误信息，通过时返回空串。
export function validateName(name: string): string {
  if (name.trim() === '') {
    return '名字不能为空'
  }
  if (charCount(name) > NAME_MAX_CHARS) {
    return `名字最多 ${NAME_MAX_CHARS} 个字符`
  }
  return ''
}

export function validateUsername(username: string): string {
  if (username.trim() === '') {
    return '用户名不能为空'
  }
  if (charCount(username) > USERNAME_MAX_CHARS) {
    return `用户名最多 ${USERNAME_MAX_CHARS} 个字符`
  }
  return ''
}

export function validatePassword(password: string): string {
  if (charCount(password) < PASSWORD_MIN_CHARS) {
    return `密码至少 ${PASSWORD_MIN_CHARS} 个字符`
  }
  if (byteCount(password) > PASSWORD_MAX_BYTES) {
    return `密码不能超过 ${PASSWORD_MAX_BYTES} 字节（一个中文字算 3 字节）`
  }
  return ''
}

/// 非空且不超过 maxChars —— 后端 `validate_name` 的镜像，用于分类树 / 名录的字段。
export function validateField(field: string, value: string, maxChars: number): string {
  if (value.trim() === '') {
    return `${field}不能为空`
  }
  if (charCount(value) > maxChars) {
    return `${field}最多 ${maxChars} 个字符`
  }
  return ''
}
