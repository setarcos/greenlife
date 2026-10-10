//! 物种照片的数据模型。
//!
//! 表结构见迁移 `2026-10-12-000000_add_species_photos`。
//!
//! 图片文件不存数据库，只存 `UPLOAD_PATH` 下的相对路径；这里同时提供
//! 「拍摄年月 → 子目录名」「UUID → 文件名」这两条落盘规则，处理器和测试共用。

use crate::errors::ServiceError;
use crate::schema::species_photos;
use crate::taxonomy_models::deserialize_some;
use chrono::{NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// --- 与数据库约束对齐的长度上限 ---
pub const PHOTOGRAPHER_MAX_CHARS: usize = 200; // species_photos.photographer
pub const LOCATION_MAX_CHARS: usize = 200; // species_photos.location
pub const ORIGINAL_FILENAME_MAX_CHARS: usize = 255; // species_photos.original_filename

/// 单个物种的照片上限。
pub const MAX_PHOTOS_PER_TAXON: i64 = 9;

/// 单张照片的字节上限（2 MiB）。超过直接拒绝，不做压缩。
pub const MAX_PHOTO_BYTES: usize = 2 * 1024 * 1024;

/// 整个 multipart 请求体的上限：照片本身 + 表单字段（拍摄人 / 地点 / 备注等）。
/// 给文本字段留 1 MiB，够用且不会被大请求打爆内存。
pub const MAX_UPLOAD_REQUEST_BYTES: usize = MAX_PHOTO_BYTES + 1024 * 1024;

pub const RATING_MIN: i16 = 1;
pub const RATING_MAX: i16 = 10;

/// 允许的图片扩展名（统一小写，jpeg 归一成 jpg）。
pub const ALLOWED_EXTENSIONS: [&str; 4] = ["jpg", "png", "gif", "webp"];

/// 一条物种照片记录。
#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = species_photos)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Photo {
    pub id: Uuid,
    /// 分类树里的物种节点。
    pub taxon_id: Uuid,
    /// 「拍摄人」。
    pub photographer: Option<String>,
    /// 「上传用户名」：上传时从 JWT 取，用户改名后不变。
    pub uploader_username: String,
    /// 「拍摄时间」：精确到天，同时决定照片落在哪个年月子目录。
    pub taken_at: NaiveDate,
    /// 「地点」。
    pub location: Option<String>,
    /// 「备注」。
    pub note: Option<String>,
    /// 「评分」：1-10，NULL 表示还没评。
    pub rating: Option<i16>,
    /// 「是否重要记录」。
    pub is_important: bool,
    /// `UPLOAD_PATH` 下的相对路径，始终用 '/' 分隔。
    pub file_path: String,
    pub original_filename: Option<String>,
    pub file_size: i64,
    pub content_type: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = species_photos)]
pub struct NewPhoto {
    pub id: Uuid,
    pub taxon_id: Uuid,
    pub photographer: Option<String>,
    pub uploader_username: String,
    pub taken_at: NaiveDate,
    pub location: Option<String>,
    pub note: Option<String>,
    pub rating: Option<i16>,
    pub is_important: bool,
    pub file_path: String,
    pub original_filename: Option<String>,
    pub file_size: i64,
    pub content_type: Option<String>,
}

/// 接口返回的照片：数据库字段 + 拼好的公开 URL。
///
/// URL 由后端拼而不是让前端知道 `UPLOAD_PATH`：路径怎么暴露给浏览器是部署
/// 决定（nginx `location /uploads/`），前端只认 `url` 这一个字段。
#[derive(Serialize, Debug, Clone)]
pub struct PhotoResponse {
    #[serde(flatten)]
    pub photo: Photo,
    pub url: String,
}

impl PhotoResponse {
    pub fn new(photo: Photo, url_prefix: &str) -> Self {
        let url = public_url(url_prefix, &photo.file_path);
        Self { photo, url }
    }
}

// ==========================================================================
// 落盘规则
// ==========================================================================

/// 拍摄时间对应的子目录名（`yyyy-mm`）。
pub fn subdirectory(taken_at: NaiveDate) -> String {
    taken_at.format("%Y-%m").to_string()
}

/// `UPLOAD_PATH` 下的相对路径：`yyyy-mm/<uuid>.<ext>`。
///
/// 文件名用新生成的 UUID，既不会和已有文件重名，也不受原始文件名里的
/// 路径分隔符 / 奇怪字符影响。
pub fn relative_file_path(taken_at: NaiveDate, id: Uuid, extension: &str) -> String {
    format!("{}/{}.{}", subdirectory(taken_at), id, extension)
}

/// 公开访问 URL：`<prefix>/<相对路径>`。
pub fn public_url(url_prefix: &str, file_path: &str) -> String {
    format!("{}/{}", url_prefix.trim_end_matches('/'), file_path)
}

