//! 鸟类调查「重要记录」的数据模型。
//!
//! 表结构见迁移 `2026-10-11-000000_add_bird_records`。目前「鸟类调查」下只有
//! 这一类数据需要自己建表：「鸟种清单」直接复用 species_records（见前端），
//! 「鸟调记录」还是空标签。之后加鸟调记录时，新模型也放这个文件。

use crate::errors::ServiceError;
use crate::schema::bird_records;
use crate::taxonomy_models::deserialize_some;
use chrono::{NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- 与数据库约束对齐的长度上限 ---
pub const OBSERVER_MAX_CHARS: usize = 200; // bird_records.observer
pub const OBSERVED_AT_MAX_CHARS: usize = 100; // bird_records.observed_at
pub const LOCATION_MAX_CHARS: usize = 200; // bird_records.location
pub const BIRD_NAME_MAX_CHARS: usize = 200; // bird_records.scientific_name / chinese_name

/// 一条鸟类重要记录。
#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = bird_records)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BirdRecord {
    pub id: Uuid,
    /// 分类树里的物种节点。
    pub taxon_id: Uuid,
    /// 导入时从分类树抄来的冗余列，与 `species_records` 的处理一致。
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    /// 「记录人」。
    pub observer: Option<String>,
    /// 「时间」：原文（2025年3月19日 / 2023年5月底 / 2007年），见迁移里的说明。
    pub observed_at: Option<String>,
    /// `observed_at` 归一出来的时间区间，生成列，随 `observed_at` 自动更新。
    /// 「2023年5月底」是 05-22 ~ 05-31；认不出日期的写法为 NULL。
    pub observed_from: Option<NaiveDate>,
    pub observed_to: Option<NaiveDate>,
    /// 「地点」。
    pub location: Option<String>,
    /// 「备注」：去掉记录人 / 时间 / 地点之后剩下的正文。
    pub note: Option<String>,
    /// 「内容来源」，导入时是 docx 文件名。
    pub source: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = bird_records)]
pub struct NewBirdRecord {
    pub id: Uuid,
    pub taxon_id: Uuid,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub observer: Option<String>,
    pub observed_at: Option<String>,
    pub location: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CreateBirdRecordDto {
    pub taxon_id: Uuid,
    pub scientific_name: String,
    pub chinese_name: Option<String>,
    pub observer: Option<String>,
    pub observed_at: Option<String>,
    pub location: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
}

impl CreateBirdRecordDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_name(
            "scientific_name",
            &self.scientific_name,
            BIRD_NAME_MAX_CHARS,
        )?;
        validate_opt_name("chinese_name", &self.chinese_name, BIRD_NAME_MAX_CHARS)?;
        validate_opt_name("observer", &self.observer, OBSERVER_MAX_CHARS)?;
        validate_opt_name("observed_at", &self.observed_at, OBSERVED_AT_MAX_CHARS)?;
        validate_opt_name("location", &self.location, LOCATION_MAX_CHARS)
    }
}

/// 更新重要记录。
///
/// 可空字段用 `Option<Option<T>>`：缺字段 = 不改，显式 `null` = 置 NULL，
/// 有值 = 改值（同 `taxonomy_models::UpdateTaxonDto`）。
#[derive(Deserialize, Debug)]
pub struct UpdateBirdRecordDto {
    #[serde(default, deserialize_with = "deserialize_some")]
    pub taxon_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub chinese_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub observer: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub observed_at: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub location: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub source: Option<Option<String>>,
}

impl UpdateBirdRecordDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(name) = &self.scientific_name {
            validate_name("scientific_name", name, BIRD_NAME_MAX_CHARS)?;
        }
        if let Some(Some(name)) = &self.chinese_name {
            validate_name("chinese_name", name, BIRD_NAME_MAX_CHARS)?;
        }
        if let Some(Some(observer)) = &self.observer {
            validate_name("observer", observer, OBSERVER_MAX_CHARS)?;
        }
        if let Some(Some(observed_at)) = &self.observed_at {
            validate_name("observed_at", observed_at, OBSERVED_AT_MAX_CHARS)?;
        }
        if let Some(Some(location)) = &self.location {
            validate_name("location", location, LOCATION_MAX_CHARS)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.taxon_id.is_none()
            && self.scientific_name.is_none()
            && self.chinese_name.is_none()
            && self.observer.is_none()
            && self.observed_at.is_none()
            && self.location.is_none()
            && self.note.is_none()
            && self.source.is_none()
    }
}

