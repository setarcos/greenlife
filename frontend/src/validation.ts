/// 与 backend/src/models.rs 顶部的常量保持一致，改后端记得同步这里。
///
/// 这里是**即时反馈**，不是安全边界：真正的把关在服务端
/// （models.rs 的 `validate_name` / `validate_username` / `validate_password`）。
export const NAME_MAX_CHARS = 10
export const USERNAME_MAX_CHARS = 50
export const PASSWORD_MIN_CHARS = 8
export const PASSWORD_MAX_BYTES = 72

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
