/// 维护日志：一份名录的修订记录，对应 xlsx 各名录「说明」表的
/// 「修订笔记 / 修订人 / 修订说明 / 物种附录」。
///
/// 与 `backend/src/maintenance_models.rs` 的 `MaintenanceLog` 一一对应。

export interface MaintenanceLog {
  id: string
  /// NULL = 不属于任何一份名录的全局日志。
  list_id: string | null
  /// 「修订笔记」：导入脚本把 Excel 序列号 / yyyyMMdd / yyyyMM 归一成
  /// yyyy-mm-dd / yyyy-mm；不能识别的文字（"2022.11.25 - 至今"）原样保留。
  entry_date: string | null
  author: string | null
  summary: string
  /// 「物种附录」，可能多行。
  species_appendix: string | null
  created_at: string
  updated_at: string
}