// --- 查询参数 ---

#[derive(Deserialize, Debug)]
pub struct BirdRecordQuery {
    /// 在学名 / 中文名 / 记录人 / 地点里模糊搜索。
    pub q: Option<String>,
    /// 时间范围（含端点）。按归一化后的区间求交集：记录的时间段与
    /// [from, to] 有重叠就命中，所以「2023年5月底」既能被 5 月下旬查到，
    /// 也能被「2023 年全年」查到。`observed_at` 为空（认不出日期）的记录
    /// 不参与时间筛选。
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- 校验辅助 ---

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requires_scientific_name() {
        let ok: CreateBirdRecordDto = serde_json::from_str(
            r#"{"taxon_id":"00000000-0000-0000-0000-000000000001","scientific_name":"Cygnus columbianus","observer":"杨延军","observed_at":"2025年3月19日","location":"未名湖","note":"一只"}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());

        // 记录人 / 时间 / 地点都可以缺（docx 里有只有日期+事件的记录）
        let sparse: CreateBirdRecordDto = serde_json::from_str(
            r#"{"taxon_id":"00000000-0000-0000-0000-000000000001","scientific_name":"Helopsaltes certhiola","observed_at":"2023年6月3日","note":"猫捕杀"}"#,
        )
        .unwrap();
        assert!(sparse.validate().is_ok());

        let blank: CreateBirdRecordDto = serde_json::from_str(
            r#"{"taxon_id":"00000000-0000-0000-0000-000000000001","scientific_name":"  "}"#,
        )
        .unwrap();
        assert!(blank.validate().is_err());
    }

    #[test]
    fn varchar_lengths_match_db_limits() {
        let long_observer: CreateBirdRecordDto = serde_json::from_str(&format!(
            r#"{{"taxon_id":"00000000-0000-0000-0000-000000000001","scientific_name":"X","observer":"{}"}}"#,
            "o".repeat(OBSERVER_MAX_CHARS + 1)
        ))
        .unwrap();
        assert!(long_observer.validate().is_err());

        let long_location: CreateBirdRecordDto = serde_json::from_str(&format!(
            r#"{{"taxon_id":"00000000-0000-0000-0000-000000000001","scientific_name":"X","location":"{}"}}"#,
            "l".repeat(LOCATION_MAX_CHARS + 1)
        ))
        .unwrap();
        assert!(long_location.validate().is_err());
    }

    #[test]
    fn query_parses_time_range() {
        let range: BirdRecordQuery =
            serde_json::from_str(r#"{"from":"2023-05-20","to":"2023-05-31"}"#).unwrap();
        assert_eq!(range.from.unwrap().to_string(), "2023-05-20");
        assert_eq!(range.to.unwrap().to_string(), "2023-05-31");

        // 只有一端也是合法的（单边开区间）
        let open: BirdRecordQuery = serde_json::from_str(r#"{"from":"2023-01-01"}"#).unwrap();
        assert_eq!(open.to, None);

        // 非 ISO 写法解析失败（actix 会因此返回 400，而不是静默忽略）
        assert!(serde_json::from_str::<BirdRecordQuery>(r#"{"from":"2023/05/20"}"#).is_err());
    }

    #[test]
    fn update_distinguishes_absent_null_and_value() {
        let absent: UpdateBirdRecordDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());

        let cleared: UpdateBirdRecordDto =
            serde_json::from_str(r#"{"observer":null,"location":"中水池"}"#).unwrap();
        assert_eq!(cleared.observer, Some(None));
        assert_eq!(cleared.location, Some(Some("中水池".to_string())));
        assert!(!cleared.is_empty());
        assert!(cleared.validate().is_ok());
    }
}
