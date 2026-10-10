//! 维护日志：名录的修订记录（对应 xlsx 各名录「说明」表的四列）。
//!
//! 与 `taxonomy_models.rs`（分类树 / 名录）分开，只共用三态字段的
//! `deserialize_some`。表结构见迁移 `2026-10-10-100000_add_maintenance_logs`。

use crate::errors::ServiceError;
use crate::schema::maintenance_logs;
use crate::taxonomy_models::deserialize_some;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- 与数据库约束对齐的长度上限 ---
pub const ENTRY_DATE_MAX_CHARS: usize = 50; // maintenance_logs.entry_date
pub const AUTHOR_MAX_CHARS: usize = 200; // maintenance_logs.author

/// 一条维护日志。`list_id` 为 NULL 表示全局日志（不属于某一份名录）。
#[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
#[diesel(table_name = maintenance_logs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MaintenanceLog {
    pub id: Uuid,
    pub list_id: Option<Uuid>,
    /// 「修订笔记」：日期或一段说明文字。导入脚本（`import_maintenance_logs_xlsx.py`）
    /// 会把 Excel 序列号 / yyyyMMdd / yyyyMM 归一成 yyyy-mm-dd / yyyy-mm，
    /// 不能识别的文字（"2022.11.25 - 至今"）原样保留；手工录入时是自由文本。
    pub entry_date: Option<String>,
    /// 「修订人」。
    pub author: Option<String>,
    /// 「修订说明」，不能为空。
    pub summary: String,
    /// 「物种附录」，多行文本原样保留。
    pub species_appendix: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = maintenance_logs)]
pub struct NewMaintenanceLog {
    pub id: Uuid,
    pub list_id: Option<Uuid>,
    pub entry_date: Option<String>,
    pub author: Option<String>,
    pub summary: String,
    pub species_appendix: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CreateMaintenanceLogDto {
    /// 缺省 / null 都表示全局日志。
    pub list_id: Option<Uuid>,
    pub entry_date: Option<String>,
    pub author: Option<String>,
    pub summary: String,
    pub species_appendix: Option<String>,
}

impl CreateMaintenanceLogDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        validate_summary(&self.summary)?;
        validate_opt_len("entry_date", &self.entry_date, ENTRY_DATE_MAX_CHARS)?;
        validate_opt_len("author", &self.author, AUTHOR_MAX_CHARS)
    }
}

/// 更新维护日志。
///
/// `list_id` / `entry_date` / `author` / `species_appendix` 是可空列，用
/// `Option<Option<T>>` 区分「缺字段 = 不改 / 显式 null = 置 NULL / 有值 = 改值」；
/// `summary` 是 NOT NULL，只有「缺字段」和「改成新值」两种（见
/// `taxonomy_models::UpdateTaxonDto` 的同一套语义）。
#[derive(Deserialize, Debug)]
pub struct UpdateMaintenanceLogDto {
    #[serde(default, deserialize_with = "deserialize_some")]
    pub list_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub entry_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub author: Option<Option<String>>,
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub species_appendix: Option<Option<String>>,
}

impl UpdateMaintenanceLogDto {
    pub fn validate(&self) -> Result<(), ServiceError> {
        if let Some(summary) = &self.summary {
            validate_summary(summary)?;
        }
        if let Some(Some(entry_date)) = &self.entry_date {
            validate_len("entry_date", entry_date, ENTRY_DATE_MAX_CHARS)?;
        }
        if let Some(Some(author)) = &self.author {
            validate_len("author", author, AUTHOR_MAX_CHARS)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.list_id.is_none()
            && self.entry_date.is_none()
            && self.author.is_none()
            && self.summary.is_none()
            && self.species_appendix.is_none()
    }
}

// --- 查询参数 ---

#[derive(Deserialize, Debug)]
pub struct MaintenanceLogQuery {
    /// 只看某一份名录的日志；缺省返回全部（含全局日志）。
    pub list_id: Option<Uuid>,
    /// 在修订说明 / 修订人里模糊搜索。
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- 校验辅助 ---

/// `summary` 是 TEXT（没有数据库长度上限），只要求非空。
fn validate_summary(value: &str) -> Result<(), ServiceError> {
    if value.trim().is_empty() {
        return Err(ServiceError::ValidationError(
            "summary must not be empty".to_string(),
        ));
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

fn validate_len(field: &str, value: &str, max_chars: usize) -> Result<(), ServiceError> {
    let chars = value.chars().count();
    if chars > max_chars {
        return Err(ServiceError::ValidationError(format!(
            "{field} must be at most {max_chars} characters, got {chars}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_requires_non_blank_summary() {
        let ok: CreateMaintenanceLogDto = serde_json::from_str(
            r#"{"list_id":null,"entry_date":"20230402","author":"周正暘","summary":"重新排序"}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());
        // list_id 缺省 = 全局日志
        let global: CreateMaintenanceLogDto =
            serde_json::from_str(r#"{"summary":"某次修订"}"#).unwrap();
        assert_eq!(global.list_id, None);
        assert!(global.validate().is_ok());

        let blank: CreateMaintenanceLogDto = serde_json::from_str(r#"{"summary":"  "}"#).unwrap();
        assert!(blank.validate().is_err());
    }

    #[test]
    fn varchar_lengths_match_db_limits() {
        let long_date: CreateMaintenanceLogDto = serde_json::from_str(&format!(
            r#"{{"entry_date":"{}","summary":"x"}}"#,
            "d".repeat(ENTRY_DATE_MAX_CHARS + 1)
        ))
        .unwrap();
        assert!(long_date.validate().is_err());

        let long_author: CreateMaintenanceLogDto = serde_json::from_str(&format!(
            r#"{{"author":"{}","summary":"x"}}"#,
            "a".repeat(AUTHOR_MAX_CHARS + 1)
        ))
        .unwrap();
        assert!(long_author.validate().is_err());
    }

    #[test]
    fn update_distinguishes_absent_null_and_value() {
        // 缺字段：全部保持不变
        let absent: UpdateMaintenanceLogDto = serde_json::from_str("{}").unwrap();
        assert!(absent.is_empty());
        assert_eq!(absent.author, None);

        // 显式 null：把可空字段置 NULL
        let cleared: UpdateMaintenanceLogDto =
            serde_json::from_str(r#"{"list_id":null,"author":null,"summary":"保留"}"#).unwrap();
        assert_eq!(cleared.list_id, Some(None));
        assert_eq!(cleared.author, Some(None));
        assert_eq!(cleared.summary.as_deref(), Some("保留"));
        assert!(!cleared.is_empty());
        assert!(cleared.validate().is_ok());

        // 有值
        let id = Uuid::new_v4();
        let some: UpdateMaintenanceLogDto =
            serde_json::from_str(&format!(r#"{{"list_id":"{id}"}}"#)).unwrap();
        assert_eq!(some.list_id, Some(Some(id)));

        // summary 是 NOT NULL：`null` 与「字段缺失」一样当作不变（与
        // `UpdateSpeciesListDto` 对 description 的处理一致），不能用来清空。
        let null_summary: UpdateMaintenanceLogDto =
            serde_json::from_str(r#"{"summary":null}"#).unwrap();
        assert_eq!(null_summary.summary, None);
        // 空串 summary 被拒
        let blank: UpdateMaintenanceLogDto = serde_json::from_str(r#"{"summary":"   "}"#).unwrap();
        assert!(blank.validate().is_err());
    }
}
