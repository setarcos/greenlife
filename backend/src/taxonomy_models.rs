//! 分类树（门/纲/目/科/属/种）与物种名录的数据模型。
//!
//! 与 `models.rs`（用户）分开，避免那个文件继续膨胀；两者没有共享逻辑。

use crate::errors::ServiceError;
use crate::schema::sql_types::TaxonomyRank as TaxonomyRankSql;
use crate::schema::{species_lists, species_records, taxa};
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql, FromSqlRow};
use diesel::expression::AsExpression;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Text;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::str::FromStr;
use uuid::Uuid;

// --- 与数据库约束对齐的长度上限 ---
// 不对齐的后果是超长输入会撞数据库约束，返回 500 而不是 400。
pub const TAXON_NAME_MAX_CHARS: usize = 200; // taxa.scientific_name / chinese_name
pub const LIST_NAME_MAX_CHARS: usize = 100; // species_lists.name
pub const RECORD_NO_MAX_CHARS: usize = 50; // species_records.record_no

/// 分类阶元。顺序即「界 > 门 > 纲 > 目 > 科 > 属 > 种」。
///
/// 数据库侧是 PostgreSQL 枚举 `taxonomy_rank`（由 diesel 生成到
/// `schema::sql_types::TaxonomyRank`），这里提供对应的 Rust 类型，
/// 手写 `ToSql` / `FromSql`：枚举在 PG 里的线上表示就是标签文本。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = TaxonomyRankSql)]
#[serde(rename_all = "lowercase")]
pub enum TaxonomyRank {
    Kingdom,
    Phylum,
    Class,
    Order,
    Family,
    Genus,
    Species,
}

impl TaxonomyRank {
    pub const ALL: [TaxonomyRank; 7] = [
        TaxonomyRank::Kingdom,
        TaxonomyRank::Phylum,
        TaxonomyRank::Class,
        TaxonomyRank::Order,
        TaxonomyRank::Family,
        TaxonomyRank::Genus,
        TaxonomyRank::Species,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaxonomyRank::Kingdom => "kingdom",
            TaxonomyRank::Phylum => "phylum",
            TaxonomyRank::Class => "class",
            TaxonomyRank::Order => "order",
            TaxonomyRank::Family => "family",
            TaxonomyRank::Genus => "genus",
            TaxonomyRank::Species => "species",
        }
    }

    /// 阶元深度，1 最高（界）。用来校验父子关系。
    pub fn depth(self) -> i32 {
        match self {
            TaxonomyRank::Kingdom => 1,
            TaxonomyRank::Phylum => 2,
            TaxonomyRank::Class => 3,
            TaxonomyRank::Order => 4,
            TaxonomyRank::Family => 5,
            TaxonomyRank::Genus => 6,
            TaxonomyRank::Species => 7,
        }
    }

    /// 父级必须严格高于子级。允许跳级（苔藓缺目/科时属直接挂到纲下），
    /// 但不允许同级或反向——这条规则同时保证了分类树不可能成环。
    pub fn is_strictly_higher_than(self, other: TaxonomyRank) -> bool {
        self.depth() < other.depth()
    }
}

impl FromStr for TaxonomyRank {
    type Err = ServiceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TaxonomyRank::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .ok_or_else(|| {
                ServiceError::ValidationError(format!(
                    "unknown taxonomy rank '{s}' (expected one of: kingdom, phylum, class, \
                     order, family, genus, species)"
                ))
            })
    }
}

impl ToSql<TaxonomyRankSql, Pg> for TaxonomyRank {
    fn to_sql<'a>(&'a self, out: &mut Output<'a, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<TaxonomyRankSql, Pg> for TaxonomyRank {
    fn from_sql(bytes: <Pg as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        // PostgreSQL 枚举在线上就是标签字符串，借用 Text 的读取实现即可。
        let raw = <String as FromSql<Text, Pg>>::from_sql(bytes)?;
        TaxonomyRank::from_str(&raw)
            .map_err(|_| format!("unknown taxonomy_rank value in database: {raw}").into())
    }
}

