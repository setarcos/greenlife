//! 鸟类调查「重要记录」的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/bird-records` 下，写接口挂在 `/staff/bird-records`
//! （STAFF 或 ADMIN），与 taxonomy / maintenance-logs 的分法一致。
//! 所有数据库访问都走 `blocking_db`，不在 worker 线程上同步查询。

use crate::bird_models::*;
use crate::config::UPLOAD_URL_PREFIX;
use crate::db::{blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::gpx::{self, GpxWaypoint};
use crate::photo_models::public_url;
use crate::schema::{bird_records, bird_survey_records, bird_surveys, species_photos, taxa};
use crate::taxonomy_handlers::{descendant_taxon_ids, like_pattern, page, Paged};
use crate::taxonomy_models::TaxonomyRank;
use crate::upload::read_multipart;
use actix_multipart::Multipart;
use actix_web::{delete, get, patch, post, web, HttpResponse};
use chrono::NaiveDate;
use diesel::pg::Pg;
use diesel::prelude::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
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

// ==========================================================================
// 鸟调记录（bird_surveys / bird_survey_records）
// ==========================================================================

/// GPX 文件的上限。示例文件 17 KB，4 MiB 已经很宽松了。
const MAX_GPX_BYTES: usize = 4 * 1024 * 1024;

/// 列出鸟调，按日期从新到旧。
///
/// `species_count` 由明细现算（同一鸟种的多条观察只算一种），不落库：
/// 明细一改就自动跟着变，不会出现表头和明细对不上。
#[get("")]
async fn list_bird_surveys(
    pool: web::Data<DbPool>,
    query: web::Query<BirdSurveyQuery>,
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
        let total = bird_surveys_query(pattern.clone(), from, to)
            .count()
            .get_result::<i64>(conn)?;
        let surveys = bird_surveys_query(pattern, from, to)
            .order((
                bird_surveys::survey_date.desc(),
                bird_surveys::created_at.desc(),
                bird_surveys::id.desc(),
            ))
            .limit(limit)
            .offset(offset)
            .select(BirdSurvey::as_select())
            .load::<BirdSurvey>(conn)?;
        let items = with_species_counts(conn, surveys)?;
        Ok(Paged { items, total })
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 鸟调的筛选条件。取总数和取当前页都走它。
///
/// `q` 按**鸟种**搜：命中该次调查明细里任一鸟种的中文名 / 学名。
fn bird_surveys_query(
    pattern: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> bird_surveys::BoxedQuery<'static, Pg> {
    let mut q = bird_surveys::table.into_boxed();
    if let Some(pattern) = pattern {
        q = q.filter(diesel::dsl::exists(
            bird_survey_records::table
                .filter(bird_survey_records::survey_id.eq(bird_surveys::id))
                .filter(
                    bird_survey_records::chinese_name
                        .ilike(pattern.clone())
                        .or(bird_survey_records::scientific_name
                            .ilike(pattern)
                            .assume_not_null()),
                )
                .select(bird_survey_records::id),
        ));
    }
    if let Some(from) = from {
        q = q.filter(bird_surveys::survey_date.ge(from));
    }
    if let Some(to) = to {
        q = q.filter(bird_surveys::survey_date.le(to));
    }
    q
}

/// 单次鸟调：表头 + 全部明细。
#[get("/{survey_id}")]
async fn get_bird_survey(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let survey_id = path.into_inner();
    let detail = blocking_db(pool, move |conn| {
        let survey = find_bird_survey(conn, survey_id)?;
        let records = load_survey_records(conn, survey_id)?;
        let survey = with_species_counts(conn, vec![survey])?.remove(0);
        Ok(BirdSurveyDetailResponse { survey, records })
    })
    .await?;

    Ok(HttpResponse::Ok().json(detail))
}

/// 从 GPX 导入一次鸟调。
///
/// 表单字段：`file`（GPX）+ `leader`（必填）+ `survey_date`（可选，缺省取最早
/// waypoint 的日期）+ `weather` + `note`。
///
/// 明细里能对上鸟纲（Aves）中文名的记录关联 `taxon_id`；对不上的（旧名 /
/// 名录里没有的名字）保留原名，`taxon_id` 为空，界面上标「未匹配」。
#[post("/import")]
async fn import_bird_survey(
    pool: web::Data<DbPool>,
    payload: Multipart,
) -> Result<HttpResponse, ServiceError> {
    let (fields, file) = read_multipart(payload, MAX_GPX_BYTES, "GPX").await?;
    let file = file
        .ok_or_else(|| ServiceError::ValidationError("GPX file is required".to_string()))?;

    let leader = match fields
        .get("leader")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        Some(leader) => leader.to_string(),
        None => {
            return Err(ServiceError::ValidationError(
                "leader is required".to_string(),
            ))
        }
    };
    if leader.chars().count() > SURVEY_LEADER_MAX_CHARS {
        return Err(ServiceError::ValidationError(format!(
            "leader must be at most {SURVEY_LEADER_MAX_CHARS} characters"
        )));
    }
    let weather = clean_optional(fields.get("weather"));
    if let Some(weather) = &weather {
        if weather.chars().count() > SURVEY_WEATHER_MAX_CHARS {
            return Err(ServiceError::ValidationError(format!(
                "weather must be at most {SURVEY_WEATHER_MAX_CHARS} characters"
            )));
        }
    }
    let note = clean_optional(fields.get("note"));
    // 只保留原始文件名，不落盘（见迁移里的说明）。
    let source = clean_optional(Some(&file.filename))
        .map(|name| name.chars().take(SURVEY_SOURCE_MAX_CHARS).collect());

    let xml = String::from_utf8(file.bytes)
        .map_err(|_| ServiceError::ValidationError("GPX must be UTF-8 text".to_string()))?;
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(&xml);
    let waypoints = gpx::parse_waypoints(xml)
        .map_err(|e| ServiceError::ValidationError(format!("GPX 解析失败：{e}")))?;
    if waypoints.is_empty() {
        return Err(ServiceError::ValidationError(
            "GPX 里没有任何 waypoint".to_string(),
        ));
    }

    let requested_date = match fields
        .get("survey_date")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        Some(text) => Some(NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| {
            ServiceError::ValidationError("survey_date must be yyyy-mm-dd".to_string())
        })?),
        None => None,
    };
    let survey_date = requested_date
        .or_else(|| {
            waypoints
                .iter()
                .filter_map(|waypoint| waypoint.time)
                .map(|time| time.date())
                .min()
        })
        .ok_or_else(|| {
            ServiceError::ValidationError("survey_date is required (no time in the GPX)".to_string())
        })?;

    let survey_id = Uuid::new_v4();
    let new_survey = NewBirdSurvey {
        id: survey_id,
        survey_date,
        leader: Some(leader),
        weather,
        note,
        source,
    };

    let detail = blocking_db(pool, move |conn| {
        conn.transaction::<BirdSurveyDetailResponse, ServiceError, _>(|conn| {
            let name_map = bird_chinese_name_map(conn)?;
            let mut new_records = Vec::new();
            for waypoint in &waypoints {
                let Some(name) = waypoint
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    continue;
                };
                let (taxon_id, scientific_name) = match name_map.get(name) {
                    Some((taxon_id, scientific_name)) => {
                        (Some(*taxon_id), Some(scientific_name.clone()))
                    }
                    None => (None, None),
                };
                new_records.push(NewBirdSurveyRecord {
                    id: Uuid::new_v4(),
                    survey_id,
                    taxon_id,
                    scientific_name,
                    chinese_name: name.to_string(),
                    quantity: waypoint.quantity(),
                    observed_at: waypoint.time,
                    latitude: waypoint.latitude,
                    longitude: waypoint.longitude,
                    note: waypoint_note(waypoint),
                });
            }
            if new_records.is_empty() {
                return Err(ServiceError::ValidationError(
                    "GPX 里没有任何带 <name> 的 waypoint".to_string(),
                ));
            }

            diesel::insert_into(bird_surveys::table)
                .values(&new_survey)
                .execute(conn)?;
            diesel::insert_into(bird_survey_records::table)
                .values(&new_records)
                .execute(conn)?;

            let survey = find_bird_survey(conn, survey_id)?;
            let records = load_survey_records(conn, survey_id)?;
            let survey = with_species_counts(conn, vec![survey])?.remove(0);
            Ok(BirdSurveyDetailResponse { survey, records })
        })
    })
    .await?;

    Ok(HttpResponse::Created().json(detail))
}

