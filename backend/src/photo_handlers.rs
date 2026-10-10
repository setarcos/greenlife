//! 物种照片的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/photos` 下，写接口挂在 `/staff/photos`
//! （STAFF 或 ADMIN），与 taxonomy / bird-records 的分法一致。
//!
//! 上传是 multipart：图片本身不落数据库，写到 `UPLOAD_PATH/<yyyy-mm>/<uuid>.<ext>`，
//! 数据库只记相对路径。

use crate::config::{AppConfig, UPLOAD_URL_PREFIX};
use crate::db::{blocking_cpu, blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::middleware::AuthenticatedUser;
use crate::photo_models::*;
use crate::schema::{species_photos, taxa};
use crate::taxonomy_handlers::{like_pattern, page, Paged};
use actix_multipart::Multipart;
use actix_web::{delete, get, patch, post, web, HttpResponse};
use diesel::pg::Pg;
use diesel::prelude::*;
use futures_util::TryStreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;

/// 单个文本字段的上限。拍摄人 / 地点都比这个小得多，备注也是短文本；
/// 卡住它是为了不让一个超大表单把内存打满。
const MAX_TEXT_FIELD_BYTES: usize = 64 * 1024;

// ==========================================================================
// 读接口（公开）
// ==========================================================================

/// 列出照片，可按物种 / 是否重要 / 关键字筛选。
#[get("")]
async fn list_photos(
    pool: web::Data<DbPool>,
    query: web::Query<PhotoQuery>,
) -> Result<HttpResponse, ServiceError> {
    let query = query.into_inner();
    let (limit, offset) = page(query.limit, query.offset);
    let pattern = query.q.as_deref().map(like_pattern);
    let taxon_id = query.taxon_id;
    let important = query.important;

    let items = blocking_db(pool, move |conn| {
        let total = photos_query(pattern.clone(), taxon_id, important)
            .count()
            .get_result::<i64>(conn)?;
        let photos = photos_query(pattern, taxon_id, important)
            // 拍摄时间新的在前；同一天按上传先后排，分页结果才是稳定的。
            .order((
                species_photos::taken_at.desc(),
                species_photos::created_at.desc(),
                species_photos::id.desc(),
            ))
            .limit(limit)
            .offset(offset)
            .select(Photo::as_select())
            .load::<Photo>(conn)?;

        let items: Vec<PhotoResponse> = photos
            .into_iter()
            .map(|photo| PhotoResponse::new(photo, UPLOAD_URL_PREFIX))
            .collect();
        Ok(Paged { items, total })
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 照片的筛选条件。取总数和取当前页都走它。
fn photos_query(
    pattern: Option<String>,
    taxon_id: Option<Uuid>,
    important: Option<bool>,
) -> species_photos::BoxedQuery<'static, Pg> {
    let mut q = species_photos::table.into_boxed();
    if let Some(taxon_id) = taxon_id {
        q = q.filter(species_photos::taxon_id.eq(taxon_id));
    }
    if let Some(important) = important {
        q = q.filter(species_photos::is_important.eq(important));
    }
    if let Some(pattern) = pattern {
        q = q.filter(
            species_photos::photographer
                .ilike(pattern.clone())
                .assume_not_null()
                .or(species_photos::uploader_username.ilike(pattern.clone()))
                .or(species_photos::location
                    .ilike(pattern.clone())
                    .assume_not_null())
                .or(species_photos::note.ilike(pattern).assume_not_null()),
        );
    }
    q
}

/// 单张照片。
#[get("/{photo_id}")]
async fn get_photo(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let photo_id = path.into_inner();
    let photo = blocking_db(pool, move |conn| find_photo(conn, photo_id)).await?;
    Ok(HttpResponse::Ok().json(PhotoResponse::new(photo, UPLOAD_URL_PREFIX)))
}

// ==========================================================================
// 写接口（STAFF / ADMIN）
// ==========================================================================

/// 上传一张照片。
///
/// 限制：单个文件 ≤ 2 MiB，单个物种最多 9 张。超过 2 MiB 直接 400，不做压缩。
#[post("")]
async fn create_photo(
    pool: web::Data<DbPool>,
    config: web::Data<AppConfig>,
    auth: AuthenticatedUser,
    payload: Multipart,
) -> Result<HttpResponse, ServiceError> {
    let (fields, file) = read_multipart(payload).await?;
    let file =
        file.ok_or_else(|| ServiceError::ValidationError("photo file is required".to_string()))?;
    if file.bytes.is_empty() {
        return Err(ServiceError::ValidationError(
            "photo file is empty".to_string(),
        ));
    }

    let dto = CreatePhotoDto::from_fields(&fields)?;
    dto.validate()?;
    let extension = image_extension(&file.filename, file.content_type.as_deref())?;

    let id = Uuid::new_v4();
    let relative_path = relative_file_path(dto.taken_at, id, &extension);
    let upload_path = config.upload_path.clone();
    let absolute_path = upload_path.join(&relative_path);
    let size = file.bytes.len() as i64;

    // 磁盘写入是同步 IO，放到 blocking 线程池，别堵住 worker 的事件循环。
    let write_path = absolute_path.clone();
    let bytes = file.bytes;
    blocking_cpu(move || {
        if let Some(parent) = write_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                log::error!(
                    "failed to create upload directory {}: {}",
                    parent.display(),
                    e
                );
                ServiceError::InternalServerError
            })?;
        }
        fs::write(&write_path, &bytes).map_err(|e| {
            log::error!("failed to write photo {}: {}", write_path.display(), e);
            ServiceError::InternalServerError
        })
    })
    .await?;

    let original_filename = if file.filename.trim().is_empty() {
        None
    } else {
        Some(
            file.filename
                .chars()
                .take(ORIGINAL_FILENAME_MAX_CHARS)
                .collect(),
        )
    };
    let new = NewPhoto {
        id,
        taxon_id: dto.taxon_id,
        photographer: dto.photographer,
        uploader_username: auth.user.username.clone(),
        taken_at: dto.taken_at,
        location: dto.location,
        note: dto.note,
        rating: dto.rating,
        is_important: dto.is_important,
        file_path: relative_path,
        original_filename,
        file_size: size,
        content_type: file.content_type,
    };

    let created = match blocking_db(pool, move |conn| insert_photo(conn, new)).await {
        Ok(photo) => photo,
        Err(e) => {
            // 数据库没记上这条记录，磁盘上的文件就成了孤儿，顺手删掉；
            // 年月目录是这次上传新建的，回滚后通常是空的，一并收走。
            if let Err(remove_err) = fs::remove_file(&absolute_path) {
                log::warn!(
                    "failed to clean up orphan photo {}: {}",
                    absolute_path.display(),
                    remove_err
                );
            }
            if let Some(parent) = absolute_path.parent() {
                let _ = fs::remove_dir(parent);
            }
            return Err(e);
        }
    };

    Ok(HttpResponse::Created().json(PhotoResponse::new(created, UPLOAD_URL_PREFIX)))
}

