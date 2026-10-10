/// 鸟类调查的类型。
///
/// `BirdRecord` 与 `backend/src/bird_models.rs` 的 `BirdRecord` 一一对应。
/// 「鸟种清单」没有自己的类型：直接复用分类树的 `SpeciesRecord`。

export interface BirdRecord {
  id: string
  /// 分类树里的物种节点。
  taxon_id: string
  scientific_name: string
  chinese_name: string | null
  /// 「记录人」。
  observer: string | null
  /// 「时间」：docx 原文（2025年3月19日 / 2023年5月底 / 2007年），不是严格日期。
  observed_at: string | null
  /// `observed_at` 归一出来的时间区间（NULL = 认不出日期）。
  /// 时间范围筛选按这两个字段求交集，展示上仍然用 `observed_at` 原文。
  observed_from: string | null
  observed_to: string | null
  /// 「地点」。
  location: string | null
  /// 「备注」：去掉记录人 / 时间 / 地点之后剩下的正文。
  note: string | null
  /// 「内容来源」。
  source: string | null
  created_at: string
  updated_at: string
}

/// 「鸟种清单」= 鸟纲（Aves）下的全部名录记录。
///
/// 分类节点的 id 由导入脚本按 UUIDv5 派生，所以不写死在前端：先按
/// 「阶元 = class + 学名 = Aves」找到鸟纲节点，再用 `descendants` 取整棵子树。
export const BIRD_CLASS_SCIENTIFIC_NAME = 'Aves'