/// 手工新建一次鸟调（只有表头，明细另外逐条加）。
#[post("")]
async fn create_bird_survey(
    pool: web::Data<DbPool>,
    body: web::Json<CreateBirdSurveyDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let survey_id = Uuid::new_v4();
    let new = NewBirdSurvey {
        id: survey_id,
        survey_date: dto.survey_date,
        leader: dto.leader,
        weather: dto.weather,
        note: dto.note,
        source: dto.source,
    };

    let created = blocking_db(pool, move |conn| {
        diesel::insert_into(bird_surveys::table)
            .values(&new)
            .execute(conn)?;
        let survey = find_bird_survey(conn, survey_id)?;
        Ok(with_species_counts(conn, vec![survey])?.remove(0))
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/{survey_id}")]
async fn update_bird_survey(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateBirdSurveyDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let survey_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = find_bird_survey(conn, survey_id)?;

        // 三态合并：None=不改，Some(None)=置 NULL，Some(Some(v))=改成 v。
        let survey_date = dto.survey_date.unwrap_or(current.survey_date);
        let leader = match dto.leader {
            None => current.leader,
            Some(value) => value,
        };
        let weather = match dto.weather {
            None => current.weather,
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

        let survey = diesel::update(bird_surveys::table.find(survey_id))
            .set((
                bird_surveys::survey_date.eq(survey_date),
                bird_surveys::leader.eq(&leader),
                bird_surveys::weather.eq(&weather),
                bird_surveys::note.eq(&note),
                bird_surveys::source.eq(&source),
                bird_surveys::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(BirdSurvey::as_select())
            .get_result::<BirdSurvey>(conn)
            .map_err(ServiceError::from)?;
        Ok(with_species_counts(conn, vec![survey])?.remove(0))
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

/// 删除一次鸟调，明细由外键 `ON DELETE CASCADE` 一起删掉。
#[delete("/{survey_id}")]
async fn delete_bird_survey(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let survey_id = path.into_inner();

    blocking_db(pool, move |conn| {
        let deleted = diesel::delete(bird_surveys::table.find(survey_id))
            .execute(conn)
            .map_err(ServiceError::from)?;
        if deleted == 0 {
            return Err(ServiceError::NotFound(format!(
                "Bird survey {survey_id} not found"
            )));
        }
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Bird survey deleted successfully",
        "id": survey_id,
    })))
}

/// 给一次鸟调加一条鸟种明细。
#[post("/{survey_id}/records")]
async fn create_survey_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<SurveyRecordDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let survey_id = path.into_inner();
    let dto = body.into_inner();

    let created = blocking_db(pool, move |conn| {
        find_bird_survey(conn, survey_id)?;
        let (taxon_id, scientific_name, chinese_name) = resolve_record_names(conn, &dto)?;
        let id = Uuid::new_v4();
        let new = NewBirdSurveyRecord {
            id,
            survey_id,
            taxon_id,
            scientific_name,
            chinese_name,
            quantity: dto.quantity,
            observed_at: dto.observed_at,
            latitude: dto.latitude,
            longitude: dto.longitude,
            note: dto.note,
        };
        diesel::insert_into(bird_survey_records::table)
            .values(&new)
            .execute(conn)?;
        find_survey_record(conn, id)
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

/// 修改一条鸟种明细（整体替换）。
#[patch("/{survey_id}/records/{record_id}")]
async fn update_survey_record(
    pool: web::Data<DbPool>,
    path: web::Path<(Uuid, Uuid)>,
    body: web::Json<SurveyRecordDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let (survey_id, record_id) = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = find_survey_record(conn, record_id)?;
        if current.survey_id != survey_id {
            return Err(ServiceError::NotFound(format!(
                "Bird survey record {record_id} not found in survey {survey_id}"
            )));
        }
        let (taxon_id, scientific_name, chinese_name) = resolve_record_names(conn, &dto)?;

        diesel::update(bird_survey_records::table.find(record_id))
            .set((
                bird_survey_records::taxon_id.eq(taxon_id),
                bird_survey_records::scientific_name.eq(&scientific_name),
                bird_survey_records::chinese_name.eq(&chinese_name),
                bird_survey_records::quantity.eq(dto.quantity),
                bird_survey_records::observed_at.eq(dto.observed_at),
                bird_survey_records::latitude.eq(dto.latitude),
                bird_survey_records::longitude.eq(dto.longitude),
                bird_survey_records::note.eq(&dto.note),
                bird_survey_records::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(BirdSurveyRecord::as_select())
            .get_result::<BirdSurveyRecord>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/{survey_id}/records/{record_id}")]
async fn delete_survey_record(
    pool: web::Data<DbPool>,
    path: web::Path<(Uuid, Uuid)>,
) -> Result<HttpResponse, ServiceError> {
    let (survey_id, record_id) = path.into_inner();

    blocking_db(pool, move |conn| {
        let current = find_survey_record(conn, record_id)?;
        if current.survey_id != survey_id {
            return Err(ServiceError::NotFound(format!(
                "Bird survey record {record_id} not found in survey {survey_id}"
            )));
        }
        diesel::delete(bird_survey_records::table.find(record_id)).execute(conn)?;
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Bird survey record deleted successfully",
        "id": record_id,
    })))
}

// --- 鸟调辅助 ---

fn find_bird_survey(conn: &mut DbConn, survey_id: Uuid) -> Result<BirdSurvey, ServiceError> {
    bird_surveys::table
        .find(survey_id)
        .select(BirdSurvey::as_select())
        .first::<BirdSurvey>(conn)
        .optional()?
        .ok_or_else(|| ServiceError::NotFound(format!("Bird survey {survey_id} not found")))
}

fn find_survey_record(
    conn: &mut DbConn,
    record_id: Uuid,
) -> Result<BirdSurveyRecord, ServiceError> {
    bird_survey_records::table
        .find(record_id)
        .select(BirdSurveyRecord::as_select())
        .first::<BirdSurveyRecord>(conn)
        .optional()?
        .ok_or_else(|| {
            ServiceError::NotFound(format!("Bird survey record {record_id} not found"))
        })
}

/// 一次鸟调的全部明细：先按观察时间，再按鸟种名，保证顺序稳定。
fn load_survey_records(
    conn: &mut DbConn,
    survey_id: Uuid,
) -> Result<Vec<BirdSurveyRecord>, ServiceError> {
    bird_survey_records::table
        .filter(bird_survey_records::survey_id.eq(survey_id))
        .order((
            bird_survey_records::observed_at.asc().nulls_last(),
            bird_survey_records::chinese_name.asc(),
            bird_survey_records::id.asc(),
        ))
        .select(BirdSurveyRecord::as_select())
        .load::<BirdSurveyRecord>(conn)
        .map_err(ServiceError::from)
}

/// 给一批鸟调补上「鸟种数」。同一鸟种的多条观察只算一种：
/// 关联了分类的按 `taxon_id` 去重，未匹配的退化成按中文名去重。
fn with_species_counts(
    conn: &mut DbConn,
    surveys: Vec<BirdSurvey>,
) -> Result<Vec<BirdSurveyResponse>, ServiceError> {
    let ids: Vec<Uuid> = surveys.iter().map(|survey| survey.id).collect();
    let mut species: HashMap<Uuid, HashSet<String>> = HashMap::new();
    if !ids.is_empty() {
        let rows: Vec<(Uuid, Option<Uuid>, String)> = bird_survey_records::table
            .filter(bird_survey_records::survey_id.eq_any(&ids))
            .select((
                bird_survey_records::survey_id,
                bird_survey_records::taxon_id,
                bird_survey_records::chinese_name,
            ))
            .load::<(Uuid, Option<Uuid>, String)>(conn)?;
        for (survey_id, taxon_id, chinese_name) in rows {
            let key = match taxon_id {
                Some(taxon_id) => taxon_id.to_string(),
                None => format!("name:{chinese_name}"),
            };
            species.entry(survey_id).or_default().insert(key);
        }
    }

    Ok(surveys
        .into_iter()
        .map(|survey| {
            let species_count = species.get(&survey.id).map(HashSet::len).unwrap_or(0) as i64;
            BirdSurveyResponse {
                survey,
                species_count,
            }
        })
        .collect())
}

/// 鸟纲（Aves）下「中文名 → (节点 id, 学名)」的映射，供 GPX 导入匹配鸟种。
///
/// 一次查完整棵子树再在内存里比对，而不是每个 waypoint 查一次库：
/// 鸟纲只有几百个节点，98 个 waypoint 也就一次查询。
fn bird_chinese_name_map(
    conn: &mut DbConn,
) -> Result<HashMap<String, (Uuid, String)>, ServiceError> {
    let bird_class_id = taxa::table
        .filter(taxa::rank.eq(TaxonomyRank::Class))
        .filter(taxa::scientific_name.eq("Aves"))
        .select(taxa::id)
        .into_boxed()
        .first::<Uuid>(conn)
        .optional()?;
    let Some(root) = bird_class_id else {
        return Ok(HashMap::new());
    };

    let ids = descendant_taxon_ids(conn, root)?;
    let rows: Vec<(Uuid, String, String)> = taxa::table
        .filter(taxa::id.eq_any(&ids))
        .filter(taxa::chinese_name.is_not_null())
        .select((
            taxa::id,
            taxa::scientific_name,
            taxa::chinese_name.assume_not_null(),
        ))
        .load::<(Uuid, String, String)>(conn)?;

    let mut map = HashMap::new();
    for (id, scientific_name, chinese_name) in rows {
        // 中文名在鸟纲内基本唯一；真撞名时保留先到的。
        map.entry(chinese_name).or_insert((id, scientific_name));
    }
    Ok(map)
}

/// 明细的鸟种名：选了鸟种就以后端从分类树带出的名字为准（不信任客户端传的）；
/// 没选鸟种时用客户端填的中文名（未匹配）。
fn resolve_record_names(
    conn: &mut DbConn,
    dto: &SurveyRecordDto,
) -> Result<(Option<Uuid>, Option<String>, String), ServiceError> {
    match dto.taxon_id {
        Some(taxon_id) => {
            let taxon = taxa::table
                .find(taxon_id)
                .select((taxa::scientific_name, taxa::chinese_name))
                .first::<(String, Option<String>)>(conn)
                .optional()?
                .ok_or_else(|| {
                    ServiceError::ValidationError(format!("taxon {taxon_id} not found"))
                })?;
            let chinese_name = taxon.1.unwrap_or_else(|| taxon.0.clone());
            Ok((Some(taxon_id), Some(taxon.0), chinese_name))
        }
        None => Ok((
            None,
            dto.scientific_name.clone(),
            dto.chinese_name.clone().unwrap_or_default().trim().to_string(),
        )),
    }
}

/// 空串 / 全空白统一转成 `None`。
fn clean_optional(value: Option<&String>) -> Option<String> {
    value
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// GPX 的 `desc`（数量）与 `gpxx:StreetAddress`（备注）合成一条备注。
/// `desc` 是数字时已经进了 `quantity`，不再重复塞进备注。
fn waypoint_note(waypoint: &GpxWaypoint) -> Option<String> {
    let remark = waypoint
        .remark
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let leftover = if waypoint.quantity().is_none() {
        waypoint
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
    } else {
        None
    };

    match (remark, leftover) {
        (Some(remark), Some(leftover)) => Some(format!("{leftover}；{remark}")),
        (Some(remark), None) => Some(remark.to_string()),
        (None, Some(leftover)) => Some(leftover.to_string()),
        (None, None) => None,
    }
}