/// 归一图片扩展名，只接受白名单内的格式。
///
/// 优先信 `Content-Type`（手机上传的文件名常常没有扩展名），认不出来再退回
/// 文件名后缀。两处都认不出就拒绝，避免把任意文件塞进照片目录。
pub fn image_extension(filename: &str, content_type: Option<&str>) -> Result<String, ServiceError> {
    if let Some(content_type) = content_type {
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !mime.starts_with("image/") {
            return Err(ServiceError::ValidationError(format!(
                "unsupported content type: {content_type:?}"
            )));
        }
        let from_mime = match mime.as_str() {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            _ => None,
        };
        if let Some(ext) = from_mime {
            return Ok(ext.to_string());
        }
    }

    let raw = filename.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    let ext = raw.trim().to_ascii_lowercase();
    let normalized = match ext.as_str() {
        "jpeg" | "jpg" => "jpg",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        _ => {
            return Err(ServiceError::ValidationError(format!(
                "unsupported image extension: {raw:?}; allowed: {}",
                ALLOWED_EXTENSIONS.join(", ")
            )))
        }
    };
    Ok(normalized.to_string())
}

// ==========================================================================
// 上传（multipart 表单里全是字符串，这里负责解析成类型）
// ==========================================================================

#[derive(Debug)]
pub struct CreatePhotoDto {
    pub taxon_id: Uuid,
    pub photographer: Option<String>,
    pub taken_at: NaiveDate,
    pub location: Option<String>,
    pub note: Option<String>,
    pub rating: Option<i16>,
    pub is_important: bool,
}

impl CreatePhotoDto {
    /// 从 multipart 的文本字段里解析。缺字段 / 格式不对都返回 400。
    pub fn from_fields(fields: &HashMap<String, String>) -> Result<Self, ServiceError> {
        let taxon_id = fields
            .get("taxon_id")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ServiceError::ValidationError("taxon_id is required".to_string()))
            .and_then(|v| {
                Uuid::parse_str(v).map_err(|_| {
                    ServiceError::ValidationError("taxon_id must be a UUID".to_string())
                })
            })?;

        let taken_at = fields
            .get("taken_at")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ServiceError::ValidationError("taken_at is required".to_string()))
            .and_then(|v| {
                NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| {
                    ServiceError::ValidationError(
                        "taken_at must be a date in yyyy-mm-dd format".to_string(),
                    )
                })
            })?;

        Ok(Self {
            taxon_id,
            photographer: optional_text(fields, "photographer"),
            taken_at,
            location: optional_text(fields, "location"),
            note: optional_text(fields, "note"),
            rating: optional_rating(fields.get("rating").map(String::as_str))?,
            is_important: optional_bool(fields.get("is_important").map(String::as_str))?,
        })
    }

    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_opt_len("photographer", &self.photographer, PHOTOGRAPHER_MAX_CHARS)?;
        validate_opt_len("location", &self.location, LOCATION_MAX_CHARS)?;
        Ok(())
    }
}

/// 空字符串按「没填」处理，统一成 `None`。
fn optional_text(fields: &HashMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_rating(raw: Option<&str>) -> Result<Option<i16>, ServiceError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let rating = raw.parse::<i16>().map_err(|_| {
        ServiceError::ValidationError("rating must be an integer between 1 and 10".to_string())
    })?;
    if !(RATING_MIN..=RATING_MAX).contains(&rating) {
        return Err(ServiceError::ValidationError(format!(
            "rating must be between {RATING_MIN} and {RATING_MAX}"
        )));
    }
    Ok(Some(rating))
}

/// 复选框 / 单选按钮可能不发字段，也可能发 `"on"` / `"true"` / `"1"`。
fn optional_bool(raw: Option<&str>) -> Result<bool, ServiceError> {
    match raw.map(str::trim) {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(ServiceError::ValidationError(format!(
            "is_important must be a boolean, got {other:?}"
        ))),
    }
}

/// 更新照片的元数据。
///
/// 可空字段用 `Option<Option<T>>`：缺字段 = 不改，显式 `null` = 置 NULL，
/// 有值 = 改值（同 `UpdateBirdRecordDto`）。
///
/// 刻意不允许改 `taken_at` / 文件本身：它们决定文件落在哪个目录，改了就得
/// 移动磁盘上的文件；传错了直接删掉重传，语义更清楚。
#[derive(Deserialize, Debug)]
pub struct UpdatePhotoDto {
    #[serde(default, deserialize_with = "deserialize_some")]
    pub photographer: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub location: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub rating: Option<Option<i16>>,
    pub is_important: Option<bool>,
}

impl UpdatePhotoDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(Some(photographer)) = &self.photographer {
            validate_len("photographer", photographer, PHOTOGRAPHER_MAX_CHARS)?;
        }
        if let Some(Some(location)) = &self.location {
            validate_len("location", location, LOCATION_MAX_CHARS)?;
        }
        if let Some(Some(rating)) = self.rating {
            if !(RATING_MIN..=RATING_MAX).contains(&rating) {
                return Err(ServiceError::ValidationError(format!(
                    "rating must be between {RATING_MIN} and {RATING_MAX}"
                )));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.photographer.is_none()
            && self.location.is_none()
            && self.note.is_none()
            && self.rating.is_none()
            && self.is_important.is_none()
    }
}

// ==========================================================================
// 查询参数
// ==========================================================================

