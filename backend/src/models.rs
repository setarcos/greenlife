use crate::errors::ServiceError;
use crate::permissions::Permissions;
use crate::schema::users;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- 与数据库约束对齐的长度上限 ---
// 不对齐的后果是超长输入会撞到数据库约束，返回 500 而不是 400。

/// `users.name` 在数据库里是 VARCHAR(10)。
/// 注意 PostgreSQL 的 varchar(n) 限制的是「字符数」而不是字节数，所以用 chars().count()。
pub const NAME_MAX_CHARS: usize = 10;

/// `users.username` 在数据库里是 VARCHAR(50)。
pub const USERNAME_MAX_CHARS: usize = 50;

/// bcrypt 0.15 只哈希前 72 字节，超出部分被静默丢弃（见 bcrypt-0.15.1/src/lib.rs:116）。
/// 不拦下来的话，超过 72 字节的两个不同密码会互相等价。
pub const PASSWORD_MAX_BYTES: usize = 72;

/// 密码长度下限。这是一条新增的策略，如果不需要可以放宽。
pub const PASSWORD_MIN_CHARS: usize = 8;

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub username: String,
    // 保留 skip_serializing 作为纵深防御，但响应一律走 UserResponse。
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: Permissions,
    pub created_at: NaiveDateTime,
    /// 每次「使已签发 token 失效」的操作（目前是改密码）都会 +1。
    /// JWT 里带上签发时的值，middleware 每请求比对。
    pub token_version: i32,
}

#[derive(Insertable, Deserialize)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub name: String,
    pub username: String,
    pub password_hash: String,
    pub role: Permissions,
}

/// 显式的响应 DTO。
///
/// 原实现直接把实体 `User` 返回给客户端，只靠字段上的 `#[serde(skip_serializing)]`
/// 来挡住 password_hash。多一个显式 DTO，将来给 `User` 加字段时不会意外泄露
/// （例如 `token_version` 就刻意不出现在这里）。
#[derive(Serialize, Debug, Clone)]
pub struct UserResponse {
    pub id: Uuid,
    pub name: String,
    pub username: String,
    pub role: Permissions,
    pub created_at: NaiveDateTime,
}

impl From<&User> for UserResponse {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            name: user.name.clone(),
            username: user.username.clone(),
            role: user.role,
            created_at: user.created_at,
        }
    }
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse::from(&user)
    }
}

// --- DTOs for requests/responses ---

#[derive(Deserialize)]
pub struct RegisterUserDto {
    pub name: String,
    pub username: String,
    pub password: String,
    pub role: Permissions,
}

impl RegisterUserDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_name(&self.name)?;
        validate_username(&self.username)?;
        validate_password(&self.password)
    }
}

#[derive(Deserialize, Debug)]
pub struct UpdateUserRequestDto {
    pub name: Option<String>,
    pub username: Option<String>,
    pub role: Option<Permissions>,
    pub password: Option<String>,
}

impl UpdateUserRequestDto {
    /// 只校验「提供了的」字段。password 在这里不做非空/长度判断，
    /// 由 handler 区分「没提供」和「提供了空串」。
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }
        if let Some(username) = &self.username {
            validate_username(username)?;
        }
        Ok(())
    }
}

/// 用户自助修改自己的资料。
///
/// 刻意只有 `name` 和 `password` 两个字段：`username` 是身份标识、`role` 是权限，
/// 都不允许自助修改。它们不出现在这里，所以客户端即使传了也到不了 SQL
/// （serde 默认忽略未知字段），提权在类型层面就不可达。
#[derive(Deserialize, Debug)]
pub struct UpdateSelfRequestDto {
    pub name: Option<String>,
    pub password: Option<String>,
}