/// 插入照片。物种存在性和「最多 9 张」都在同一个事务里检查：
/// `SELECT ... FOR UPDATE` 锁住物种行，避免并发上传把上限撑破。
fn insert_photo(conn: &mut DbConn, new: NewPhoto) -> Result<Photo, ServiceError> {
    conn.transaction::<Photo, ServiceError, _>(|conn| {
        let taxon = taxa::table
            .find(new.taxon_id)
            .select(taxa::id)
            .for_update()
            .first::<Uuid>(conn)
            .optional()?;
        if taxon.is_none() {
            return Err(ServiceError::ValidationError(format!(
                "taxon {} not found",
                new.taxon_id
            )));
        }

        let count: i64 = species_photos::table
            .filter(species_photos::taxon_id.eq(new.taxon_id))
            .count()
            .get_result(conn)?;
        if count >= MAX_PHOTOS_PER_TAXON {
            return Err(ServiceError::ValidationError(format!(
                "每个物种最多上传 {MAX_PHOTOS_PER_TAXON} 张照片"
            )));
        }

        diesel::insert_into(species_photos::table)
            .values(&new)
            .execute(conn)?;
        species_photos::table
            .find(new.id)
            .select(Photo::as_select())
            .first::<Photo>(conn)
            .map_err(ServiceError::from)
    })
}

