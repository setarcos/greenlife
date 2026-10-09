use actix_web::{error::ResponseError, http::StatusCode, HttpResponse};
use derive_more::Display;

#[derive(Debug, Display)]
pub enum ServiceError {
    #[display("Internal Server Error")]
    InternalServerError,
    #[display("BadRequest: {}", _0)]
    BadRequest(String),
    #[display("Unauthorized: {}", _0)]
    Unauthorized(String),
    #[display("Forbidden: {}", _0)]
    Forbidden(String),
    #[display("NotFound: {}", _0)]
    NotFound(String),
    #[display("Conflict: {}", _0)]
    Conflict(String),
    #[display("ValidationError: {}", _0)]
    ValidationError(String),

    /// 依赖暂时不可用（例如数据库连接池取不到连接）。
    /// 用 503 而不是 500，让调用方/网关知道这是可重试的瞬时状态。
    #[display("ServiceUnavailable: {}", _0)]
    ServiceUnavailable(String),

    /// 数据库错误。它的 `Display` 会带上原始 SQL 错误、表名和约束名，
    /// 所以只能进日志，绝不能进响应体。
    #[display("DieselError: {}", _0)]
    DieselError(diesel::result::Error),

    /// 同上：`jsonwebtoken` 的错误详情只进日志。
    #[display("JwtError: {}", _0)]
    JwtError(jsonwebtoken::errors::Error),
}

impl ServiceError {
    /// 可以安全返回给客户端的消息。
    ///
    /// 内部错误（数据库/JWT）的 `Display` 会泄露实现细节，这里替换为泛化文案；
    /// 其余变体保持原有字符串不变，避免无谓地改动前端已依赖的文案。
    fn client_message(&self) -> String {
        match self {
            ServiceError::DieselError(_) => "Internal Server Error".to_string(),
            ServiceError::JwtError(_) => "Invalid token".to_string(),
            other => other.to_string(),
        }
    }
}

impl ResponseError for ServiceError {
    fn status_code(&self) -> StatusCode {
        match self {
            ServiceError::InternalServerError | ServiceError::DieselError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            ServiceError::BadRequest(_) | ServiceError::ValidationError(_) => StatusCode::BAD_REQUEST,
            ServiceError::Unauthorized(_) | ServiceError::JwtError(_) => StatusCode::UNAUTHORIZED,
            ServiceError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ServiceError::Forbidden(_) => StatusCode::FORBIDDEN,
            ServiceError::NotFound(_) => StatusCode::NOT_FOUND,
            ServiceError::Conflict(_) => StatusCode::CONFLICT,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        if status.is_server_error() {
            // 客户端只看到泛化文案，细节留在服务端日志里。
            // 503 的两条日志由产生错误的地方（db.rs）负责，这里不重复刷屏。
            if !matches!(self, ServiceError::ServiceUnavailable(_)) {
                log::error!("internal error: {}", self);
            }
        }
        HttpResponse::build(status).json(serde_json::json!({ "error": self.client_message() }))
    }
}

impl From<diesel::result::Error> for ServiceError {
    fn from(err: diesel::result::Error) -> ServiceError {
        use diesel::result::{DatabaseErrorKind, Error as DieselError};

        match err {
            // 唯一约束冲突应该是 409 而不是 500。例如 register 的「先查后插」在并发下
            // 会撞 users_username_key，这个分支把它变成有意义的 409。
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
                log::warn!("unique constraint violated: {}", info.message());
                ServiceError::Conflict("Resource already exists".to_string())
            }
            // 外键冲突同理。分类树的删除接口会先显式检查子节点 / 记录并返回更
            // 具体的 409，这里只是并发窗口下的兜底。
            DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info) => {
                log::warn!("foreign key constraint violated: {}", info.message());
                ServiceError::Conflict(
                    "Resource is still referenced by other records".to_string(),
                )
            }
            DieselError::NotFound => ServiceError::NotFound("Resource not found".to_string()),
            other => ServiceError::DieselError(other),
        }
    }
}

impl From<jsonwebtoken::errors::Error> for ServiceError {
    fn from(err: jsonwebtoken::errors::Error) -> ServiceError {
        // 细节只进日志；返回给客户端的是泛化文案（见 client_message）。
        log::debug!("underlying JWT error: {:?}", err);

        match err.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                ServiceError::Unauthorized("Token has expired".to_string())
            }
            jsonwebtoken::errors::ErrorKind::InvalidToken
            | jsonwebtoken::errors::ErrorKind::InvalidSignature
            | jsonwebtoken::errors::ErrorKind::InvalidAlgorithm
            | jsonwebtoken::errors::ErrorKind::MissingRequiredClaim(_)
            | jsonwebtoken::errors::ErrorKind::InvalidIssuer
            | jsonwebtoken::errors::ErrorKind::InvalidAudience
            | jsonwebtoken::errors::ErrorKind::InvalidSubject
            | jsonwebtoken::errors::ErrorKind::ImmatureSignature
            | jsonwebtoken::errors::ErrorKind::InvalidEcdsaKey
            | jsonwebtoken::errors::ErrorKind::InvalidRsaKey(_)
            | jsonwebtoken::errors::ErrorKind::InvalidKeyFormat
            | jsonwebtoken::errors::ErrorKind::Base64(_)
            | jsonwebtoken::errors::ErrorKind::Json(_)
            | jsonwebtoken::errors::ErrorKind::Utf8(_)
            | jsonwebtoken::errors::ErrorKind::Crypto(_) => {
                ServiceError::Unauthorized("Invalid token".to_string())
            }
            _ => ServiceError::JwtError(err),
        }
    }
}
