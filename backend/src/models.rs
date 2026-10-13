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

/// 注册信息里的「真实姓名」，数据库里是 VARCHAR(50)。
pub const REAL_NAME_MAX_CHARS: usize = 50;

/// 「学号 / 工号」，数据库里是 VARCHAR(50)。
pub const STUDENT_ID_MAX_CHARS: usize = 50;

/// 「单位」，数据库里是 VARCHAR(200)。
pub const ORGANIZATION_MAX_CHARS: usize = 200;

/// 「联系方式」，数据库里是 VARCHAR(200)。
pub const CONTACT_MAX_CHARS: usize = 200;

/// 「微信号」，数据库里是 VARCHAR(100)。
pub const WECHAT_MAX_CHARS: usize = 100;

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
    // --- 注册信息（昵称就是上面的 name）---
    // 都可空：迁移之前的老账号没有这些资料，管理员手动建号也不强制填。
    pub real_name: Option<String>,
    pub student_id: Option<String>,
    pub organization: Option<String>,
    pub contact: Option<String>,
    pub wechat: Option<String>,
}

#[derive(Insertable, Deserialize)]
#[diesel(table_name = users)]
pub struct NewUser {
    pub id: Uuid,
    pub name: String,
    pub username: String,
    pub password_hash: String,
    pub role: Permissions,
    pub real_name: Option<String>,
    pub student_id: Option<String>,
    pub organization: Option<String>,
    pub contact: Option<String>,
    pub wechat: Option<String>,
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
    pub real_name: Option<String>,
    pub student_id: Option<String>,
    pub organization: Option<String>,
    pub contact: Option<String>,
    pub wechat: Option<String>,
}

