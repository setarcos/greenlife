use crate::errors::ServiceError;
use actix_web::web;
use diesel::r2d2::{self, ConnectionManager};
use diesel::PgConnection;
use dotenvy::dotenv;
use std::env;

pub type DbPool = r2d2::Pool<ConnectionManager<PgConnection>>;
pub type DbConn = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

pub fn establish_connection_pool() -> DbPool {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let manager = ConnectionManager::<PgConnection>::new(database_url);

    r2d2::Pool::builder()
        .build(manager)
        .expect("Failed to create database connection pool.")
}

/// 把同步的数据库访问挪到 actix 的 blocking 线程池上。
///
/// 直接在 handler 里跑同步 diesel 会阻塞 worker 的事件循环——每个 worker 只有一条
/// 线程，一个慢查询就会拖住该 worker 上所有其它请求。
///
/// 连接池取连接失败返回 503，而不是像原来那样 `expect()` 掉整个 worker 线程。
pub async fn blocking_db<T, F>(pool: web::Data<DbPool>, f: F) -> Result<T, ServiceError>
where
    F: FnOnce(&mut DbConn) -> Result<T, ServiceError> + Send + 'static,
    T: Send + 'static,
{
    let pooled = web::block(move || {
        let mut conn = pool.get().map_err(|e| {
            // 连接池耗尽/超时是瞬时状态：返回 503 让调用方可重试，
            // 而不是原来的 expect() 直接打掉整个 worker 线程。
            log::error!("database connection pool unavailable: {}", e);
            ServiceError::ServiceUnavailable("Database temporarily unavailable".to_string())
        })?;
        f(&mut conn)
    })
    .await
    .map_err(|e| {
        // blocking 线程池关闭，或者闭包 panic 了
        log::error!("blocking database task failed: {:?}", e);
        ServiceError::InternalServerError
    })?;

    pooled
}

/// 把 CPU 密集的同步计算（bcrypt cost=12 大约 250ms）挪到 blocking 线程池上。
pub async fn blocking_cpu<T, F>(f: F) -> Result<T, ServiceError>
where
    F: FnOnce() -> Result<T, ServiceError> + Send + 'static,
    T: Send + 'static,
{
    web::block(f)
        .await
        .map_err(|e| {
            log::error!("blocking cpu task failed: {:?}", e);
            ServiceError::InternalServerError
        })?
}