#[derive(Deserialize, Debug)]
pub struct PhotoQuery {
    /// 只看某个物种的照片。
    pub taxon_id: Option<Uuid>,
    /// 只看「重要记录」。
    pub important: Option<bool>,
    /// 在拍摄人 / 上传用户名 / 地点 / 备注里模糊搜索。
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- 校验辅助 ---

fn validate_len(field: &str, value: &str, max_chars: usize) -> Result<(), ServiceError> {
    let chars = value.chars().count();
    if chars > max_chars {
        return Err(ServiceError::ValidationError(format!(
            "{field} must be at most {max_chars} characters, got {chars}"
        )));
    }
    Ok(())
}

fn validate_opt_len(
    field: &str,
    value: &Option<String>,
    max_chars: usize,
) -> Result<(), ServiceError> {
    match value {
        Some(v) => validate_len(field, v, max_chars),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn extension_prefers_content_type_then_filename() {
        // 文件名没有后缀时靠 MIME 兜底（手机上传常见）
        assert_eq!(
            image_extension("IMG_0001", Some("image/jpeg")).unwrap(),
            "jpg"
        );
        // 带参数 / 大小写的 MIME 也能认
        assert_eq!(
            image_extension("x", Some("IMAGE/PNG; charset=binary")).unwrap(),
            "png"
        );
        // 没有 MIME 时退回文件名，jpeg 归一成 jpg
        assert_eq!(image_extension("photo.JPEG", None).unwrap(), "jpg");
        assert_eq!(image_extension("photo.webp", None).unwrap(), "webp");
    }

    #[test]
    fn extension_rejects_non_image_and_unknown() {
        assert!(image_extension("evil.svg", Some("image/svg+xml")).is_err());
        assert!(image_extension("evil.sh", Some("text/x-shellscript")).is_err());
        assert!(image_extension("archive.zip", None).is_err());
        assert!(image_extension("noext", None).is_err());
    }

    #[test]
    fn path_layout_is_year_month_plus_uuid() {
        let taken_at = NaiveDate::from_ymd_opt(2025, 3, 19).unwrap();
        let id = Uuid::parse_str("00000000-0000-0000-0000-0000000000ab").unwrap();
        assert_eq!(subdirectory(taken_at), "2025-03");
        assert_eq!(
            relative_file_path(taken_at, id, "jpg"),
            "2025-03/00000000-0000-0000-0000-0000000000ab.jpg"
        );
        // 月和日补零，10 月不会变成 "2025-1"
        let october = NaiveDate::from_ymd_opt(2025, 10, 1).unwrap();
        assert_eq!(subdirectory(october), "2025-10");
    }

    #[test]
    fn public_url_does_not_double_slash() {
        assert_eq!(
            public_url("/uploads", "2025-03/a.jpg"),
            "/uploads/2025-03/a.jpg"
        );
        assert_eq!(
            public_url("/uploads/", "2025-03/a.jpg"),
            "/uploads/2025-03/a.jpg"
        );
    }

    #[test]
    fn create_dto_parses_and_validates() {
        let fields = field_map(&[
            ("taxon_id", "00000000-0000-0000-0000-000000000001"),
            ("taken_at", "2025-03-19"),
            ("photographer", " 杨延军 "),
            ("location", ""),
            ("rating", "8"),
            ("is_important", "on"),
        ]);
        let dto = CreatePhotoDto::from_fields(&fields).unwrap();
        assert_eq!(dto.photographer.as_deref(), Some("杨延军"));
        assert_eq!(dto.location, None);
        assert_eq!(dto.rating, Some(8));
        assert!(dto.is_important);
        assert!(dto.validate().is_ok());

        // 评分越界 / 非整数都拒
        let bad_rating = field_map(&[
            ("taxon_id", "00000000-0000-0000-0000-000000000001"),
            ("taken_at", "2025-03-19"),
            ("rating", "11"),
        ]);
        assert!(CreatePhotoDto::from_fields(&bad_rating).is_err());

        // 缺必填字段
        assert!(CreatePhotoDto::from_fields(&field_map(&[("taken_at", "2025-03-19")])).is_err());
        assert!(CreatePhotoDto::from_fields(&field_map(&[(
            "taxon_id",
            "00000000-0000-0000-0000-000000000001"
        )]))
        .is_err());

        // 日期格式必须是 yyyy-mm-dd
        let bad_date = field_map(&[
            ("taxon_id", "00000000-0000-0000-0000-000000000001"),
            ("taken_at", "2025/03/19"),
        ]);
        assert!(CreatePhotoDto::from_fields(&bad_date).is_err());
    }

    #[test]
    fn update_distinguishes_absent_null_and_value() {
        let absent: UpdatePhotoDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());

        let cleared: UpdatePhotoDto =
            serde_json::from_str(r#"{"photographer":null,"rating":9}"#).unwrap();
        assert_eq!(cleared.photographer, Some(None));
        assert_eq!(cleared.rating, Some(Some(9)));
        assert!(!cleared.is_empty());
        assert!(cleared.validate().is_ok());

        let bad: UpdatePhotoDto = serde_json::from_str(r#"{"rating":0}"#).unwrap();
        assert!(bad.validate().is_err());
    }
}
