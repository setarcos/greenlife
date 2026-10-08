mod auth_handlers;
mod config;
mod db;
mod errors;
mod middleware;
mod models;
mod permissions;
mod schema;
mod user_handlers;

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::permissions::Permissions;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{get, middleware::Logger, web, App, HttpResponse, HttpServer};
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

/// 存活/就绪探针，顺带验证数据库连通性。
#[get("/health")]
async fn health(pool: web::Data<DbPool>) -> Result<HttpResponse, errors::ServiceError> {
    db::blocking_db(pool, |conn| {
        diesel::sql_query("SELECT 1").execute(conn)?;
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "ok" })))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_pool = db::establish_connection_pool();
    let config = AppConfig::from_env();
    let server_address =
        env::var("SERVER_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    // 登录限流：每个客户端 IP 允许 5 次突发，之后每 2 秒补充 1 次。
    //
    // 必须在 HttpServer::new 之外构造：在闭包内构造会让每个 worker 持有独立的
    // 限流器，实际额度变成 worker 数 × burst_size。
    let login_governor = GovernorConfigBuilder::default()
        .burst_size(5)
        .seconds_per_request(2)
        .finish()
        .expect("login rate limit config is valid");

    log::info!("Starting server at http://{}", server_address);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(config.clone()))
            .wrap(Logger::default())
            .service(health)
            .service(
                web::scope("/auth")
                    .wrap(Governor::new(&login_governor))
                    .service(auth_handlers::login),
            )
            .service(
                web::scope("/admin")
                    // actix-web 的 wrap 是「后注册的先执行」，所以写在下面的 JwtAuth
                    // 才是外层。PermissionGuard 依赖 JwtAuth 放进请求扩展里的
                    // AuthenticatedUser，顺序颠倒会直接 401。
                    .wrap(middleware::PermissionGuard::all(Permissions::ADMIN))
                    .wrap(middleware::JwtAuth)
                    .service(user_handlers::register)
                    .service(user_handlers::get_user_by_id)
                    .service(user_handlers::update_user)
                    .service(user_handlers::delete_user)
                    .service(user_handlers::list_users),
            )
            .service(
                web::scope("/staff")
                    // STAFF 或 ADMIN 都可以访问
                    .wrap(middleware::PermissionGuard::any(
                        Permissions::STAFF | Permissions::ADMIN,
                    ))
                    .wrap(middleware::JwtAuth)
                    .service(auth_handlers::get_me)
                    .service(auth_handlers::update_me),
            )
    })
    .bind(&server_address)?
    .run()
    .await
}