impl UpdateSelfRequestDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }
        // password 在这里可以完整校验：与 `UpdateUserRequestDto` 不同，
        // 自助修改没有「管理员代改」这层语义，空串只可能是客户端写错了。
        if let Some(password) = &self.password {
            if password.is_empty() {
                return Err(ServiceError::ValidationError(
                    "password must not be empty (omit the field to leave it unchanged)"
                        .to_string(),
                ));
            }
            validate_password(password)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct LoginUserDto {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject: user_id as string
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
    /// 签发时的 `users.token_version`。与数据库当前值不一致就说明该 token 已被作废。
    ///
    /// 这是必填字段：本次部署之前签发的 token（没有 `ver`）会直接解码失败，
    /// 即全部作废，需要重新登录。
    pub ver: i32,
}

// 注意：role 被刻意从 token 里移除了。
// 权限必须以数据库里的当前值为准（见 middleware::authenticate），
// token 里再放一份只会诱使后来者去信任它。
// 旧 token 里多余的 `rol` 字段在反序列化时会被忽略（未知字段默认被忽略）。
//
// token 里保留的是 `ver`（版本号）而不是权限：它是用来「作废」的，不是用来授权的。

fn validate_name(name: &str) -> Result<(), ServiceError> {
    if name.trim().is_empty() {
        return Err(ServiceError::ValidationError(
            "name must not be empty".to_string(),
        ));
    }
    let chars = name.chars().count();
    if chars > NAME_MAX_CHARS {
        return Err(ServiceError::ValidationError(format!(
            "name must be at most {NAME_MAX_CHARS} characters, got {chars}"
        )));
    }
    Ok(())
}

fn validate_username(username: &str) -> Result<(), ServiceError> {
    if username.trim().is_empty() {
        return Err(ServiceError::ValidationError(
            "username must not be empty".to_string(),
        ));
    }
    let chars = username.chars().count();
    if chars > USERNAME_MAX_CHARS {
        return Err(ServiceError::ValidationError(format!(
            "username must be at most {USERNAME_MAX_CHARS} characters, got {chars}"
        )));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), ServiceError> {
    if password.chars().count() < PASSWORD_MIN_CHARS {
        return Err(ServiceError::ValidationError(format!(
            "password must be at least {PASSWORD_MIN_CHARS} characters"
        )));
    }
    if password.len() > PASSWORD_MAX_BYTES {
        return Err(ServiceError::ValidationError(format!(
            "password must be at most {PASSWORD_MAX_BYTES} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(name: &str, username: &str, password: &str) -> RegisterUserDto {
        RegisterUserDto {
            name: name.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            role: Permissions::STAFF,
        }
    }

    #[test]
    fn accepts_valid_registration() {
        assert!(dto("张三", "zhangsan", "correct-horse").validate().is_ok());
    }

    #[test]
    fn rejects_name_over_database_limit() {
        // 10 个中文字符 = 30 字节，但 varchar(10) 限制的是字符数，所以要通过
        assert!(dto("一二三四五六七八九十", "u", "correct-horse").validate().is_ok());
        // 11 个字符：数据库会拒绝，必须在应用层先拦成 400
        assert!(dto("一二三四五六七八九十一", "u", "correct-horse").validate().is_err());
    }

    #[test]
    fn rejects_blank_name_and_username() {
        assert!(dto("   ", "u", "correct-horse").validate().is_err());
        assert!(dto("n", "   ", "correct-horse").validate().is_err());
    }

    #[test]
    fn rejects_username_over_database_limit() {
        let long = "u".repeat(USERNAME_MAX_CHARS + 1);
        assert!(dto("n", &long, "correct-horse").validate().is_err());
    }

    #[test]
    fn rejects_password_bcrypt_would_silently_truncate() {
        let exactly_72 = "a".repeat(PASSWORD_MAX_BYTES);
        assert!(dto("n", "u", &exactly_72).validate().is_ok());

        let over_72 = "a".repeat(PASSWORD_MAX_BYTES + 1);
        assert!(dto("n", "u", &over_72).validate().is_err());
    }

    #[test]
    fn rejects_short_password() {
        assert!(dto("n", "u", "short").validate().is_err());
    }

    #[test]
    fn self_update_dto_only_validates_provided_fields() {
        let empty = UpdateSelfRequestDto {
            name: None,
            password: None,
        };
        assert!(empty.validate().is_ok());

        let ok = UpdateSelfRequestDto {
            name: Some("张三".to_string()),
            password: Some("correct-horse".to_string()),
        };
        assert!(ok.validate().is_ok());

        let too_long = UpdateSelfRequestDto {
            name: Some("x".repeat(NAME_MAX_CHARS + 1)),
            password: None,
        };
        assert!(too_long.validate().is_err());

        // 空串和过短的密码都必须被拒：不能让自助端点成为绕过密码策略的入口。
        let empty_password = UpdateSelfRequestDto {
            name: None,
            password: Some(String::new()),
        };
        assert!(empty_password.validate().is_err());

        let short_password = UpdateSelfRequestDto {
            name: None,
            password: Some("short".to_string()),
        };
        assert!(short_password.validate().is_err());
    }

    #[test]
    fn update_dto_only_validates_provided_fields() {
        let empty = UpdateUserRequestDto {
            name: None,
            username: None,
            role: None,
            password: None,
        };
        assert!(empty.validate().is_ok());

        let too_long = UpdateUserRequestDto {
            name: Some("x".repeat(NAME_MAX_CHARS + 1)),
            username: None,
            role: None,
            password: None,
        };
        assert!(too_long.validate().is_err());
    }
}
