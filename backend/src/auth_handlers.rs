use crate::config::AppConfig;
use crate::db::{blocking_cpu, blocking_db, DbPool};
use crate::errors::ServiceError;
use crate::middleware::{generate_jwt, AuthenticatedUser};
use crate::models::{AuthResponse, LoginUserDto, User, UserResponse};
use crate::schema::users;
use actix_web::{get, post, web, HttpResponse};
use bcrypt::{hash, verify, DEFAULT_COST};
use diesel::prelude::*;
use std::sync::LazyLock;

/// 用户名不存在时用来「凑时长」的假 hash。
///
/// 原实现在用户不存在时直接返回，而存在时会跑一次 cost=12 的 bcrypt（约 250ms）。
/// 攻击者可以靠响应时间区分「用户名存在」和「不存在」，从而枚举用户名。
/// 用 LazyLock 生成而不是硬编码一个常量，是为了让它的 cost 始终跟随 DEFAULT_COST。
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash("timing-equalisation-placeholder", DEFAULT_COST)
        .expect("hashing a fixed literal with bcrypt cannot fail")
});

#[post("/login")]
async fn login(
    pool: web::Data<DbPool>,
    config: web::Data<AppConfig>,
    login_data: web::Json<LoginUserDto>,
) -> Result<HttpResponse, ServiceError> {
    let username = login_data.username.clone();
    let password = login_data.password.clone();

    let found = blocking_db(pool, move |conn| {
        users::table
            .filter(users::username.eq(&username))
            .select(User::as_select())
            .first::<User>(conn)
            .optional()
            .map_err(ServiceError::from)
    })
    .await?;

    // bcrypt cost=12 是约 250ms 的纯 CPU 工作，必须放到 blocking 线程池上，
    // 否则会阻塞 worker 的事件循环。
    let (user, password_ok) = blocking_cpu(move || match found {
        Some(user) => {
            let ok = verify(&password, &user.password_hash).map_err(|e| {
                log::error!("bcrypt verification failed: {}", e);
                ServiceError::InternalServerError
            })?;
            Ok((Some(user), ok))
        }
        None => {
            // 结果本身无意义，只为消耗与真实分支同量级的 CPU 时间。
            let _ = verify(&password, DUMMY_HASH.as_str());
            Ok((None, false))
        }
    })
    .await?;

    // 用户不存在与密码错误返回完全相同的响应，避免用户名枚举。
    if !password_ok {
        return Err(ServiceError::Unauthorized(
            "Invalid username or password".to_string(),
        ));
    }

    let user = user.ok_or(ServiceError::InternalServerError)?;
    let token = generate_jwt(user.id, user.token_version, &config)?;

    Ok(HttpResponse::Ok().json(AuthResponse {
        token,
        user: UserResponse::from(user),
    }))
}

#[get("/me")]
async fn get_me(auth_user: AuthenticatedUser) -> Result<HttpResponse, ServiceError> {
    // JwtAuth 已经回查过数据库并把整行 User 放进了请求扩展，
    // 这里直接用即可——原实现会为此再打一次库。
    Ok(HttpResponse::Ok().json(UserResponse::from(auth_user.user)))
}
