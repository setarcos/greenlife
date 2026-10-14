/// 鸟类调查「鸟调记录」的类型。
///
/// 与 `backend/src/bird_models.rs` 的 `BirdSurvey` / `BirdSurveyRecord` 一一对应。

/// 一次鸟调：某天的调查。`species_count` 由后端按明细里的不同鸟种现算。
export interface BirdSurvey {
  id: string
  /// `yyyy-mm-dd`。
  survey_date: string
  /// 「领队」。
  leader: string | null
  /// 「天气」。
  weather: string | null
  note: string | null
  /// 数据来源：导入时是原始 GPX 文件名。
  source: string | null
  species_count: number
  created_at: string
  updated_at: string
}

/// 明细里的一条鸟种观察（GPX 的一个 waypoint）。
export interface BirdSurveyRecord {
  id: string
  survey_id: string
  /// 关联的鸟种；为 null 表示没匹配上（旧名 / 名录里没有），界面标「未匹配」。
  taxon_id: string | null
  scientific_name: string | null
  chinese_name: string
  /// 数量（GPX 的 `<desc>`）。
  quantity: number | null
  /// 观察时间，后端已换算成北京时间；naive datetime 形如 `2018-05-22T08:08:19.046`。
  observed_at: string | null
  latitude: number | null
  longitude: number | null
  note: string | null
  created_at: string
  updated_at: string
}

/// `GET /bird-surveys/{id}`：表头 + 全部明细。
export interface BirdSurveyDetail extends BirdSurvey {
  records: BirdSurveyRecord[]
}
