use crate::config::AppConfig;
use crate::db::{blocking_db, DbPool};
use crate::errors::ServiceError;
use crate::models::{TokenClaims, User};
use crate::permissions::Permissions;
use crate::schema::users;
use actix_web::body::MessageBody;
use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{web, Error as ActixWebError, HttpMessage};
use chrono::Utc;
use diesel::prelude::*;
use futures_util::future::{ok, ready, FutureExt, LocalBoxFuture};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use std::rc::Rc;
use uuid::Uuid;

/// 插入到请求扩展里的已认证用户。
///
/// 存的是 middleware **刚从数据库读出来的整行 `User`**，而不是 token 里的声明。
/// 因此 `role()` 一定是当前值，而不是签发 token 时的快照。
/// 顺带的好处：`get_me` 不必再查一次数据库。
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user: User,
}

impl AuthenticatedUser {
    pub fn id(&self) -> Uuid {
        self.user.id
    }

    pub fn role(&self) -> Permissions {
        self.user.role
    }
}

pub fn generate_jwt(
    user_id: Uuid,
    token_version: i32,
    config: &AppConfig,
) -> Result<String, ServiceError> {
    let now = Utc::now();
    let expiration =
        (now + chrono::Duration::hours(config.jwt_expiration_hours)).timestamp() as usize;

    let claims = TokenClaims {
        sub: user_id.to_string(),
        exp: expiration,
        iat: now.timestamp() as usize,
        ver: token_version,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(ServiceError::from)
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<TokenClaims, ServiceError> {
    let mut validation = Validation::default();
    validation.validate_exp = true;

    decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(ServiceError::from)
}

/// 从 `Authorization` 头的值里取出 token。
///
/// scheme 是大小写敏感的（与原实现一致），只接受 `Bearer `。
fn bearer_token(header: Option<&str>) -> Option<&str> {
    header?.strip_prefix("Bearer ")
}

/// 校验 token 并回查数据库，返回**当前**的用户记录。
async fn authenticate(req: &ServiceRequest) -> Result<User, ServiceError> {
    let token = bearer_token(req.headers().get("Authorization").and_then(|v| v.to_str().ok()))
        .ok_or_else(|| {
            log::warn!("missing or malformed Authorization header");
            ServiceError::Unauthorized("Authorization header missing or malformed".to_string())
        })?;

    let config = req.app_data::<web::Data<AppConfig>>().ok_or_else(|| {
        log::error!("AppConfig missing from app_data; did you forget .app_data(...)?");
        ServiceError::InternalServerError
    })?;

    let claims = decode_jwt(token, &config.jwt_secret)?;

    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| {
        log::warn!("JWT subject is not a valid UUID: {}", claims.sub);
        ServiceError::Unauthorized("Invalid token claims".to_string())
    })?;

    let pool = req
        .app_data::<web::Data<DbPool>>()
        .cloned()
        .ok_or_else(|| {
            log::error!("DbPool missing from app_data; did you forget .app_data(...)?");
            ServiceError::InternalServerError
        })?;

    // 每个已认证请求回查一次 users，取「当前」role 并确认账号仍然存在。
    // 这样降级、权限回收、删除账号都能立即生效，而不是等 token 过期（最长 24h）。
    //
    // 代价：每个已认证请求多一次主键索引查询（单行命中，本地约 0.1ms）。
    // 这和「加 token_version 列」方案的查询次数完全一样，只是省掉一次迁移。
    let user = blocking_db(pool, move |conn| {
        users::table
            .find(user_id)
            .select(User::as_select())
            .first::<User>(conn)
            .optional()
            .map_err(ServiceError::from)
    })
    .await?
    .ok_or_else(|| {
        log::warn!("token subject {user_id} no longer exists");
        ServiceError::Unauthorized("Account no longer exists".to_string())
    })?;

    // 版本号比对：改密码会把 users.token_version +1，于是所有旧 token 立即失效。
    //
    // 这里不需要额外的数据库查询——上面查 role 时已经把整行取回来了，
    // 所以 token_version 校验是「免费」的。
    if claims.ver != user.token_version {
        log::warn!(
            "stale token for user {}: token ver={}, current ver={}",
            user_id,
            claims.ver,
            user.token_version
        );
        return Err(ServiceError::Unauthorized(
            "Token has been revoked".to_string(),
        ));
    }

    Ok(user)
}

// --- Middleware ---

pub struct JwtAuth;

impl<S, B> Transform<S, ServiceRequest> for JwtAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixWebError;
    type InitError = ();
    type Transform = JwtAuthMiddleware<S>;
    type Future = LocalBoxFuture<'static, Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        // 用 Rc 包住下游 service，这样 call() 才能把 service 移进 async block——
        // 我们必须在调用下游之前 await 一次数据库查询（取当前 role）。
        //
        // HttpServer 的约束里没有 Send（App 是在各 worker 线程内构造的），
        // 所以这里用 Rc 而不是 Arc。
        ok(JwtAuthMiddleware {
            service: Rc::new(service),
        })
        .boxed_local()
    }
}

