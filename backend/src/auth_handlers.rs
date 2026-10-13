use crate::config::AppConfig;
use crate::db::{blocking_cpu, blocking_db, DbPool};
use crate::errors::ServiceError;
use crate::middleware::{generate_jwt, AuthenticatedUser};
use crate::models::{
    normalize_optional_text, optional_text_changeset, AuthResponse, LoginUserDto, NewUser,
    SignupRequestDto, UpdateSelfRequestDto, User, UserResponse,
};
use crate::permissions::Permissions;
use crate::schema::users;
use actix_web::{get, patch, post, web, HttpResponse};
use bcrypt::{hash, verify, DEFAULT_COST};
use diesel::prelude::*;
use std::sync::LazyLock;
use uuid::Uuid;

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

/// 登录页上的自助注册。
///
/// 这是唯一一个无需认证就能创建账号的入口，所以：
///
/// - **权限写死为 `NONE`**，请求体里没有 `role` 字段，客户端无法自报权限；
/// - 和 `/auth/login` 共用同一套按 IP 限流（bcrypt cost=12 是约 250ms 的
///   纯 CPU 工作，不限流就是一个现成的 CPU 耗尽入口）；
/// - 用户名查重与登录一样存在 TOCTOU，并发下靠唯一约束兜底成 409。
#[post("/register")]
async fn register(
    pool: web::Data<DbPool>,
    signup_data: web::Json<SignupRequestDto>,
) -> Result<HttpResponse, ServiceError> {
    signup_data.validate()?;

    let name = signup_data.name.clone();
    let username = signup_data.username.clone();
    let real_name = signup_data.real_name.trim().to_string();
    // 选填字段：去空白，空串存 NULL。
    let student_id = normalize_optional_text(&signup_data.student_id);
    let organization = normalize_optional_text(&signup_data.organization);
    let contact = normalize_optional_text(&signup_data.contact);
    let wechat = normalize_optional_text(&signup_data.wechat);
    let password = signup_data.password.clone();

    // 与 /admin/user/add 一致：哈希放到 blocking 线程池，不占数据库连接。
    let password_hash = blocking_cpu(move || {
        hash(&password, DEFAULT_COST).map_err(|e| {
            log::error!("bcrypt hashing failed: {}", e);
            ServiceError::InternalServerError
        })
    })
    .await?;

    let new_user_id = Uuid::new_v4();

    let created = blocking_db(pool, move |conn| {
        let existing = users::table
            .filter(users::username.eq(&username))
            .select(users::id)
            .first::<Uuid>(conn)
            .optional()?;

        if existing.is_some() {
            return Err(ServiceError::Conflict("Username already exists".to_string()));
        }

        let new_user = NewUser {
            id: new_user_id,
            name,
            username,
            password_hash,
            // 自助注册的账号一律无权限，等管理员在「用户管理」里审批。
            role: Permissions::NONE,
            real_name: Some(real_name),
            student_id,
            organization,
            contact,
            wechat,
        };

        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(conn)?;

        users::table
            .find(new_user_id)
            .select(User::as_select())
            .first::<User>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Created().json(UserResponse::from(created)))
}

/// 读自己的账号资料。空路径 = 挂载在 `/me` 作用域根上。
///
/// 只要 token 有效就能读，不要求 STAFF / ADMIN：无权限账号也需要看到自己的
/// 注册信息和审批状态。
#[get("")]
async fn get_me(auth_user: AuthenticatedUser) -> Result<HttpResponse, ServiceError> {
    // JwtAuth 已经回查过数据库并把整行 User 放进了请求扩展，
    // 这里直接用即可——原实现会为此再打一次库。
    Ok(HttpResponse::Ok().json(UserResponse::from(auth_user.user)))
}

/// 自助修改的 changeset。只有注册信息和 password_hash，
/// 因此这条路径永远碰不到 username / role。
///
/// 选填的注册信息用 `Option<Option<String>>`：外层 None = 不修改，
/// 内层 None = 置空（写 NULL）。见 `models::optional_text_changeset`。
#[derive(AsChangeset, Debug)]
#[diesel(table_name = crate::schema::users)]
struct SelfChangeset {
    name: Option<String>,
    real_name: Option<String>,
    password_hash: Option<String>,
    student_id: Option<Option<String>>,
    organization: Option<Option<String>>,
    contact: Option<Option<String>>,
    wechat: Option<Option<String>>,
}

/// 任何已登录用户（包括无权限账号）都可以改自己的注册信息（昵称 / 真实姓名 /
/// 学号 / 单位 / 联系方式 / 微信号）和密码。
///
/// 与管理员的 `PATCH /admin/user/{user_id}` 是两条独立路径：这里的目标用户
/// 恒等于 token 里的 `sub`，路径里没有 user_id，所以不存在改别人的可能。
/// 请求体里也没有 username / role，提权在类型层面就不可达。
///
/// 改密码会把 `token_version` +1，于是**当前这把 token 也会立即失效**，
/// 客户端必须重新登录（前端在改密码成功后主动登出）。只改资料不递增版本号。
#[patch("")]
async fn update_me(
    pool: web::Data<DbPool>,
    request_data: web::Json<UpdateSelfRequestDto>,
    auth_user: AuthenticatedUser,
) -> Result<HttpResponse, ServiceError> {
    request_data.validate()?;
    let user_id = auth_user.id();

    let password_hash = match &request_data.password {
        None => None,
        Some(p) => {
            let p = p.clone();
            Some(
                blocking_cpu(move || {
                    hash(&p, DEFAULT_COST).map_err(|e| {
                        log::error!("bcrypt hashing failed: {}", e);
                        ServiceError::InternalServerError
                    })
                })
                .await?,
            )
        }
    };

    let bump_token_version = password_hash.is_some();

    let changeset = SelfChangeset {
        name: request_data.name.clone(),
        // 真实姓名已在 validate 里确认非空，所以提供了就是改成这个值。
        real_name: request_data.real_name.as_ref().map(|v| v.trim().to_string()),
        password_hash,
        student_id: optional_text_changeset(&request_data.student_id),
        organization: optional_text_changeset(&request_data.organization),
        contact: optional_text_changeset(&request_data.contact),
        wechat: optional_text_changeset(&request_data.wechat),
    };

    if changeset.name.is_none()
        && changeset.real_name.is_none()
        && changeset.password_hash.is_none()
        && changeset.student_id.is_none()
        && changeset.organization.is_none()
        && changeset.contact.is_none()
        && changeset.wechat.is_none()
    {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }

    let updated = blocking_db(pool, move |conn| {
        conn.transaction::<User, ServiceError, _>(|conn| {
            // 与 update_user 同一套机制：用 col = col + 1 而不是「先读后写」，
            // 避免并发下丢失递增。递增和主 UPDATE 必须在同一事务里。
            if bump_token_version {
                diesel::update(users::table.find(user_id))
                    .set(users::token_version.eq(users::token_version + 1))
                    .execute(conn)?;
            }

            diesel::update(users::table.find(user_id))
                .set(&changeset)
                .returning(User::as_select())
                .get_result::<User>(conn)
                .map_err(ServiceError::from)
        })
    })
    .await?;

    Ok(HttpResponse::Ok().json(UserResponse::from(updated)))
}