// --- 分类树 ---

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = taxa)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Taxon {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub rank: TaxonomyRank,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = taxa)]
pub struct NewTaxon {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub rank: TaxonomyRank,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CreateTaxonDto {
    /// 上一级阶元。为空表示根节点（例如门）。
    pub parent_id: Option<Uuid>,
    pub rank: TaxonomyRank,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
}

impl CreateTaxonDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_name("scientific_name", &self.scientific_name, TAXON_NAME_MAX_CHARS)?;
        validate_opt_name("chinese_name", &self.chinese_name, TAXON_NAME_MAX_CHARS)
    }
}

/// 更新分类节点。
///
/// `parent_id` / `chinese_name` 用 `Option<Option<T>>`：缺字段是 `None`（不改），
/// 显式 `null` 是 `Some(None)`（改成 NULL），有值是 `Some(Some(v))`。
/// 没有这个双层 Option 就无法把节点移回根、或清空中文名。
#[derive(Deserialize, Debug)]
pub struct UpdateTaxonDto {
    #[serde(default, deserialize_with = "deserialize_some")]
    pub parent_id: Option<Option<Uuid>>,
    pub scientific_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub chinese_name: Option<Option<String>>,
}

impl UpdateTaxonDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.scientific_name {
            validate_name("scientific_name", name, TAXON_NAME_MAX_CHARS)?;
        }
        if let Some(Some(cn)) = &self.chinese_name {
            validate_name("chinese_name", cn, TAXON_NAME_MAX_CHARS)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.parent_id.is_none() && self.scientific_name.is_none() && self.chinese_name.is_none()
    }
}

/// 节点详情：本身 + 从根到父节点的路径 + 直接子节点。
#[derive(Serialize, Debug)]
pub struct TaxonDetail {
    #[serde(flatten)]
    pub taxon: Taxon,
    pub path: Vec<Taxon>,
    pub children: Vec<Taxon>,
}

/// 嵌套的分类树节点。
#[derive(Serialize, Debug)]
pub struct TaxonNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub rank: TaxonomyRank,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub children: Vec<TaxonNode>,
}

impl From<Taxon> for TaxonNode {
    fn from(t: Taxon) -> Self {
        Self {
            id: t.id,
            parent_id: t.parent_id,
            rank: t.rank,
            scientific_name: t.scientific_name,
            chinese_name: t.chinese_name,
            children: Vec::new(),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct RankInfo {
    pub rank: TaxonomyRank,
    pub depth: i32,
}

// --- 名录 ---

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = species_lists)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SpeciesList {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: NaiveDateTime,
    /// 展示顺序，小的排前面（见 `list_lists`）。
    pub position: i32,
}

#[derive(Insertable)]
#[diesel(table_name = species_lists)]
pub struct NewSpeciesList {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// 展示顺序，小的排前面。
    pub position: i32,
}

/// 新建名录时的默认顺序（与迁移 `2026-10-10-075917` 的列默认值一致）。
pub const DEFAULT_LIST_POSITION: i32 = 100;

#[derive(Deserialize, Debug)]
pub struct CreateSpeciesListDto {
    pub name: String,
    pub description: Option<String>,
    /// 缺省 / null 都表示「用默认值」。
    pub position: Option<i32>,
}

impl CreateSpeciesListDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_name("name", &self.name, LIST_NAME_MAX_CHARS)?;
        validate_position(&self.position)
    }
}

#[derive(Deserialize, Debug)]
pub struct UpdateSpeciesListDto {
    pub name: Option<String>,
    pub description: Option<String>,
    /// 缺省 / null 都表示「保持不变」（列是 NOT NULL，不存在清空）。
    pub position: Option<i32>,
}

impl UpdateSpeciesListDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.name {
            validate_name("name", name, LIST_NAME_MAX_CHARS)?;
        }
        validate_position(&self.position)
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.description.is_none() && self.position.is_none()
    }
}

// --- 名录记录 ---

#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = species_records)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SpeciesRecord {
    pub id: Uuid,
    pub list_id: Uuid,
    pub taxon_id: Uuid,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub distribution: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
    pub record_no: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = species_records)]