pub struct JwtAuthMiddleware<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for JwtAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixWebError;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            let user = authenticate(&req).await?;
            req.extensions_mut().insert(AuthenticatedUser { user });
            service.call(req).await
        })
        .boxed_local()
    }
}

// --- Extractor for Authenticated User ---
impl actix_web::FromRequest for AuthenticatedUser {
    type Error = ActixWebError;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        match req.extensions().get::<AuthenticatedUser>() {
            Some(auth_user) => ok(auth_user.clone()).boxed_local(),
            None => {
                log::error!(
                    "AuthenticatedUser missing from request extensions; \
                     is JwtAuth applied to this scope?"
                );
                ready(Err(
                    ServiceError::Unauthorized("Not authenticated".to_string()).into(),
                ))
                .boxed_local()
            }
        }
    }
}

// --- Permission Guard Middleware ---

/// 权限判定方式。
#[derive(Debug, Clone, Copy)]
enum Requirement {
    /// 必须拥有全部指定权限
    All(Permissions),
    /// 拥有任意一个指定权限即可
    Any(Permissions),
}

impl Requirement {
    fn is_met_by(self, held: Permissions) -> bool {
        match self {
            Requirement::All(required) => held.contains(required),
            Requirement::Any(required) => held.intersects(required),
        }
    }

    fn describe(self) -> String {
        match self {
            Requirement::All(required) => format!("all of {required:?}"),
            Requirement::Any(required) => format!("any of {required:?}"),
        }
    }
}

/// 依赖 `AuthenticatedUser` 已在请求扩展里，因此必须包在 [`JwtAuth`] 的**内侧**
/// （即 `JwtAuth` 后注册）。actix-web 的 `wrap` 是后注册的先执行。
pub struct PermissionGuard {
    requirement: Requirement,
}

impl PermissionGuard {
    /// 要求同时拥有 `required` 里的全部权限。
    pub fn all(required: Permissions) -> Self {
        Self {
            requirement: Requirement::All(required),
        }
    }

    /// 只要拥有 `required` 里的任意一个权限即可。
    pub fn any(required: Permissions) -> Self {
        Self {
            requirement: Requirement::Any(required),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for PermissionGuard
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixWebError;
    type InitError = ();
    type Transform = PermissionGuardMiddleware<S>;
    type Future = LocalBoxFuture<'static, Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(PermissionGuardMiddleware {
            service,
            requirement: self.requirement,
        })
        .boxed_local()
    }
}

pub struct PermissionGuardMiddleware<S> {
    service: S,
    requirement: Requirement,
}

