//! 鸟类调查「重要记录」的数据模型。
//!
//! 表结构见迁移 `2026-10-11-000000_add_bird_records`。目前「鸟类调查」下只有
//! 这一类数据需要自己建表：「鸟种清单」直接复用 species_records（见前端），
//! 「鸟调记录」还是空标签。之后加鸟调记录时，新模型也放这个文件。

use crate::errors::ServiceError;
use crate::schema::{bird_records, bird_survey_records, bird_surveys};
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
    /// 关联的照片（`species_photos.id`）：可以没有，有则必须是同一物种的照片。
    /// 照片被删除时外键 ON DELETE SET NULL，这里自动变回 NULL。
    pub photo_id: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 接口返回的重要记录：数据库字段 + 拼好的照片公开 URL。
///
/// 与 `photo_models::PhotoResponse` 同一个思路：路径怎么暴露给浏览器是部署
/// 决定（nginx `location /uploads/`），前端只认 `photo_url` 这一个字段。
#[derive(Serialize, Debug, Clone)]
pub struct BirdRecordResponse {
    #[serde(flatten)]
    pub record: BirdRecord,
    pub photo_url: Option<String>,
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
    pub photo_id: Option<Uuid>,
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
    /// 关联的照片；物种是否一致由处理器校验（要查库）。
    pub photo_id: Option<Uuid>,
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
    #[serde(default, deserialize_with = "deserialize_some")]
    pub photo_id: Option<Option<Uuid>>,
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
            && self.photo_id.is_none()
    }
}

// ==========================================================================
// 鸟调记录（bird_surveys / bird_survey_records）
// ==========================================================================

/// 一次鸟调：某天的调查，表头是日期 / 领队 / 天气 / 备注。
#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = bird_surveys)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BirdSurvey {
    pub id: Uuid,
    pub survey_date: NaiveDate,
    /// 「领队」。
    pub leader: Option<String>,
    /// 「天气」。
    pub weather: Option<String>,
    pub note: Option<String>,
    /// 数据来源：导入时是原始 GPX 文件名。
    pub source: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 明细里的一条鸟种观察（GPX 的一个 waypoint）。
#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = bird_survey_records)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BirdSurveyRecord {
    pub id: Uuid,
    pub survey_id: Uuid,
    /// 关联的分类节点；匹配不上（旧名 / 名录没有）时为空。
    pub taxon_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    pub chinese_name: String,
    /// 数量（GPX 的 `<desc>`）。
    pub quantity: Option<i32>,
    /// 观察时间，已换算成北京时间。
    pub observed_at: Option<NaiveDateTime>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub note: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 列表里的一行：鸟调表头 + 现算的「鸟种数」（明细里的不同鸟种）。
#[derive(Serialize, Debug, Clone)]
pub struct BirdSurveyResponse {
    #[serde(flatten)]
    pub survey: BirdSurvey,
    pub species_count: i64,
}

/// `GET /bird-surveys/{id}`：表头 + 全部明细。
#[derive(Serialize, Debug, Clone)]
pub struct BirdSurveyDetailResponse {
    #[serde(flatten)]
    pub survey: BirdSurveyResponse,
    pub records: Vec<BirdSurveyRecord>,
}

#[derive(Insertable)]
#[diesel(table_name = bird_surveys)]
pub struct NewBirdSurvey {
    pub id: Uuid,
    pub survey_date: NaiveDate,
    pub leader: Option<String>,
    pub weather: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = bird_survey_records)]
pub struct NewBirdSurveyRecord {
    pub id: Uuid,
    pub survey_id: Uuid,
    pub taxon_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    pub chinese_name: String,
    pub quantity: Option<i32>,
    pub observed_at: Option<NaiveDateTime>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub note: Option<String>,
}

// --- 与数据库约束对齐的长度上限 ---
pub const SURVEY_LEADER_MAX_CHARS: usize = 200; // bird_surveys.leader
pub const SURVEY_WEATHER_MAX_CHARS: usize = 100; // bird_surveys.weather
pub const SURVEY_SOURCE_MAX_CHARS: usize = 255; // bird_surveys.source
pub const SURVEY_RECORD_NAME_MAX_CHARS: usize = 200; // bird_survey_records.chinese_name / scientific_name

/// 手工新建一次鸟调（不含明细，明细另外逐条加）。
#[derive(Deserialize, Debug)]
pub struct CreateBirdSurveyDto {
    pub survey_date: NaiveDate,
    pub leader: Option<String>,
    pub weather: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
}

impl CreateBirdSurveyDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_opt_name("leader", &self.leader, SURVEY_LEADER_MAX_CHARS)?;
        validate_opt_name("weather", &self.weather, SURVEY_WEATHER_MAX_CHARS)?;
        validate_opt_name("source", &self.source, SURVEY_SOURCE_MAX_CHARS)
    }
}

/// 改鸟调表头。可空字段用 `Option<Option<T>>`：缺字段 = 不改，显式 `null` = 置 NULL。
#[derive(Deserialize, Debug)]
pub struct UpdateBirdSurveyDto {
    pub survey_date: Option<NaiveDate>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub leader: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub weather: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub source: Option<Option<String>>,
}

