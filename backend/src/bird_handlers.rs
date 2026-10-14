//! 鸟类调查「重要记录」的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/bird-records` 下，写接口挂在 `/staff/bird-records`
//! （STAFF 或 ADMIN），与 taxonomy / maintenance-logs 的分法一致。
//! 所有数据库访问都走 `blocking_db`，不在 worker 线程上同步查询。

use crate::bird_models::*;
use crate::config::UPLOAD_URL_PREFIX;
use crate::db::{blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::photo_models::public_url;
use crate::schema::{bird_records, species_photos, taxa};
use crate::taxonomy_handlers::{like_pattern, page, Paged};
use actix_web::{delete, get, patch, post, web, HttpResponse};
use chrono::NaiveDate;
use diesel::pg::Pg;
use diesel::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

// ==========================================================================
// 读接口（公开）
// ==========================================================================

/// 扁平列出重要记录，可按关键字与时间范围筛选。
///
/// 时间范围是「区间求交集」而不是「包含在区间内」：`observed_at` 里的日期
/// 可能只是一个月或一年（2023年5月底 / 2007年），把它当成一个点去比较
/// 会漏掉这些记录，也不能如实回答「2007 年有没有记录」这类问题。
///
/// 排序：先按学名归组（同一物种的记录挨在一起），组内按时间文本升序。
/// `observed_at` 是原文（2025年3月19日 / 2023年5月底），不是严格日期，
/// 所以这个顺序只是「大致按时间」，和不写日期的记录一起排在末尾。
#[get("")]
async fn list_bird_records(
    pool: web::Data<DbPool>,
    query: web::Query<BirdRecordQuery>,
) -> Result<HttpResponse, ServiceError> {
    let query = query.into_inner();
    let (limit, offset) = page(query.limit, query.offset);
    let pattern = query.q.as_deref().map(like_pattern);
    let (from, to) = (query.from, query.to);
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(ServiceError::ValidationError(
                "from must not be later than to".to_string(),
            ));
        }
    }

    let items = blocking_db(pool, move |conn| {
        let total = bird_records_query(pattern.clone(), from, to)
            .count()
            .get_result::<i64>(conn)?;
        let items = bird_records_query(pattern, from, to)
            .order((
                bird_records::scientific_name.asc(),
                bird_records::observed_at.asc().nulls_last(),
                bird_records::id.asc(),
            ))
            .limit(limit)
            .offset(offset)
            .select(BirdRecord::as_select())
            .load::<BirdRecord>(conn)?;
        let items = with_photo_urls(conn, items)?;

        Ok(Paged { items, total })
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 重要记录的筛选条件。取总数和取当前页都走它。
fn bird_records_query(
    pattern: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> bird_records::BoxedQuery<'static, Pg> {
    let mut q = bird_records::table.into_boxed();
    if let Some(pattern) = pattern {
        q = q.filter(
            bird_records::scientific_name
                .ilike(pattern.clone())
                .or(bird_records::chinese_name
                    .ilike(pattern.clone())
                    .assume_not_null())
                .or(bird_records::observer
                    .ilike(pattern.clone())
                    .assume_not_null())
                .or(bird_records::location.ilike(pattern).assume_not_null()),
        );
    }
    // 两个比较都写出来，NULL（认不出日期的记录）会在任意一侧被过滤掉。
    if let Some(from) = from {
        q = q.filter(bird_records::observed_to.ge(from));
    }
    if let Some(to) = to {
        q = q.filter(bird_records::observed_from.le(to));
    }
    q
}

/// 单条重要记录。
#[get("/{record_id}")]
async fn get_bird_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let record_id = path.into_inner();
    let record = blocking_db(pool, move |conn| {
        let record = find_bird_record(conn, record_id)?;
        Ok(with_photo_urls(conn, vec![record])?.remove(0))
    })
    .await?;
    Ok(HttpResponse::Ok().json(record))
}

// ==========================================================================
// 写接口（STAFF / ADMIN）
// ==========================================================================

#[post("")]
async fn create_bird_record(
    pool: web::Data<DbPool>,
    body: web::Json<CreateBirdRecordDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let new = NewBirdRecord {
        id: Uuid::new_v4(),
        taxon_id: dto.taxon_id,
        scientific_name: dto.scientific_name,
        chinese_name: dto.chinese_name,
        observer: dto.observer,
        observed_at: dto.observed_at,
        location: dto.location,
        note: dto.note,
        source: dto.source,
        photo_id: dto.photo_id,
    };

    let created = blocking_db(pool, move |conn| {
        ensure_taxon_exists(conn, new.taxon_id)?;
        if let Some(photo_id) = new.photo_id {
            ensure_photo_matches_taxon(conn, photo_id, new.taxon_id)?;
        }
        diesel::insert_into(bird_records::table)
            .values(&new)
            .execute(conn)?;
        if let Some(photo_id) = new.photo_id {
            mark_photo_important(conn, photo_id)?;
        }
        let record = find_bird_record(conn, new.id)?;
        Ok(with_photo_urls(conn, vec![record])?.remove(0))
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/{record_id}")]
async fn update_bird_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateBirdRecordDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let record_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = find_bird_record(conn, record_id)?;

        // 三态合并：None=不改，Some(None)=置 NULL，Some(Some(v))=改成 v。
        let taxon_id = dto.taxon_id.unwrap_or(current.taxon_id);
        let scientific_name = dto.scientific_name.unwrap_or(current.scientific_name);
        let chinese_name = match dto.chinese_name {
            None => current.chinese_name,
            Some(value) => value,
        };
        let observer = match dto.observer {
            None => current.observer,
            Some(value) => value,
        };
        let observed_at = match dto.observed_at {
            None => current.observed_at,
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
        let source = match dto.source {
            None => current.source,
            Some(value) => value,
        };
        let photo_id = match dto.photo_id {
            None => current.photo_id,
            Some(value) => value,
        };

        ensure_taxon_exists(conn, taxon_id)?;
        if let Some(photo_id) = photo_id {
            ensure_photo_matches_taxon(conn, photo_id, taxon_id)?;
        }

        let record = diesel::update(bird_records::table.find(record_id))
            .set((
                bird_records::taxon_id.eq(taxon_id),
                bird_records::scientific_name.eq(&scientific_name),
                bird_records::chinese_name.eq(&chinese_name),
                bird_records::observer.eq(&observer),
                bird_records::observed_at.eq(&observed_at),
                bird_records::location.eq(&location),
                bird_records::note.eq(&note),
                bird_records::source.eq(&source),
                bird_records::photo_id.eq(photo_id),
                bird_records::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(BirdRecord::as_select())
            .get_result::<BirdRecord>(conn)
            .map_err(ServiceError::from)?;
        if let Some(photo_id) = photo_id {
            mark_photo_important(conn, photo_id)?;
        }
        Ok(with_photo_urls(conn, vec![record])?.remove(0))
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/{record_id}")]
async fn delete_bird_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let record_id = path.into_inner();

    blocking_db(pool, move |conn| {
        let deleted = diesel::delete(bird_records::table.find(record_id))
            .execute(conn)
            .map_err(ServiceError::from)?;
        if deleted == 0 {
            return Err(ServiceError::NotFound(format!(
                "Bird record {record_id} not found"
            )));
        }
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Bird record deleted successfully",
        "id": record_id,
    })))
}

// ==========================================================================
// 公共校验
// ==========================================================================

fn find_bird_record(conn: &mut DbConn, record_id: Uuid) -> Result<BirdRecord, ServiceError> {
    bird_records::table
        .find(record_id)
        .select(BirdRecord::as_select())
        .first::<BirdRecord>(conn)
        .optional()?
        .ok_or_else(|| ServiceError::NotFound(format!("Bird record {record_id} not found")))
}

/// 记录必须挂在一个真实的分类节点上（外键约束的兜底，报错更可读）。
fn ensure_taxon_exists(conn: &mut DbConn, taxon_id: Uuid) -> Result<(), ServiceError> {
    let exists = taxa::table
        .find(taxon_id)
        .select(taxa::id)
        .first::<Uuid>(conn)
        .optional()?;
    if exists.is_none() {
        return Err(ServiceError::ValidationError(format!(
            "taxon {taxon_id} not found"
        )));
    }
    Ok(())
}

/// 记录挂的照片必须存在，并且属于同一个物种（照片是从该物种的照片库里挑的）。
/// 跨物种引用数据库里拦不住，只能在这里查一次。
fn ensure_photo_matches_taxon(
    conn: &mut DbConn,
    photo_id: Uuid,
    taxon_id: Uuid,
) -> Result<(), ServiceError> {
    let owner = species_photos::table
        .find(photo_id)
        .select(species_photos::taxon_id)
        .first::<Uuid>(conn)
        .optional()?;
    match owner {
        None => Err(ServiceError::ValidationError(format!(
            "photo {photo_id} not found"
        ))),
        Some(owner) if owner != taxon_id => Err(ServiceError::ValidationError(format!(
            "photo {photo_id} belongs to taxon {owner}, not taxon {taxon_id}"
        ))),
        Some(_) => Ok(()),
    }
}

/// 记录挂的照片自动打上「重要记录」标记（`species_photos.is_important`）。
///
/// 只「挂上时置 true」，解绑不清除：同一个标记也可能是用户自己勾的，而且一张
/// 照片可以被多条记录引用，后端分不出是谁设的，清掉会丢用户的意图。
fn mark_photo_important(conn: &mut DbConn, photo_id: Uuid) -> Result<(), ServiceError> {
    diesel::update(species_photos::table.find(photo_id))
        .set(species_photos::is_important.eq(true))
        .execute(conn)?;
    Ok(())
}

/// 给记录补上照片的公开 URL。
///
/// 照片路径单独查一次再在内存里拼，比在列表查询里 LEFT JOIN 更好写：
/// 分页 / 筛选 / 计数那套 `BoxedQuery` 不用为 JOIN 再写一份。
fn with_photo_urls(
    conn: &mut DbConn,
    records: Vec<BirdRecord>,
) -> Result<Vec<BirdRecordResponse>, ServiceError> {
    let photo_ids: Vec<Uuid> = records.iter().filter_map(|record| record.photo_id).collect();
    let mut paths: HashMap<Uuid, String> = HashMap::new();
    if !photo_ids.is_empty() {
        paths = species_photos::table
            .filter(species_photos::id.eq_any(&photo_ids))
            .select((species_photos::id, species_photos::file_path))
            .load::<(Uuid, String)>(conn)?
            .into_iter()
            .collect();
    }

    Ok(records
        .into_iter()
        .map(|record| {
            let photo_url = record
                .photo_id
                .and_then(|photo_id| paths.get(&photo_id))
                .map(|file_path| public_url(UPLOAD_URL_PREFIX, file_path));
            BirdRecordResponse { record, photo_url }
        })
        .collect())
}