/// 改照片的元数据（拍摄人 / 地点 / 备注 / 评分 / 是否重要记录）。
#[patch("/{photo_id}")]
async fn update_photo(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdatePhotoDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let photo_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = find_photo(conn, photo_id)?;

        // 三态合并：None=不改，Some(None)=置 NULL，Some(Some(v))=改成 v。
        let photographer = match dto.photographer {
            None => current.photographer,
            Some(value) => value,
        };
        let location = match dto.location {
            None => current.location,
            Some(value) => value,
        };
        let note = match dto.note {
            None => current.note,
            Some(value) => value,
        };
        let rating = match dto.rating {
            None => current.rating,
            Some(value) => value,
        };
        let is_important = dto.is_important.unwrap_or(current.is_important);

        diesel::update(species_photos::table.find(photo_id))
            .set((
                species_photos::photographer.eq(photographer),
                species_photos::location.eq(location),
                species_photos::note.eq(note),
                species_photos::rating.eq(rating),
                species_photos::is_important.eq(is_important),
                species_photos::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(Photo::as_select())
            .get_result::<Photo>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(PhotoResponse::new(updated, UPLOAD_URL_PREFIX)))
}

/// 删除照片，连同磁盘上的文件。
///
/// 先删数据库记录：即使随后删文件失败，用户也不会再看到这张照片，
/// 失败的只是磁盘上的一点点残留（记在日志里）。
#[delete("/{photo_id}")]
async fn delete_photo(
    pool: web::Data<DbPool>,
    config: web::Data<AppConfig>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let photo_id = path.into_inner();
    let file_path = blocking_db(pool, move |conn| {
        let photo = find_photo(conn, photo_id)?;
        diesel::delete(species_photos::table.find(photo_id)).execute(conn)?;
        Ok(photo.file_path)
    })
    .await?;

    let absolute_path = config.upload_path.join(&file_path);
    if let Err(e) = fs::remove_file(&absolute_path) {
        log::warn!(
            "failed to remove photo file {}: {}",
            absolute_path.display(),
            e
        );
    }

    Ok(HttpResponse::Ok().json(json!({
        "message": "Photo deleted successfully",
        "id": photo_id,
    })))
}

// ==========================================================================
// 公共辅助
// ==========================================================================

fn find_photo(conn: &mut DbConn, photo_id: Uuid) -> Result<Photo, ServiceError> {
    species_photos::table
        .find(photo_id)
        .select(Photo::as_select())
        .first::<Photo>(conn)
        .optional()?
        .ok_or_else(|| ServiceError::NotFound(format!("Photo {photo_id} not found")))
}

/// 一个已读进内存的上传文件。
struct UploadedFile {
    filename: String,
    content_type: Option<String>,
    bytes: Vec<u8>,
}

/// 把 multipart 拆成「文本字段表 + 文件」。
///
/// 文件大小在累积过程中就卡住，不会等到整个请求读完才发现超限：
/// 一旦超过 `MAX_PHOTO_BYTES` 立即返回 400。
async fn read_multipart(
    mut payload: Multipart,
) -> Result<(HashMap<String, String>, Option<UploadedFile>), ServiceError> {
    let mut fields = HashMap::new();
    let mut file: Option<UploadedFile> = None;

    while let Some(mut field) = payload.try_next().await.map_err(multipart_error)? {
        let name = match field.content_disposition().and_then(|cd| cd.get_name()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if name == "file" {
            if file.is_some() {
                return Err(ServiceError::ValidationError(
                    "only one photo file is allowed".to_string(),
                ));
            }
            let filename = field
                .content_disposition()
                .and_then(|cd| cd.get_filename())
                .unwrap_or("")
                .to_string();
            let content_type = field.content_type().map(|mime| mime.to_string());
            let mut bytes: Vec<u8> = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(multipart_error)? {
                if bytes.len() + chunk.len() > MAX_PHOTO_BYTES {
                    return Err(ServiceError::ValidationError(format!(
                        "photo must be at most {} bytes (2 MiB)",
                        MAX_PHOTO_BYTES
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            file = Some(UploadedFile {
                filename,
                content_type,
                bytes,
            });
        } else {
            let mut value: Vec<u8> = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(multipart_error)? {
                if value.len() + chunk.len() > MAX_TEXT_FIELD_BYTES {
                    return Err(ServiceError::ValidationError(format!(
                        "form field {name} is too large"
                    )));
                }
                value.extend_from_slice(&chunk);
            }
            let text = String::from_utf8(value).map_err(|_| {
                ServiceError::ValidationError(format!("form field {name} is not valid UTF-8"))
            })?;
            fields.insert(name, text);
        }
    }

    Ok((fields, file))
}

fn multipart_error(err: actix_multipart::MultipartError) -> ServiceError {
    ServiceError::BadRequest(format!("invalid multipart form: {err}"))
}