pub struct NewSpeciesRecord {
    pub id: Uuid,
    pub list_id: Uuid,
    pub taxon_id: Uuid,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub distribution: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
    pub record_no: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CreateSpeciesRecordDto {
    pub list_id: Uuid,
    /// 鉴定到的最细阶元（通常是 species，只定到属时是 genus）。
    pub taxon_id: Uuid,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub distribution: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
    pub record_no: Option<String>,
}

impl CreateSpeciesRecordDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_name(
            "scientific_name",
            &self.scientific_name,
            TAXON_NAME_MAX_CHARS,
        )?;
        if let Some(no) = &self.record_no {
            validate_name("record_no", no, RECORD_NO_MAX_CHARS)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
pub struct UpdateSpeciesRecordDto {
    #[serde(default, deserialize_with = "deserialize_some")]
    pub list_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub taxon_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub chinese_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub distribution: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub source: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub record_no: Option<Option<String>>,
}

impl UpdateSpeciesRecordDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.scientific_name {
            validate_name("scientific_name", name, TAXON_NAME_MAX_CHARS)?;
        }
        if let Some(Some(no)) = &self.record_no {
            validate_name("record_no", no, RECORD_NO_MAX_CHARS)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.list_id.is_none()
            && self.taxon_id.is_none()
            && self.scientific_name.is_none()
            && self.chinese_name.is_none()
            && self.distribution.is_none()
            && self.note.is_none()
            && self.source.is_none()
            && self.record_no.is_none()
    }
}

// --- 查询参数 ---

#[derive(Deserialize, Debug)]
pub struct TaxonQuery {
    pub rank: Option<TaxonomyRank>,
    /// 一个 UUID，或字符串 `root`（只看根节点）。
    pub parent_id: Option<String>,
    /// 学名 / 中文名模糊搜索。
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize, Debug)]
pub struct TreeQuery {
    /// 只返回以该节点为根的子树；缺省返回整片森林。
    pub root_id: Option<Uuid>,
    /// 最大展开层数；缺省不限。
    pub depth: Option<usize>,
}

