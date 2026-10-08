// src/user_handlers.rs
use crate::db::{blocking_cpu, blocking_db, DbPool};
use crate::errors::ServiceError;
use crate::middleware::AuthenticatedUser;
use crate::models::{NewUser, RegisterUserDto, UpdateUserRequestDto, User, UserResponse};
use crate::permissions::Permissions;
use crate::schema::users;
use actix_web::{delete, get, patch, post, web, HttpResponse};
use bcrypt::{hash, DEFAULT_COST};
use diesel::prelude::*;
use uuid::Uuid;

// 注意路由前缀：这些 handler 挂在 web::scope("/admin") 下。
// update_user 原来是 #[patch("/{user_id}")]，实际端点是 /admin/{user_id}，
// 与同目录其它三个 /admin/user/... 不一致，已修正为 /user/{user_id}。

#[post("/user/add")]
async fn register(
    pool: web::Data<DbPool>,
    user_data: web::Json<RegisterUserDto>,
) -> Result<HttpResponse, ServiceError> {
    // 先把长度校验做掉：否则超长输入会撞数据库约束，返回 500 而不是 400。
    user_data.validate()?;

    let name = user_data.name.clone();
    let username = user_data.username.clone();
    let role = user_data.role;
    let password = user_data.password.clone();

    // bcrypt 是 CPU 密集的，放到 blocking 线程池，并且不要占着数据库连接。
    let password_hash = blocking_cpu(move || {
        hash(&password, DEFAULT_COST).map_err(|e| {
            log::error!("bcrypt hashing failed: {}", e);
            ServiceError::InternalServerError
        })
    })
    .await?;

    let new_user_id = Uuid::new_v4();

    // 「查重 → 插入 → 回读」在一次 blocking 调用里完成。
    // 「先查后插」在并发下仍会撞唯一约束（TOCTOU）：那条路径由
    // ServiceError::from(diesel::result::Error) 统一映射成 409，不再是 500。
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
            role,
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

#[get("/user/{user_id}")]
async fn get_user_by_id(
    pool: web::Data<DbPool>,
    user_id_path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let user_id = user_id_path.into_inner();

    let user = blocking_db(pool, move |conn| {
        users::table
            .find(user_id)
            .select(User::as_select())
            .first::<User>(conn)
            .optional()
            .map_err(ServiceError::from)
    })
    .await?
    // 找不到应该是 404，原实现返回的是 BadRequest(400)。
    .ok_or_else(|| ServiceError::NotFound(format!("User {user_id} not found")))?;

    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
}

#[derive(AsChangeset, Debug)]
#[diesel(table_name = crate::schema::users)]
struct UserChangeset {
    name: Option<String>,
    username: Option<String>,
    role: Option<Permissions>,
    password_hash: Option<String>,
}

#[patch("/user/{user_id}")]
async fn update_user(
    pool: web::Data<DbPool>,
    user_id_path: web::Path<Uuid>,
    request_data: web::Json<UpdateUserRequestDto>,
    auth_user: AuthenticatedUser,
) -> Result<HttpResponse, ServiceError> {
    request_data.validate()?;
    let user_id = user_id_path.into_inner();

    // 不允许管理员改自己的角色：唯一的管理员把自己降成 NONE 就把系统锁死了。
    // delete_user 已有同类的自我保护，这里原本缺失。
    if auth_user.id() == user_id && request_data.role.is_some() {
        return Err(ServiceError::BadRequest(
            "You cannot change your own role".to_string(),
        ));
    }

    // 空字符串密码不再被静默忽略：原实现会落进一个空的 if 分支，
    // 客户端以为改了密码，实际上什么都没发生。
    let password_hash = match &request_data.password {
        None => None,
        Some(p) if p.is_empty() => {
            return Err(ServiceError::ValidationError(
                "password must not be empty (omit the field to leave it unchanged)".to_string(),
            ))
        }
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

    let changeset = UserChangeset {
        name: request_data.name.clone(),
        username: request_data.username.clone(),
        role: request_data.role,
        password_hash,
    };

    if changeset.name.is_none()
        && changeset.username.is_none()
        && changeset.role.is_none()
        && changeset.password_hash.is_none()
    {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }

    let updated = blocking_db(pool, move |conn| {
        // 用户名查重。同样存在 TOCTOU，兜底是唯一约束映射成 409。
        if let Some(new_username) = &changeset.username {
            let taken = users::table
                .filter(users::username.eq(new_username).and(users::id.ne(user_id)))
                .select(users::id)
                .first::<Uuid>(conn)
                .optional()?;

            if taken.is_some() {
                return Err(ServiceError::Conflict(format!(
                    "Username '{new_username}' is already taken."
                )));
            }
        }

        conn.transaction::<User, ServiceError, _>(|conn| {
            // 改密码必须让已签发的 token 失效，否则旧 token 在最长 24h 内仍然可用。
            // 用 `col = col + 1` 而不是「先读后写」，避免并发下丢失递增。
            if bump_token_version {
                diesel::update(users::table.find(user_id))
                    .set(users::token_version.eq(users::token_version + 1))
                    .execute(conn)?;
            }

            diesel::update(users::table.find(user_id))
                .set(&changeset)
                .returning(User::as_select())
                .get_result::<User>(conn)
                .optional()?
                .ok_or_else(|| ServiceError::NotFound(format!("User {user_id} not found")))
        })
    })
    .await?;

    Ok(HttpResponse::Ok().json(UserResponse::from(updated)))
}

#[delete("/user/{user_id}")]
async fn delete_user(
    pool: web::Data<DbPool>,
    user_id_path: web::Path<Uuid>,
    auth_user: AuthenticatedUser,
) -> Result<HttpResponse, ServiceError> {
    let user_id_to_delete = user_id_path.into_inner();

    if auth_user.id() == user_id_to_delete {
        return Err(ServiceError::BadRequest(
            "Admins cannot delete themselves through this endpoint.".to_string(),
        ));
    }

    let deleted = blocking_db(pool, move |conn| {
        diesel::delete(users::table.find(user_id_to_delete))
            .execute(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    if deleted == 0 {
        Err(ServiceError::NotFound(format!(
            "User {user_id_to_delete} not found"
        )))
    } else {
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "message": "User deleted successfully",
            "id": user_id_to_delete
        })))
    }
}

#[get("/users")]
async fn list_users(pool: web::Data<DbPool>) -> Result<HttpResponse, ServiceError> {
    // 明确决定不做分页：仍然是无条件全表读取。
    let all_users = blocking_db(pool, |conn| {
        users::table
            .select(User::as_select())
            .load::<User>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    let response: Vec<UserResponse> = all_users.iter().map(UserResponse::from).collect();

    Ok(HttpResponse::Ok().json(response))
}
