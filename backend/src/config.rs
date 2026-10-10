use std::env;
use std::path::PathBuf;

/// 照片的公开 URL 前缀。
///
/// 线上由反向代理把 `/uploads/` 直接映射到 `UPLOAD_PATH`；开发环境没有 nginx，
/// 由后端自己的静态文件服务提供同一个前缀（见 main.rs），这样前后端在两种环境
/// 下看到的图片 URL 完全一致。
pub const UPLOAD_URL_PREFIX: &str = "/uploads";

/// 运行期配置，在启动时一次性读入。
///
/// 原实现把 `env::var("JWT_SECRET").expect(...)` 放在请求路径上（见旧 middleware.rs），
/// 有两个问题：环境变量缺失会 panic 掉 actix worker 线程；而且每个请求都要做一次
/// 加锁的环境变量查找。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub jwt_secret: String,
    pub jwt_expiration_hours: i64,
    /// 照片存储根目录（`UPLOAD_PATH`）。照片按拍摄年月分子目录存放，
    /// 数据库里只记相对这个目录的路径。
    pub upload_path: PathBuf,
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

        let upload_path = PathBuf::from(env::var("UPLOAD_PATH").expect("UPLOAD_PATH must be set"));
        // 启动时把目录建出来，让部署期的权限/挂载问题在启动日志里就能看到，
        // 而不是等到第一次上传才暴露。目录建不出来不拒绝启动：读取旧照片仍然可用。
        if let Err(e) = std::fs::create_dir_all(&upload_path) {
            log::warn!(
                "UPLOAD_PATH {} is not usable ({}); photo upload will fail",
                upload_path.display(),
                e
            );
        }

        Self {
            jwt_secret,
            jwt_expiration_hours,
            upload_path,
        }
    }
}
