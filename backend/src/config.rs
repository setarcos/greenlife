use std::env;

/// 运行期配置，在启动时一次性读入。
///
/// 原实现把 `env::var("JWT_SECRET").expect(...)` 放在请求路径上（见旧 middleware.rs），
/// 有两个问题：环境变量缺失会 panic 掉 actix worker 线程；而且每个请求都要做一次
/// 加锁的环境变量查找。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub jwt_secret: String,
    pub jwt_expiration_hours: i64,
}

/// HMAC-SHA256 的密钥长度下限。短于此值会告警但不拒绝启动，
/// 以免打断已有部署。
const MIN_SECRET_LEN: usize = 32;

const DEFAULT_EXPIRATION_HOURS: i64 = 24;

impl AppConfig {
    pub fn from_env() -> Self {
        let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

        if jwt_secret.len() < MIN_SECRET_LEN {
            log::warn!(
                "JWT_SECRET is only {} bytes; HMAC-SHA256 keys should be at least {} bytes \
                 (generate one with: head -c 48 /dev/urandom | base64)",
                jwt_secret.len(),
                MIN_SECRET_LEN
            );
        }

        let jwt_expiration_hours = match env::var("JWT_EXPIRATION_HOURS") {
            Ok(raw) => raw.parse::<i64>().unwrap_or_else(|_| {
                log::warn!(
                    "JWT_EXPIRATION_HOURS={raw:?} is not a valid integer, \
                     falling back to {DEFAULT_EXPIRATION_HOURS}"
                );
                DEFAULT_EXPIRATION_HOURS
            }),
            Err(_) => DEFAULT_EXPIRATION_HOURS,
        };

        Self {
            jwt_secret,
            jwt_expiration_hours,
        }
    }
}