#[derive(Deserialize, Debug)]
pub struct RecordQuery {
    pub list_id: Option<Uuid>,
    pub taxon_id: Option<Uuid>,
    /// taxon_id 是否包含其所有下级（点「门」就能看到门下所有记录）。
    pub descendants: Option<bool>,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- 校验辅助 ---

/// 把「缺字段 / 显式 null / 有值」三态区分开。见 `UpdateTaxonDto` 的说明。
///
/// 维护日志的 `UpdateMaintenanceLogDto` 也用同一套三态语义，所以是 `pub(crate)`。
pub(crate) fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn validate_name(field: &str, value: &str, max_chars: usize) -> Result<(), ServiceError> {
    if value.trim().is_empty() {
        return Err(ServiceError::ValidationError(format!(
            "{field} must not be empty"
        )));
    }
    let chars = value.chars().count();
    if chars > max_chars {
        return Err(ServiceError::ValidationError(format!(
            "{field} must be at most {max_chars} characters, got {chars}"
        )));
    }
    Ok(())
}

fn validate_opt_name(
    field: &str,
    value: &Option<String>,
    max_chars: usize,
) -> Result<(), ServiceError> {
    match value {
        Some(v) => validate_name(field, v, max_chars),
        None => Ok(()),
    }
}

/// `position` 是 `species_lists.position`（INTEGER），只用非负值。
fn validate_position(value: &Option<i32>) -> Result<(), ServiceError> {
    match value {
        Some(v) if *v < 0 => Err(ServiceError::ValidationError(
            "position must not be negative".to_string(),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_depth_orders_the_ladder() {
        assert!(TaxonomyRank::Phylum.is_strictly_higher_than(TaxonomyRank::Class));
        assert!(TaxonomyRank::Genus.is_strictly_higher_than(TaxonomyRank::Species));
        // 允许跳级：属直接挂在纲下（苔藓名录就是这样）
        assert!(TaxonomyRank::Class.is_strictly_higher_than(TaxonomyRank::Genus));
        // 同级 / 反向都不合法
        assert!(!TaxonomyRank::Genus.is_strictly_higher_than(TaxonomyRank::Genus));
        assert!(!TaxonomyRank::Species.is_strictly_higher_than(TaxonomyRank::Genus));
    }

    #[test]
    fn rank_string_round_trip() {
        for rank in TaxonomyRank::ALL {
            assert_eq!(TaxonomyRank::from_str(rank.as_str()).unwrap(), rank);
        }
        assert!(TaxonomyRank::from_str("Phylum").is_err());
        assert!(TaxonomyRank::from_str("subspecies").is_err());
    }

    #[test]
    fn rank_serde_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&TaxonomyRank::Phylum).unwrap(),
            "\"phylum\""
        );
        assert_eq!(
            serde_json::from_str::<TaxonomyRank>("\"species\"").unwrap(),
            TaxonomyRank::Species
        );
        assert!(serde_json::from_str::<TaxonomyRank>("\"Species\"").is_err());
    }

    #[test]
    fn taxon_name_length_matches_db_limit() {
        let ok = CreateTaxonDto {
            parent_id: None,
            rank: TaxonomyRank::Genus,
            scientific_name: "Acer".to_string(),
            chinese_name: Some("槭属".to_string()),
        };
        assert!(ok.validate().is_ok());

        let blank = CreateTaxonDto {
            parent_id: None,
            rank: TaxonomyRank::Genus,
            scientific_name: "  ".to_string(),
            chinese_name: None,
        };
        assert!(blank.validate().is_err());

        let too_long = CreateTaxonDto {
            parent_id: None,
            rank: TaxonomyRank::Genus,
            scientific_name: "x".repeat(TAXON_NAME_MAX_CHARS + 1),
            chinese_name: None,
        };
        assert!(too_long.validate().is_err());
    }

    #[test]
    fn update_dto_distinguishes_absent_and_null() {
        // 缺字段：保持不变
        let absent: UpdateTaxonDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());
        assert_eq!(absent.parent_id, None);

        // 显式 null：改成 NULL（移动到根 / 清空中文名）
        let null: UpdateTaxonDto = serde_json::from_str(r#"{"parent_id":null}"#).unwrap();
        assert_eq!(null.parent_id, Some(None));

        // 有值
        let id = Uuid::new_v4();
        let some: UpdateTaxonDto =
            serde_json::from_str(&format!(r#"{{"parent_id":"{id}"}}"#)).unwrap();
        assert_eq!(some.parent_id, Some(Some(id)));
    }

    #[test]
    fn record_update_allows_clearing_text_fields() {
        let cleared: UpdateSpeciesRecordDto =
            serde_json::from_str(r#"{"note":null,"distribution":"燕南园"}"#).unwrap();
        assert_eq!(cleared.note, Some(None));
        assert_eq!(cleared.distribution, Some(Some("燕南园".to_string())));
        assert!(!cleared.is_empty());
    }

    #[test]
    fn list_position_is_optional_but_never_negative() {
        // 缺字段 / null：都表示「不变」（列是 NOT NULL，没有三态）
        let absent: UpdateSpeciesListDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());
        assert!(absent.validate().is_ok());

        let null: UpdateSpeciesListDto = serde_json::from_str(r#"{"position":null}"#).unwrap();
        assert_eq!(null.position, None);
        assert!(null.is_empty());

        let ok: UpdateSpeciesListDto = serde_json::from_str(r#"{"position":0}"#).unwrap();
        assert_eq!(ok.position, Some(0));
        assert!(!ok.is_empty());
        assert!(ok.validate().is_ok());

        let negative: UpdateSpeciesListDto = serde_json::from_str(r#"{"position":-1}"#).unwrap();
        assert!(negative.validate().is_err());

        // 新建时缺省 → 调用方用 DEFAULT_LIST_POSITION 兜底
        let created: CreateSpeciesListDto =
            serde_json::from_str(r#"{"name":"某名录"}"#).unwrap();
        assert_eq!(created.position, None);
        assert!(created.validate().is_ok());

        let created_negative: CreateSpeciesListDto =
            serde_json::from_str(r#"{"name":"某名录","position":-5}"#).unwrap();
        assert!(created_negative.validate().is_err());
    }
}