impl From<&User> for UserResponse {
    fn from(user: &User) -> Self {
        Self {
            id: user.id,
            name: user.name.clone(),
            username: user.username.clone(),
            role: user.role,
            created_at: user.created_at,
            real_name: user.real_name.clone(),
            student_id: user.student_id.clone(),
            organization: user.organization.clone(),
            contact: user.contact.clone(),
            wechat: user.wechat.clone(),
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

/// 登录页上的自助注册。
///
/// 必填：用户名、真实姓名、昵称（`name`）、密码。选填：学号/工号、单位、联系方式、微信号。
/// `role` 刻意不在请求体里：自助注册得到的账号恒为无权限，权限只能由管理员审批时赋予。
#[derive(Deserialize)]
pub struct SignupRequestDto {
    pub username: String,
    pub real_name: String,
    pub name: String,
    pub password: String,
    pub student_id: Option<String>,
    pub organization: Option<String>,
    pub contact: Option<String>,
    pub wechat: Option<String>,
}

impl SignupRequestDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_username(&self.username)?;
        validate_real_name(&self.real_name)?;
        validate_name(&self.name)?;
        validate_password(&self.password)?;
        validate_optional_length("student_id", &self.student_id, STUDENT_ID_MAX_CHARS)?;
        validate_optional_length("organization", &self.organization, ORGANIZATION_MAX_CHARS)?;
        validate_optional_length("contact", &self.contact, CONTACT_MAX_CHARS)?;
        validate_optional_length("wechat", &self.wechat, WECHAT_MAX_CHARS)
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
/// `username` 是身份标识、`role` 是权限，都不允许自助修改。它们不出现在这里，
/// 所以客户端即使传了也到不了 SQL（serde 默认忽略未知字段），提权在类型层面就不可达。
///
/// 除 `name` / `real_name` / `password` 外的选填字段用「空串 = 置空」的语义：
/// 请求体里 `Option<String>` 区分不了「字段缺失」和 `null`，所以统一约定，
/// 想知道「怎么置空」就看 `normalize_optional_text`。
#[derive(Deserialize, Debug)]
pub struct UpdateSelfRequestDto {
    pub name: Option<String>,
    pub real_name: Option<String>,
    pub password: Option<String>,
    pub student_id: Option<String>,
    pub organization: Option<String>,
    pub contact: Option<String>,
    pub wechat: Option<String>,
}

impl UpdateSelfRequestDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }
        // 真实姓名一旦提供了就不能是空串：它是必填的注册信息，
        // 「清空真实姓名」不在自助修改的语义里。
        if let Some(real_name) = &self.real_name {
            validate_real_name(real_name)?;
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
        validate_optional_length("student_id", &self.student_id, STUDENT_ID_MAX_CHARS)?;
        validate_optional_length("organization", &self.organization, ORGANIZATION_MAX_CHARS)?;
        validate_optional_length("contact", &self.contact, CONTACT_MAX_CHARS)?;
        validate_optional_length("wechat", &self.wechat, WECHAT_MAX_CHARS)
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

fn validate_real_name(real_name: &str) -> Result<(), ServiceError> {
    if real_name.trim().is_empty() {
        return Err(ServiceError::ValidationError(
            "real_name must not be empty".to_string(),
        ));
    }
    let chars = real_name.chars().count();
    if chars > REAL_NAME_MAX_CHARS {
        return Err(ServiceError::ValidationError(format!(
            "real_name must be at most {REAL_NAME_MAX_CHARS} characters, got {chars}"
        )));
    }
    Ok(())
}

/// 选填字段的长度校验：`None` 和空串都合法，只在超长时报错。
fn validate_optional_length(
    field: &str,
    value: &Option<String>,
    max_chars: usize,
) -> Result<(), ServiceError> {
    if let Some(value) = value {
        let chars = value.chars().count();
        if chars > max_chars {
            return Err(ServiceError::ValidationError(format!(
                "{field} must be at most {max_chars} characters, got {chars}"
            )));
        }
    }
    Ok(())
}

/// 选填文本归一：去掉首尾空白，空串当成「没填」存 NULL。
///
/// 注册和自助修改共用这一份实现，避免两处对「空」的判断不一致。
pub fn normalize_optional_text(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

/// 把请求体里的选填文本转成 changeset 值：外层 `None` = 不修改，
/// 内层 `None` = 置空（写 NULL），`Some` = 改成这个值。
pub fn optional_text_changeset(value: &Option<String>) -> Option<Option<String>> {
    // `Some("")` 也要变成「置空」，所以先记下「提供了字段」再归一。
    value.as_ref().map(|_| normalize_optional_text(value))
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

    fn self_update(name: Option<&str>, password: Option<&str>) -> UpdateSelfRequestDto {
        UpdateSelfRequestDto {
            name: name.map(str::to_string),
            real_name: None,
            password: password.map(str::to_string),
            student_id: None,
            organization: None,
            contact: None,
            wechat: None,
        }
    }

    fn signup() -> SignupRequestDto {
        SignupRequestDto {
            username: "zhangsan".to_string(),
            real_name: "张三".to_string(),
            name: "小三".to_string(),
            password: "correct-horse".to_string(),
            student_id: None,
            organization: None,
            contact: None,
            wechat: None,
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
        let empty = self_update(None, None);
        assert!(empty.validate().is_ok());

        let ok = self_update(Some("张三"), Some("correct-horse"));
        assert!(ok.validate().is_ok());

        // 空串和过短的密码都必须被拒：不能让自助端点成为绕过密码策略的入口。
        assert!(self_update(None, Some("")).validate().is_err());
        assert!(self_update(None, Some("short")).validate().is_err());
    }

    #[test]
    fn self_update_still_checks_name_and_real_name_length() {
        let too_long_name = self_update(Some(&"x".repeat(NAME_MAX_CHARS + 1)), None);
        assert!(too_long_name.validate().is_err());

        let mut too_long_real_name = self_update(None, None);
        too_long_real_name.real_name = Some("x".repeat(REAL_NAME_MAX_CHARS + 1));
        assert!(too_long_real_name.validate().is_err());

        // 真实姓名是必填注册信息，自助修改时提供了就不能是空串。
        let mut blank_real_name = self_update(None, None);
        blank_real_name.real_name = Some("   ".to_string());
        assert!(blank_real_name.validate().is_err());
    }

    #[test]
    fn signup_requires_real_name_and_respects_column_limits() {
        assert!(signup().validate().is_ok());

        let mut blank_real_name = signup();
        blank_real_name.real_name = "  ".to_string();
        assert!(blank_real_name.validate().is_err());

        let mut long_real_name = signup();
        long_real_name.real_name = "x".repeat(REAL_NAME_MAX_CHARS + 1);
        assert!(long_real_name.validate().is_err());

        let mut long_student_id = signup();
        long_student_id.student_id = Some("x".repeat(STUDENT_ID_MAX_CHARS + 1));
        assert!(long_student_id.validate().is_err());

        let mut long_wechat = signup();
        long_wechat.wechat = Some("x".repeat(WECHAT_MAX_CHARS + 1));
        assert!(long_wechat.validate().is_err());

        // 选填字段填空串是合法的（归一后存 NULL）。
        let mut empty_optionals = signup();
        empty_optionals.student_id = Some(String::new());
        empty_optionals.organization = Some(String::new());
        assert!(empty_optionals.validate().is_ok());
    }

    #[test]
    fn optional_text_normalises_blank_to_none() {
        assert_eq!(normalize_optional_text(&None), None);
        assert_eq!(normalize_optional_text(&Some(String::new())), None);
        assert_eq!(normalize_optional_text(&Some("   ".to_string())), None);
        assert_eq!(
            normalize_optional_text(&Some(" 张三 ".to_string())),
            Some("张三".to_string())
        );

        // 外层 None = 不改，空串 = 置空（Some(None)）。
        assert_eq!(optional_text_changeset(&None), None);
        assert_eq!(optional_text_changeset(&Some(String::new())), Some(None));
        assert_eq!(
            optional_text_changeset(&Some("x".to_string())),
            Some(Some("x".to_string()))
        );
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
