//! 维护日志的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/maintenance-logs` 下，写接口挂在
//! `/staff/maintenance-logs`（STAFF 或 ADMIN）。所有数据库访问都走
//! `blocking_db`，与 `taxonomy_handlers.rs` 一致。

use crate::db::{blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::maintenance_models::*;
use crate::schema::{maintenance_logs, species_lists};
use crate::taxonomy_handlers::{like_pattern, page, Paged};
use actix_web::{delete, get, patch, post, web, HttpResponse};
use diesel::pg::Pg;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

// ==========================================================================
// 读接口（公开）
// ==========================================================================

/// 扁平列出维护日志，可按名录过滤 / 按修订说明与修订人搜索。
///
/// 排序：有「修订笔记」的排前面，按笔记倒序（新的在前）；笔记为空的行垫底。
/// 列是文本，所以是字符串排序；导入脚本已把日期归一成 yyyy-mm-dd / yyyy-mm，
/// 使「全部」列表接近时间倒序（不能识别的文字按字符串位置参与，见脚本注释）。
#[get("")]
async fn list_maintenance_logs(
    pool: web::Data<DbPool>,
    query: web::Query<MaintenanceLogQuery>,
) -> Result<HttpResponse, ServiceError> {
    let query = query.into_inner();
    let (limit, offset) = page(query.limit, query.offset);
    let list_id = query.list_id;
    let pattern = query.q.as_deref().map(like_pattern);

    let items = blocking_db(pool, move |conn| {
        let total = maintenance_logs_query(list_id, pattern.clone())
            .count()
            .get_result::<i64>(conn)?;
        let items = maintenance_logs_query(list_id, pattern)
            .order((
                // is_null() 升序 = 非空在前（PostgreSQL 里 false < true）。
                maintenance_logs::entry_date.is_null().asc(),
                maintenance_logs::entry_date.desc(),
                maintenance_logs::id.asc(),
            ))
            .limit(limit)
            .offset(offset)
            .select(MaintenanceLog::as_select())
            .load::<MaintenanceLog>(conn)?;

        Ok(Paged { items, total })
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 维护日志的筛选条件。取总数和取当前页都走它。
fn maintenance_logs_query(
    list_id: Option<Uuid>,
    pattern: Option<String>,
) -> maintenance_logs::BoxedQuery<'static, Pg> {
    let mut q = maintenance_logs::table.into_boxed();
    if let Some(list_id) = list_id {
        q = q.filter(maintenance_logs::list_id.eq(list_id));
    }
    if let Some(pattern) = pattern {
        q = q.filter(
            maintenance_logs::summary
                .ilike(pattern.clone())
                .or(maintenance_logs::author.ilike(pattern).assume_not_null()),
        );
    }
    q
}

/// 单条维护日志。
#[get("/{log_id}")]
async fn get_maintenance_log(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let log_id = path.into_inner();
    let log = blocking_db(pool, move |conn| {
        maintenance_logs::table
            .find(log_id)
            .select(MaintenanceLog::as_select())
            .first::<MaintenanceLog>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Maintenance log {log_id} not found")))
    })
    .await?;
    Ok(HttpResponse::Ok().json(log))
}

// ==========================================================================
// 写接口（STAFF / ADMIN）
// ==========================================================================

#[post("")]
async fn create_maintenance_log(
    pool: web::Data<DbPool>,
    body: web::Json<CreateMaintenanceLogDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let new = NewMaintenanceLog {
        id: Uuid::new_v4(),
        list_id: dto.list_id,
        entry_date: dto.entry_date,
        author: dto.author,
        summary: dto.summary,
        species_appendix: dto.species_appendix,
    };

    let created = blocking_db(pool, move |conn| {
        ensure_list_exists(conn, new.list_id)?;
        diesel::insert_into(maintenance_logs::table)
            .values(&new)
            .execute(conn)?;
        maintenance_logs::table
            .find(new.id)
            .select(MaintenanceLog::as_select())
            .first::<MaintenanceLog>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/{log_id}")]
async fn update_maintenance_log(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateMaintenanceLogDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let log_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = maintenance_logs::table
            .find(log_id)
            .select(MaintenanceLog::as_select())
            .first::<MaintenanceLog>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Maintenance log {log_id} not found")))?;

        // 三态合并：None=不改，Some(None)=置 NULL，Some(Some(v))=改成 v。
        let list_id = match dto.list_id {
            None => current.list_id,
            Some(value) => value,
        };
        let entry_date = match dto.entry_date {
            None => current.entry_date,
            Some(value) => value,
        };
        let author = match dto.author {
            None => current.author,
            Some(value) => value,
        };
        let summary = dto.summary.unwrap_or(current.summary);
        let species_appendix = match dto.species_appendix {
            None => current.species_appendix,
            Some(value) => value,
        };

        ensure_list_exists(conn, list_id)?;

        diesel::update(maintenance_logs::table.find(log_id))
            .set((
                maintenance_logs::list_id.eq(list_id),
                maintenance_logs::entry_date.eq(&entry_date),
                maintenance_logs::author.eq(&author),
                maintenance_logs::summary.eq(&summary),
                maintenance_logs::species_appendix.eq(&species_appendix),
                maintenance_logs::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(MaintenanceLog::as_select())
            .get_result::<MaintenanceLog>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/{log_id}")]
async fn delete_maintenance_log(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let log_id = path.into_inner();

    blocking_db(pool, move |conn| {
        let deleted = diesel::delete(maintenance_logs::table.find(log_id))
            .execute(conn)
            .map_err(ServiceError::from)?;
        if deleted == 0 {
            return Err(ServiceError::NotFound(format!(
                "Maintenance log {log_id} not found"
            )));
        }
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Maintenance log deleted successfully",
        "id": log_id,
    })))
}

// ==========================================================================
// 公共校验
// ==========================================================================

/// 日志可以不属于任何名录（全局日志），但写了 list_id 就必须真实存在。
fn ensure_list_exists(conn: &mut DbConn, list_id: Option<Uuid>) -> Result<(), ServiceError> {
    let Some(list_id) = list_id else {
        return Ok(());
    };
    let exists = species_lists::table
        .find(list_id)
        .select(species_lists::id)
        .first::<Uuid>(conn)
        .optional()?;
    if exists.is_none() {
        return Err(ServiceError::ValidationError(format!(
            "species list {list_id} not found"
        )));
    }
    Ok(())
}
