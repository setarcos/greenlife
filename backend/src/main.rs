mod auth_handlers;
mod bird_handlers;
mod bird_models;
mod config;
mod db;
mod errors;
mod maintenance_handlers;
mod maintenance_models;
mod middleware;
mod models;
mod permissions;
mod photo_handlers;
mod photo_models;
mod rate_limit;
mod schema;
mod taxonomy_handlers;
mod taxonomy_models;
mod user_handlers;

use crate::config::{AppConfig, UPLOAD_URL_PREFIX};
use crate::db::DbPool;
use crate::permissions::Permissions;
use actix_files::Files;
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

    // 登录/注册限流：每个客户端 IP 允许 5 次突发，之后每 2 秒补充 1 次。
    //
    // 自助注册也走这套限流：它同样要跑一次 cost=12 的 bcrypt，不限流就是一个
    // 现成的 CPU 耗尽入口（两者共用额度是有意的）。
    //
    // 必须在 HttpServer::new 之外构造：在闭包内构造会让每个 worker 持有独立的
    // 限流器，实际额度变成 worker 数 × burst_size。
    //
    // 用 RealIpKeyExtractor 而不是默认的 PeerIpKeyExtractor：后者只看 TCP 对端
    // 地址，nginx 反代时永远是 127.0.0.1，限流会退化成全局限流。
    let auth_governor = GovernorConfigBuilder::default()
        .burst_size(5)
        .seconds_per_request(2)
        .key_extractor(rate_limit::RealIpKeyExtractor)
        .finish()
        .expect("login rate limit config is valid");

    // 照片目录。开发环境由后端直接提供静态文件；线上 nginx 的 `location /uploads/`
    // 会先命中，根本不会转发到这里（见 README）。
    let upload_dir = config.upload_path.clone();

    log::info!("Starting server at http://{}", server_address);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db_pool.clone()))
            .app_data(web::Data::new(config.clone()))
            // %a 是 TCP 对端地址，反代场景下永远是 nginx（127.0.0.1），所以额外打上
            // X-Forwarded-For（nginx 必须配 `proxy_set_header X-Forwarded-For
            // $proxy_add_x_forwarded_for;`，见 README）。配了 realip 时末尾那段就是真实
            // 客户端 IP；请求没带这个头时显示 `-`。
            .wrap(Logger::new(
                "%a %{X-Forwarded-For}i \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T",
            ))
            .service(health)
            .service(
                web::scope("/auth")
                    .wrap(Governor::new(&auth_governor))
                    .service(auth_handlers::login)
                    // 登录页上的自助注册：无需认证，创建的账号恒为无权限（NONE）。
                    .service(auth_handlers::register),
            )
            .service(
                web::scope("/admin")
                    // actix-web 的 wrap 是「后注册的先执行」，所以写在下面的 JwtAuth
                    // 才是外层。PermissionGuard 依赖 JwtAuth 放进请求扩展里的
                    // AuthenticatedUser，顺序颠倒会直接 401。
                    .wrap(middleware::PermissionGuard::all(Permissions::ADMIN))
                    .wrap(middleware::JwtAuth)
                    .service(user_handlers::register)
                    .service(user_handlers::approve_user)
                    .service(user_handlers::get_user_by_id)
                    .service(user_handlers::update_user)
                    .service(user_handlers::delete_user)
                    .service(user_handlers::list_users),
            )
            // 自己的账号资料：任何已登录用户都能读写，包括无权限（NONE）账号
            // —— 自助注册后就能保持登录、查看 / 修改自己的注册信息，
            // 不必等管理员审批。上传、编辑物种数据等写接口仍在 /staff 下，
            // 仍然要 STAFF 或 ADMIN。
            //
            // 路径是 /me 而不是 /staff/me：它不属于「员工」资源，
            // 放在 /staff 作用域里会被 PermissionGuard 拦住。
            .service(
                web::scope("/me")
                    .wrap(middleware::JwtAuth)
                    .service(auth_handlers::get_me)
                    .service(auth_handlers::update_me),
            )
            .service(
                web::scope("/staff")
                    // STAFF 或 ADMIN 都可以访问
                    .wrap(middleware::PermissionGuard::any(
                        Permissions::STAFF | Permissions::ADMIN,
                    ))
                    .wrap(middleware::JwtAuth)
                    // 分类树 / 名录的写接口：STAFF 和 ADMIN 都能维护。
                    // 嵌套 scope 会继承父 scope 的中间件，所以不用重复 wrap。
                    .service(
                        web::scope("/taxonomy")
                            .service(taxonomy_handlers::create_taxon)
                            .service(taxonomy_handlers::update_taxon)
                            .service(taxonomy_handlers::delete_taxon)
                            .service(taxonomy_handlers::create_list)
                            .service(taxonomy_handlers::update_list)
                            .service(taxonomy_handlers::delete_list)
                            .service(taxonomy_handlers::create_record)
                            .service(taxonomy_handlers::update_record)
                            .service(taxonomy_handlers::delete_record),
                    )
                    // 维护日志的读 + 写接口。这是唯一一个连读都要 STAFF / ADMIN 的
                    // 资源（无权限用户和游客都看不到），所以读接口也放在这个 scope 里，
                    // 不再提供公开的 /maintenance-logs 路由。
                    .service(
                        web::scope("/maintenance-logs")
                            .service(maintenance_handlers::list_maintenance_logs)
                            .service(maintenance_handlers::get_maintenance_log)
                            .service(maintenance_handlers::create_maintenance_log)
                            .service(maintenance_handlers::update_maintenance_log)
                            .service(maintenance_handlers::delete_maintenance_log),
                    )
                    // 鸟类调查「重要记录」的写接口。
                    .service(
                        web::scope("/bird-records")
                            .service(bird_handlers::create_bird_record)
                            .service(bird_handlers::update_bird_record)
                            .service(bird_handlers::delete_bird_record),
                    )
                    // 物种照片的写接口。上传是 multipart，放宽 payload 上限，
                    // 让「2 MiB 照片 + 表单字段」过得去（具体大小在处理器里再校）。
                    .service(
                        web::scope("/photos")
                            .app_data(web::PayloadConfig::new(
                                photo_models::MAX_UPLOAD_REQUEST_BYTES,
                            ))
                            .service(photo_handlers::create_photo)
                            .service(photo_handlers::update_photo)
                            .service(photo_handlers::delete_photo),
                    ),
            )
            // 分类树 / 名录的读接口：公开，未登录也能浏览物种。
            .service(
                web::scope("/taxonomy")
                    .service(taxonomy_handlers::list_ranks)
                    .service(taxonomy_handlers::list_taxa)
                    .service(taxonomy_handlers::get_taxon)
                    .service(taxonomy_handlers::get_tree)
                    .service(taxonomy_handlers::list_lists)
                    .service(taxonomy_handlers::get_list)
                    .service(taxonomy_handlers::list_records)
                    .service(taxonomy_handlers::get_record),
            )
            // 鸟类调查「重要记录」的读接口：同样公开。
            .service(
                web::scope("/bird-records")
                    .service(bird_handlers::list_bird_records)
                    .service(bird_handlers::get_bird_record),
            )
            // 物种照片的读接口：同样公开。
            .service(
                web::scope("/photos")
                    .service(photo_handlers::list_photos)
                    .service(photo_handlers::get_photo),
            )
            // 开发环境直接由后端提供照片文件；线上由 nginx 的 `location /uploads/`
            // 指向 UPLOAD_PATH，这条路由不会命中。
            .service(Files::new(UPLOAD_URL_PREFIX, upload_dir.clone()).prefer_utf8(true))
    })
    .bind(&server_address)?
    .run()
    .await
}