impl<S, B> Service<ServiceRequest> for PermissionGuardMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = ActixWebError> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = ActixWebError;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // 新作用域用来限制对 req.extensions() 的借用，之后才能移动 req。
        let decision = {
            match req.extensions().get::<AuthenticatedUser>() {
                Some(auth_user) => {
                    if self.requirement.is_met_by(auth_user.role()) {
                        Ok(())
                    } else {
                        log::warn!(
                            "user {} with role {:?} denied access requiring {}",
                            auth_user.id(),
                            auth_user.role(),
                            self.requirement.describe()
                        );
                        // 401 的语义是「未认证」；已认证但权限不足应该是 403。
                        Err(ServiceError::Forbidden(
                            "Insufficient permissions".to_string(),
                        ))
                    }
                }
                None => {
                    log::error!(
                        "PermissionGuard ran without AuthenticatedUser in extensions; \
                         JwtAuth must be wrapped outside of it"
                    );
                    Err(ServiceError::Unauthorized(
                        "Authentication required".to_string(),
                    ))
                }
            }
        };

        match decision {
            Ok(()) => self.service.call(req).boxed_local(),
            Err(err) => ready(Err(err.into())).boxed_local(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[test]
    fn bearer_token_extraction() {
        assert_eq!(bearer_token(Some("Bearer abc.def.ghi")), Some("abc.def.ghi"));
        assert_eq!(bearer_token(Some("Bearer ")), Some(""));
        // 缺少 scheme 前缀
        assert_eq!(bearer_token(Some("abc.def.ghi")), None);
        // scheme 大小写敏感（与原实现一致）
        assert_eq!(bearer_token(Some("bearer abc")), None);
        assert_eq!(bearer_token(Some("")), None);
        assert_eq!(bearer_token(None), None);
    }

    #[test]
    fn all_requires_every_flag() {
        let req = Requirement::All(Permissions::ADMIN | Permissions::STAFF);
        assert!(!req.is_met_by(Permissions::ADMIN));
        assert!(!req.is_met_by(Permissions::STAFF));
        assert!(req.is_met_by(Permissions::ADMIN | Permissions::STAFF));
    }

    #[test]
    fn any_accepts_a_single_flag() {
        let req = Requirement::Any(Permissions::STAFF | Permissions::ADMIN);
        assert!(req.is_met_by(Permissions::ADMIN));
        assert!(req.is_met_by(Permissions::STAFF));
        assert!(!req.is_met_by(Permissions::NONE));
    }

    #[test]
    fn single_admin_guard_behaves_the_same_for_all_and_any() {
        let all = Requirement::All(Permissions::ADMIN);
        let any = Requirement::Any(Permissions::ADMIN);
        for held in [
            Permissions::NONE,
            Permissions::STAFF,
            Permissions::ADMIN,
            Permissions::ADMIN | Permissions::STAFF,
        ] {
            assert_eq!(all.is_met_by(held), any.is_met_by(held));
            assert_eq!(all.is_met_by(held), held.contains(Permissions::ADMIN));
        }
    }

    fn test_config(secret: &str) -> AppConfig {
        AppConfig {
            jwt_secret: secret.to_string(),
            jwt_expiration_hours: 24,
        }
    }

    #[test]
    fn jwt_round_trip_preserves_token_version() {
        let config = test_config(&"s".repeat(32));
        let user_id = Uuid::new_v4();

        let token = generate_jwt(user_id, 7, &config).unwrap();
        let claims = decode_jwt(&token, &config.jwt_secret).unwrap();

        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.ver, 7);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn decode_rejects_token_signed_with_a_different_secret() {
        let issuer = test_config(&"a".repeat(32));
        let other = test_config(&"b".repeat(32));

        let token = generate_jwt(Uuid::new_v4(), 1, &issuer).unwrap();
        assert!(decode_jwt(&token, &other.jwt_secret).is_err());
    }

    /// 回归测试：本次改动之前签发的 token 没有 `ver` 字段，必须被拒。
    /// 否则「改密码作废旧 token」会留下一个永远有效的后门。
    #[test]
    fn decode_rejects_legacy_token_without_version() {
        #[derive(Serialize)]
        struct LegacyClaims {
            sub: String,
            exp: usize,
            iat: usize,
        }

        let config = test_config(&"c".repeat(32));
        let now = Utc::now().timestamp() as usize;

        let token = encode(
            &Header::default(),
            &LegacyClaims {
                sub: Uuid::new_v4().to_string(),
                exp: now + 3600,
                iat: now,
            },
            &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
        )
        .unwrap();

        assert!(decode_jwt(&token, &config.jwt_secret).is_err());
    }
}
