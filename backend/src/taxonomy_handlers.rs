//! 分类树与物种名录的 HTTP 处理器。
//!
//! 读接口挂在无鉴权的 `/taxonomy` scope 下，写接口挂在 `/staff/taxonomy`
//! （STAFF 或 ADMIN）。所有数据库访问都走 `blocking_db`，不在 worker 线程上同步查询。

use crate::db::{blocking_db, DbConn, DbPool};
use crate::errors::ServiceError;
use crate::schema::{species_lists, species_records, taxa};
use crate::taxonomy_models::*;
use actix_web::{delete, get, patch, post, web, HttpResponse};
use diesel::prelude::*;
use diesel::sql_types::Uuid as SqlUuid;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 200;
const MAX_LIMIT: i64 = 1000;

fn page(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        offset.unwrap_or(0).max(0),
    )
}

/// 把用户输入变成 `%...%` 的 LIKE 模式，并转义 `%` / `_` / `\`。
///
/// 不转义的话，搜 `100%` 会变成通配符查询。PostgreSQL 的 LIKE 默认转义符是
/// 反斜杠，而这里是绑定参数（不是字符串字面量），所以不需要额外的 ESCAPE 子句。
fn like_pattern(q: &str) -> String {
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

// ==========================================================================
// 读接口（公开）
// ==========================================================================

/// 列出所有阶元及其深度，供前端拼下拉框 / 排序。
#[get("/ranks")]
async fn list_ranks() -> HttpResponse {
    let items: Vec<RankInfo> = TaxonomyRank::ALL
        .into_iter()
        .map(|rank| RankInfo {
            rank,
            depth: rank.depth(),
        })
        .collect();
    HttpResponse::Ok().json(items)
}

/// 扁平列出分类节点，支持按阶元 / 父级 / 关键字过滤。
///
/// 「新增物种时高级门类都用下拉框选」就是靠 `rank` + `parent_id` 逐级拉候选：
/// `?rank=phylum` → `?rank=class&parent_id=<门>` → …
#[get("/taxa")]
async fn list_taxa(
    pool: web::Data<DbPool>,
    query: web::Query<TaxonQuery>,
) -> Result<HttpResponse, ServiceError> {
    let query = query.into_inner();
    let (limit, offset) = page(query.limit, query.offset);

    let parent_filter = match query.parent_id.as_deref() {
        None => None,
        Some("root") => Some(None),
        Some(other) => Some(Some(Uuid::parse_str(other).map_err(|_| {
            ServiceError::ValidationError(
                "parent_id must be a UUID or the literal \"root\"".to_string(),
            )
        })?)),
    };
    let rank = query.rank;
    let pattern = query.q.as_deref().map(like_pattern);

    let items = blocking_db(pool, move |conn| {
        let mut q = taxa::table.into_boxed();
        if let Some(rank) = rank {
            q = q.filter(taxa::rank.eq(rank));
        }
        match parent_filter {
            None => {}
            Some(None) => q = q.filter(taxa::parent_id.is_null()),
            Some(Some(id)) => q = q.filter(taxa::parent_id.eq(id)),
        }
        if let Some(pattern) = pattern {
            q = q.filter(
                taxa::scientific_name
                    .ilike(pattern.clone())
                    .or(taxa::chinese_name.ilike(pattern).assume_not_null()),
            );
        }
        // PostgreSQL 枚举按定义顺序排序，所以 rank 升序就是「界→种」。
        q.order((taxa::rank.asc(), taxa::scientific_name.asc()))
            .limit(limit)
            .offset(offset)
            .select(Taxon::as_select())
            .load::<Taxon>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

/// 单个分类节点：本身 + 从根到父节点的路径 + 直接子节点。
#[get("/taxa/{taxon_id}")]
async fn get_taxon(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let taxon_id = path.into_inner();

    let detail = blocking_db(pool, move |conn| {
        let taxon = taxa::table
            .find(taxon_id)
            .select(Taxon::as_select())
            .first::<Taxon>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Taxon {taxon_id} not found")))?;

        let children = taxa::table
            .filter(taxa::parent_id.eq(taxon_id))
            .order(taxa::scientific_name.asc())
            .select(Taxon::as_select())
            .load::<Taxon>(conn)?;

        // 父链。因为父级阶元严格更高，链一定终止，不可能成环。
        let mut ancestors = Vec::new();
        let mut cursor = taxon.parent_id;
        while let Some(parent_id) = cursor {
            let parent = taxa::table
                .find(parent_id)
                .select(Taxon::as_select())
                .first::<Taxon>(conn)?;
            cursor = parent.parent_id;
            ancestors.push(parent);
        }
        ancestors.reverse();

        Ok(TaxonDetail {
            taxon,
            path: ancestors,
            children,
        })
    })
    .await?;

    Ok(HttpResponse::Ok().json(detail))
}

/// 嵌套分类树。给 `root_id` 返回该子树，否则返回整片森林。
#[get("/tree")]
async fn get_tree(
    pool: web::Data<DbPool>,
    query: web::Query<TreeQuery>,
) -> Result<HttpResponse, ServiceError> {
    let root_id = query.root_id;
    let depth = query.depth;

    let forest = blocking_db(pool, move |conn| {
        let all = taxa::table
            .order((taxa::rank.asc(), taxa::scientific_name.asc()))
            .select(Taxon::as_select())
            .load::<Taxon>(conn)?;

        let mut nodes: HashMap<Uuid, Taxon> = HashMap::with_capacity(all.len());
        // 子节点按加载顺序入桶；`all` 已按 (rank, 学名) 排序，兄弟同阶元即按学名有序。
        let mut children: HashMap<Option<Uuid>, Vec<Uuid>> = HashMap::new();
        for taxon in all {
            children.entry(taxon.parent_id).or_default().push(taxon.id);
            nodes.insert(taxon.id, taxon);
        }

        let starts: Vec<Uuid> = match root_id {
            Some(id) => {
                if !nodes.contains_key(&id) {
                    return Err(ServiceError::NotFound(format!("Taxon {id} not found")));
                }
                vec![id]
            }
            None => children.get(&None).cloned().unwrap_or_default(),
        };

        Ok(starts
            .into_iter()
            .filter_map(|id| build_node(id, depth, &mut nodes, &children))
            .collect::<Vec<TaxonNode>>())
    })
    .await?;

    Ok(HttpResponse::Ok().json(forest))
}

/// 从 id → Taxon 的临时表里递归取出一个子树。
///
/// `nodes` 用 `remove` 而不是 `get`：既避免借用冲突，也顺带保证每个节点只出现一次。
fn build_node(
    id: Uuid,
    depth: Option<usize>,
    nodes: &mut HashMap<Uuid, Taxon>,
    children: &HashMap<Option<Uuid>, Vec<Uuid>>,
) -> Option<TaxonNode> {
    let taxon = nodes.remove(&id)?;
    let child_ids = match depth {
        Some(0) => Vec::new(),
        other => {
            let next = other.map(|d| d - 1);
            children
                .get(&Some(id))
                .map(|ids| {
                    ids.iter()
                        .filter_map(|child| build_node(*child, next, nodes, children))
                        .collect()
                })
                .unwrap_or_default()
        }
    };
    let mut node = TaxonNode::from(taxon);
    node.children = child_ids;
    Some(node)
}

#[get("/lists")]
async fn list_lists(pool: web::Data<DbPool>) -> Result<HttpResponse, ServiceError> {
    let items = blocking_db(pool, |conn| {
        species_lists::table
            // 领域顺序由 position 决定（脊椎 > 无脊椎 > 高等植物 > 大型真菌，
            // 见迁移 2026-10-10-075917）；新名录默认 100，按名字排在后面。
            .order((species_lists::position.asc(), species_lists::name.asc()))
            .select(SpeciesList::as_select())
            .load::<SpeciesList>(conn)
            .map_err(ServiceError::from)
    })
    .await?;
    Ok(HttpResponse::Ok().json(items))
}

#[get("/lists/{list_id}")]
async fn get_list(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let list_id = path.into_inner();

    let detail = blocking_db(pool, move |conn| {
        let list = species_lists::table
            .find(list_id)
            .select(SpeciesList::as_select())
            .first::<SpeciesList>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Species list {list_id} not found")))?;

        let record_count: i64 = species_records::table
            .filter(species_records::list_id.eq(list_id))
            .count()
            .get_result(conn)?;

        Ok(SpeciesListDetail {
            list,
            record_count,
        })
    })
    .await?;

    Ok(HttpResponse::Ok().json(detail))
}

/// 扁平列出名录记录。`taxon_id` + `descendants=true` 可拿到整个类群的记录。
#[get("/records")]
async fn list_records(
    pool: web::Data<DbPool>,
    query: web::Query<RecordQuery>,
) -> Result<HttpResponse, ServiceError> {
    let query = query.into_inner();
    let (limit, offset) = page(query.limit, query.offset);
    let list_id = query.list_id;
    let taxon_id = query.taxon_id;
    let descendants = query.descendants.unwrap_or(false);
    let pattern = query.q.as_deref().map(like_pattern);

    let items = blocking_db(pool, move |conn| {
        let mut q = species_records::table.into_boxed();
        if let Some(list_id) = list_id {
            q = q.filter(species_records::list_id.eq(list_id));
        }
        if let Some(taxon_id) = taxon_id {
            if descendants {
                let ids = descendant_taxon_ids(conn, taxon_id)?;
                q = q.filter(species_records::taxon_id.eq_any(ids));
            } else {
                q = q.filter(species_records::taxon_id.eq(taxon_id));
            }
        }
        if let Some(pattern) = pattern {
            q = q.filter(
                species_records::scientific_name
                    .ilike(pattern.clone())
                    .or(
                        species_records::chinese_name
                            .ilike(pattern)
                            .assume_not_null(),
                    ),
            );
        }
        q.order((
            species_records::scientific_name.asc(),
            species_records::id.asc(),
        ))
        .limit(limit)
        .offset(offset)
        .select(SpeciesRecord::as_select())
        .load::<SpeciesRecord>(conn)
        .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(items))
}

#[get("/records/{record_id}")]
async fn get_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let record_id = path.into_inner();
    let record = blocking_db(pool, move |conn| {
        species_records::table
            .find(record_id)
            .select(SpeciesRecord::as_select())
            .first::<SpeciesRecord>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Species record {record_id} not found")))
    })
    .await?;
    Ok(HttpResponse::Ok().json(record))
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// 分类节点的所有后代 id（含自身），递归 CTE 一次查完。
fn descendant_taxon_ids(conn: &mut DbConn, root: Uuid) -> Result<Vec<Uuid>, ServiceError> {
    diesel::sql_query(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT id FROM taxa WHERE id = $1 \
             UNION ALL \
             SELECT t.id FROM taxa t JOIN subtree s ON t.parent_id = s.id \
         ) SELECT id FROM subtree",
    )
    .bind::<SqlUuid, _>(root)
    .load::<IdRow>(conn)
    .map(|rows| rows.into_iter().map(|row| row.id).collect())
    .map_err(ServiceError::from)
}

// ==========================================================================
// 写接口（STAFF / ADMIN）
// ==========================================================================

#[post("/taxa")]
async fn create_taxon(
    pool: web::Data<DbPool>,
    body: web::Json<CreateTaxonDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let new = NewTaxon {
        id: Uuid::new_v4(),
        parent_id: dto.parent_id,
        rank: dto.rank,
        scientific_name: dto.scientific_name,
        chinese_name: dto.chinese_name,
    };

    let created = blocking_db(pool, move |conn| {
        check_parent(conn, new.parent_id, new.rank)?;
        check_sibling_unique(conn, new.parent_id, new.rank, &new.scientific_name, None)?;
        diesel::insert_into(taxa::table)
            .values(&new)
            .execute(conn)?;
        taxa::table
            .find(new.id)
            .select(Taxon::as_select())
            .first::<Taxon>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/taxa/{taxon_id}")]
async fn update_taxon(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateTaxonDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let taxon_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = taxa::table
            .find(taxon_id)
            .select(Taxon::as_select())
            .first::<Taxon>(conn)
            .optional()?
            .ok_or_else(|| ServiceError::NotFound(format!("Taxon {taxon_id} not found")))?;

        // 三态合并：None=不改，Some(None)=置 NULL，Some(Some(v))=改成 v。
        let parent_id = match dto.parent_id {
            None => current.parent_id,
            Some(value) => value,
        };
        let scientific_name = dto.scientific_name.unwrap_or(current.scientific_name);
        let chinese_name = match dto.chinese_name {
            None => current.chinese_name,
            Some(value) => value,
        };

        check_parent(conn, parent_id, current.rank)?;
        check_sibling_unique(
            conn,
            parent_id,
            current.rank,
            &scientific_name,
            Some(taxon_id),
        )?;

        diesel::update(taxa::table.find(taxon_id))
            .set((
                taxa::parent_id.eq(parent_id),
                taxa::scientific_name.eq(&scientific_name),
                taxa::chinese_name.eq(&chinese_name),
                taxa::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(Taxon::as_select())
            .get_result::<Taxon>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/taxa/{taxon_id}")]
async fn delete_taxon(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let taxon_id = path.into_inner();

    blocking_db(pool, move |conn| {
        let exists = taxa::table
            .find(taxon_id)
            .select(taxa::id)
            .first::<Uuid>(conn)
            .optional()?;
        if exists.is_none() {
            return Err(ServiceError::NotFound(format!(
                "Taxon {taxon_id} not found"
            )));
        }

        // 有子节点或记录时拒绝删除，避免把整棵子树连同名录记录一起弄丢。
        // 数据库的外键只是兜底（默认 NO ACTION），这里给出可读的 409。
        let child_count: i64 = taxa::table
            .filter(taxa::parent_id.eq(taxon_id))
            .count()
            .get_result(conn)?;
        if child_count > 0 {
            return Err(ServiceError::Conflict(format!(
                "taxon {taxon_id} still has {child_count} child taxa; delete or move them first"
            )));
        }
        let record_count: i64 = species_records::table
            .filter(species_records::taxon_id.eq(taxon_id))
            .count()
            .get_result(conn)?;
        if record_count > 0 {
            return Err(ServiceError::Conflict(format!(
                "taxon {taxon_id} is referenced by {record_count} species records"
            )));
        }

        diesel::delete(taxa::table.find(taxon_id))
            .execute(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Taxon deleted successfully",
        "id": taxon_id,
    })))
}

#[post("/lists")]
async fn create_list(
    pool: web::Data<DbPool>,
    body: web::Json<CreateSpeciesListDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let new = NewSpeciesList {
        id: Uuid::new_v4(),
        name: dto.name,
        description: dto.description,
        position: dto.position.unwrap_or(DEFAULT_LIST_POSITION),
    };

    let created = blocking_db(pool, move |conn| {
        let taken = species_lists::table
            .filter(species_lists::name.eq(&new.name))
            .select(species_lists::id)
            .first::<Uuid>(conn)
            .optional()?;
        if taken.is_some() {
            return Err(ServiceError::Conflict(format!(
                "a species list named '{}' already exists",
                new.name
            )));
        }
        diesel::insert_into(species_lists::table)
            .values(&new)
            .execute(conn)?;
        species_lists::table
            .find(new.id)
            .select(SpeciesList::as_select())
            .first::<SpeciesList>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/lists/{list_id}")]
async fn update_list(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateSpeciesListDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    if body.is_empty() {
        return Err(ServiceError::BadRequest(
            "No update fields provided.".to_string(),
        ));
    }
    let list_id = path.into_inner();
    let dto = body.into_inner();

    let updated = blocking_db(pool, move |conn| {
        let current = species_lists::table
            .find(list_id)
            .select(SpeciesList::as_select())
            .first::<SpeciesList>(conn)
            .optional()?
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Species list {list_id} not found"))
            })?;

        let name = dto.name.unwrap_or(current.name);
        let description = dto.description.or(current.description);
        let position = dto.position.unwrap_or(current.position);

        let taken = species_lists::table
            .filter(species_lists::name.eq(&name).and(species_lists::id.ne(list_id)))
            .select(species_lists::id)
            .first::<Uuid>(conn)
            .optional()?;
        if taken.is_some() {
            return Err(ServiceError::Conflict(format!(
                "a species list named '{name}' already exists"
            )));
        }

        diesel::update(species_lists::table.find(list_id))
            .set((
                species_lists::name.eq(&name),
                species_lists::description.eq(&description),
                species_lists::position.eq(position),
            ))
            .returning(SpeciesList::as_select())
            .get_result::<SpeciesList>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/lists/{list_id}")]
async fn delete_list(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let list_id = path.into_inner();

    let deleted_records = blocking_db(pool, move |conn| {
        let exists = species_lists::table
            .find(list_id)
            .select(species_lists::id)
            .first::<Uuid>(conn)
            .optional()?;
        if exists.is_none() {
            return Err(ServiceError::NotFound(format!(
                "Species list {list_id} not found"
            )));
        }

        // 先显式删记录，再删名录。数据库上是 ON DELETE CASCADE，这里显式删
        // 只是为了拿到准确的行数返回给调用方。
        let deleted_records = diesel::delete(
            species_records::table.filter(species_records::list_id.eq(list_id)),
        )
        .execute(conn)?;
        diesel::delete(species_lists::table.find(list_id)).execute(conn)?;
        Ok(deleted_records)
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Species list deleted successfully",
        "id": list_id,
        "deleted_records": deleted_records,
    })))
}

#[post("/records")]
async fn create_record(
    pool: web::Data<DbPool>,
    body: web::Json<CreateSpeciesRecordDto>,
) -> Result<HttpResponse, ServiceError> {
    body.validate()?;
    let dto = body.into_inner();
    let new = NewSpeciesRecord {
        id: Uuid::new_v4(),
        list_id: dto.list_id,
        taxon_id: dto.taxon_id,
        scientific_name: dto.scientific_name,
        chinese_name: dto.chinese_name,
        distribution: dto.distribution,
        note: dto.note,
        source: dto.source,
        record_no: dto.record_no,
    };

    let created = blocking_db(pool, move |conn| {
        ensure_list_exists(conn, new.list_id)?;
        ensure_taxon_exists(conn, new.taxon_id)?;
        check_record_unique(conn, new.list_id, &new.scientific_name, None)?;
        diesel::insert_into(species_records::table)
            .values(&new)
            .execute(conn)?;
        species_records::table
            .find(new.id)
            .select(SpeciesRecord::as_select())
            .first::<SpeciesRecord>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Created().json(created))
}

#[patch("/records/{record_id}")]
async fn update_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateSpeciesRecordDto>,
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
        let current = species_records::table
            .find(record_id)
            .select(SpeciesRecord::as_select())
            .first::<SpeciesRecord>(conn)
            .optional()?
            .ok_or_else(|| {
                ServiceError::NotFound(format!("Species record {record_id} not found"))
            })?;

        let list_id = dto.list_id.unwrap_or(current.list_id);
        let taxon_id = dto.taxon_id.unwrap_or(current.taxon_id);
        let scientific_name = dto.scientific_name.unwrap_or(current.scientific_name);
        let chinese_name = dto.chinese_name.unwrap_or(current.chinese_name);
        let distribution = dto.distribution.unwrap_or(current.distribution);
        let note = dto.note.unwrap_or(current.note);
        let source = dto.source.unwrap_or(current.source);
        let record_no = dto.record_no.unwrap_or(current.record_no);

        ensure_list_exists(conn, list_id)?;
        ensure_taxon_exists(conn, taxon_id)?;
        check_record_unique(conn, list_id, &scientific_name, Some(record_id))?;

        diesel::update(species_records::table.find(record_id))
            .set((
                species_records::list_id.eq(list_id),
                species_records::taxon_id.eq(taxon_id),
                species_records::scientific_name.eq(&scientific_name),
                species_records::chinese_name.eq(&chinese_name),
                species_records::distribution.eq(&distribution),
                species_records::note.eq(&note),
                species_records::source.eq(&source),
                species_records::record_no.eq(&record_no),
                species_records::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(SpeciesRecord::as_select())
            .get_result::<SpeciesRecord>(conn)
            .map_err(ServiceError::from)
    })
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

#[delete("/records/{record_id}")]
async fn delete_record(
    pool: web::Data<DbPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ServiceError> {
    let record_id = path.into_inner();

    blocking_db(pool, move |conn| {
        let deleted = diesel::delete(species_records::table.find(record_id))
            .execute(conn)
            .map_err(ServiceError::from)?;
        if deleted == 0 {
            return Err(ServiceError::NotFound(format!(
                "Species record {record_id} not found"
            )));
        }
        Ok(())
    })
    .await?;

    Ok(HttpResponse::Ok().json(json!({
        "message": "Species record deleted successfully",
        "id": record_id,
    })))
}

// ==========================================================================
// 写接口的公共校验
// ==========================================================================

/// 父节点必须存在，且阶元严格高于子节点。
fn check_parent(
    conn: &mut DbConn,
    parent_id: Option<Uuid>,
    child_rank: TaxonomyRank,
) -> Result<(), ServiceError> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };

    let parent_rank = taxa::table
        .find(parent_id)
        .select(taxa::rank)
        .first::<TaxonomyRank>(conn)
        .optional()?
        .ok_or_else(|| {
            ServiceError::ValidationError(format!("parent taxon {parent_id} not found"))
        })?;

    if !parent_rank.is_strictly_higher_than(child_rank) {
        return Err(ServiceError::ValidationError(format!(
            "parent rank '{}' must be higher than child rank '{}'",
            parent_rank.as_str(),
            child_rank.as_str()
        )));
    }
    Ok(())
}

/// 同一父级下不允许出现同阶元同学名（与 `taxa_sibling_unique` 索引一致）。
fn check_sibling_unique(
    conn: &mut DbConn,
    parent_id: Option<Uuid>,
    rank: TaxonomyRank,
    scientific_name: &str,
    exclude_id: Option<Uuid>,
) -> Result<(), ServiceError> {
    let mut q = taxa::table.into_boxed();
    match parent_id {
        Some(id) => q = q.filter(taxa::parent_id.eq(id)),
        None => q = q.filter(taxa::parent_id.is_null()),
    }
    q = q
        .filter(taxa::rank.eq(rank))
        .filter(taxa::scientific_name.eq(scientific_name));
    if let Some(id) = exclude_id {
        q = q.filter(taxa::id.ne(id));
    }

    if q.select(taxa::id).first::<Uuid>(conn).optional()?.is_some() {
        return Err(ServiceError::Conflict(format!(
            "a {} named '{scientific_name}' already exists under the same parent",
            rank.as_str()
        )));
    }
    Ok(())
}

fn ensure_list_exists(conn: &mut DbConn, list_id: Uuid) -> Result<(), ServiceError> {
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

fn check_record_unique(
    conn: &mut DbConn,
    list_id: Uuid,
    scientific_name: &str,
    exclude_id: Option<Uuid>,
) -> Result<(), ServiceError> {
    let mut q = species_records::table
        .filter(species_records::list_id.eq(list_id))
        .filter(species_records::scientific_name.eq(scientific_name))
        .into_boxed();
    if let Some(id) = exclude_id {
        q = q.filter(species_records::id.ne(id));
    }

    if q.select(species_records::id)
        .first::<Uuid>(conn)
        .optional()?
        .is_some()
    {
        return Err(ServiceError::Conflict(format!(
            "'{scientific_name}' already exists in this species list"
        )));
    }
    Ok(())
}

/// 名录详情（列表本身 + 记录数）。
#[derive(Serialize, Debug)]
struct SpeciesListDetail {
    #[serde(flatten)]
    list: SpeciesList,
    record_count: i64,
}

#[cfg(test)]
mod tests {
    use super::like_pattern;

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("Acer"), "%Acer%");
        // % 和 _ 必须转义，否则用户输入会变成通配符
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("a\\b"), "%a\\\\b%");
    }
}