impl UpdateBirdSurveyDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(Some(leader)) = &self.leader {
            validate_name("leader", leader, SURVEY_LEADER_MAX_CHARS)?;
        }
        if let Some(Some(weather)) = &self.weather {
            validate_name("weather", weather, SURVEY_WEATHER_MAX_CHARS)?;
        }
        if let Some(Some(source)) = &self.source {
            validate_name("source", source, SURVEY_SOURCE_MAX_CHARS)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.survey_date.is_none()
            && self.leader.is_none()
            && self.weather.is_none()
            && self.note.is_none()
            && self.source.is_none()
    }
}

/// 新增 / 修改一条鸟种明细。整体替换，不做三态：
/// 界面上的编辑弹窗总是把全部字段一起提交。
#[derive(Deserialize, Debug)]
pub struct SurveyRecordDto {
    /// 关联的鸟种；为空 = 未匹配（此时 `chinese_name` 必填）。
    pub taxon_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    pub chinese_name: Option<String>,
    pub quantity: Option<i32>,
    pub observed_at: Option<NaiveDateTime>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub note: Option<String>,
}

impl SurveyRecordDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        // 没选鸟种时中文名是唯一的名字，必须填；选了鸟种则由后端从分类树带出。
        if self.taxon_id.is_none() {
            match self.chinese_name.as_deref() {
                Some(name) if !name.trim().is_empty() => {
                    validate_name("chinese_name", name, SURVEY_RECORD_NAME_MAX_CHARS)?;
                }
                _ => {
                    return Err(ServiceError::ValidationError(
                        "chinese_name is required when taxon_id is absent".to_string(),
                    ));
                }
            }
        } else if let Some(name) = self.chinese_name.as_deref() {
            if !name.trim().is_empty() {
                validate_name("chinese_name", name, SURVEY_RECORD_NAME_MAX_CHARS)?;
            }
        }
        validate_opt_name(
            "scientific_name",
            &self.scientific_name,
            SURVEY_RECORD_NAME_MAX_CHARS,
        )?;

        if let Some(quantity) = self.quantity {
            if quantity < 0 {
                return Err(ServiceError::ValidationError(
                    "quantity must not be negative".to_string(),
                ));
            }
        }
        if let Some(latitude) = self.latitude {
            if !(-90.0..=90.0).contains(&latitude) {
                return Err(ServiceError::ValidationError(
                    "latitude must be between -90 and 90".to_string(),
                ));
            }
        }
        if let Some(longitude) = self.longitude {
            if !(-180.0..=180.0).contains(&longitude) {
                return Err(ServiceError::ValidationError(
                    "longitude must be between -180 and 180".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// 鸟调列表的查询参数。
#[derive(Deserialize, Debug)]
pub struct BirdSurveyQuery {
    /// 按明细里的鸟种搜：命中中文名 / 学名就返回这次鸟调。
    pub q: Option<String>,
    /// 日期范围（含端点）。
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
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
        assert_eq!(cleared.photo_id, None);
        assert!(!cleared.is_empty());
        assert!(cleared.validate().is_ok());

        // 照片同样支持三态：显式 null = 取消关联
        let unset: UpdateBirdRecordDto = serde_json::from_str(r#"{"photo_id":null}"#).unwrap();
        assert_eq!(unset.photo_id, Some(None));
        assert!(!unset.is_empty());
    }

    #[test]
    fn survey_record_needs_a_name_or_a_taxon() {
        // 未匹配（没有 taxon_id）时中文名必填。
        let unnamed: SurveyRecordDto = serde_json::from_str(r#"{"taxon_id":null}"#).unwrap();
        assert!(unnamed.validate().is_err());

        let blank: SurveyRecordDto =
            serde_json::from_str(r#"{"taxon_id":null,"chinese_name":"  "}"#).unwrap();
        assert!(blank.validate().is_err());

        let manual: SurveyRecordDto =
            serde_json::from_str(r#"{"chinese_name":"普通楼燕"}"#).unwrap();
        assert!(manual.validate().is_ok());

        // 选了鸟种就不用中文名，名字由后端从分类树带出。
        let matched: SurveyRecordDto = serde_json::from_str(
            r#"{"taxon_id":"00000000-0000-0000-0000-000000000001"}"#,
        )
        .unwrap();
        assert!(matched.validate().is_ok());
    }

    #[test]
    fn survey_record_rejects_impossible_numbers() {
        let negative: SurveyRecordDto =
            serde_json::from_str(r#"{"chinese_name":"麻雀","quantity":-1}"#).unwrap();
        assert!(negative.validate().is_err());

        let bad_latitude: SurveyRecordDto =
            serde_json::from_str(r#"{"chinese_name":"麻雀","latitude":91.0}"#).unwrap();
        assert!(bad_latitude.validate().is_err());

        let bad_longitude: SurveyRecordDto =
            serde_json::from_str(r#"{"chinese_name":"麻雀","longitude":-181.0}"#).unwrap();
        assert!(bad_longitude.validate().is_err());

        let ok: SurveyRecordDto = serde_json::from_str(
            r#"{"chinese_name":"麻雀","quantity":3,"latitude":39.99,"longitude":116.30}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn update_survey_distinguishes_absent_null_and_value() {
        let absent: UpdateBirdSurveyDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());

        let cleared: UpdateBirdSurveyDto =
            serde_json::from_str(r#"{"weather":null,"leader":"杨延军"}"#).unwrap();
        assert_eq!(cleared.weather, Some(None));
        assert_eq!(cleared.leader, Some(Some("杨延军".to_string())));
        assert!(!cleared.is_empty());
        assert!(cleared.validate().is_ok());
    }
}
