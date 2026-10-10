//! 鸟类调查「重要记录」的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/bird-records` 下，写接口挂在 `/staff/bird-records`
//! （STAFF 或 ADMIN），与 taxonomy / maintenance-logs 的分法一致。
//! 所有数据库访问都走 `blocking_db`，不在 worker 线程上同步查询。

use crate::bird_models::*;
use crate::db::{blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::schema::{bird_records, taxa};
use crate::taxonomy_handlers::{like_pattern, page};
use actix_web::{delete, get, patch, post, web, HttpResponse};
use diesel::prelude::*;
use serde_json::json;
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
        q.order((
            bird_records::scientific_name.asc(),
            bird_records::observed_at.asc().nulls_last(),
            bird_records::id.asc(),
        ))
        .limit(limit)
        .offset(offset)
        .select(BirdRecord::as_select())
        .load::<BirdRecord>(conn)
        .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 单条重要记录。
#[get("/{record_id}")]
async fn get_bird_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let record_id = path.into_inner();
    let record = blocking_db(pool, move |conn| {
        bird_records::table
            .find(record_id)
            .select(BirdRecord::as_select())
            .first::<BirdRecord>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Bird record {record_id} not found")))
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
    };

    let created = blocking_db(pool, move |conn| {
        ensure_taxon_exists(conn, new.taxon_id)?;
        diesel::insert_into(bird_records::table)
            .values(&new)
            .execute(conn)?;
        bird_records::table
            .find(new.id)
            .select(BirdRecord::as_select())
            .first::<BirdRecord>(conn)
            .map_err(ServiceError::from)
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
        let current = bird_records::table
            .find(record_id)
            .select(BirdRecord::as_select())
            .first::<BirdRecord>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Bird record {record_id} not found")))?;

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

        ensure_taxon_exists(conn, taxon_id)?;

        diesel::update(bird_records::table.find(record_id))
            .set((
                bird_records::taxon_id.eq(taxon_id),
                bird_records::scientific_name.eq(&scientific_name),
                bird_records::chinese_name.eq(&chinese_name),
                bird_records::observer.eq(&observer),
                bird_records::observed_at.eq(&observed_at),
                bird_records::location.eq(&location),
                bird_records::note.eq(&note),
                bird_records::source.eq(&source),
                bird_records::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(BirdRecord::as_select())
            .get_result::<BirdRecord>(conn)
            .map_err(ServiceError::from)
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
