/// 分类树与物种名录的类型、阶元辅助函数。
///
/// 与 `backend/src/taxonomy_models.rs` 一一对应，改后端记得同步这里。

export type TaxonomyRank = 'kingdom' | 'phylum' | 'class' | 'order' | 'family' | 'genus' | 'species'

/// 顺序即深度：界 > 门 > 纲 > 目 > 科 > 属 > 种（后端 `TaxonomyRank::ALL`）。
export const RANKS: readonly TaxonomyRank[] = [
  'kingdom',
  'phylum',
  'class',
  'order',
  'family',
  'genus',
  'species',
]

const RANK_LABELS: Record<TaxonomyRank, string> = {
  kingdom: '界',
  phylum: '门',
  class: '纲',
  order: '目',
  family: '科',
  genus: '属',
  species: '种',
}

export function rankLabel(rank: TaxonomyRank): string {
  return RANK_LABELS[rank]
}

/// 1 最高（界），7 最低（种）。与后端 `TaxonomyRank::depth` 对齐。
export function rankDepth(rank: TaxonomyRank): number {
  return RANKS.indexOf(rank) + 1
}

/// 父级必须严格高于子级，允许跳级。
export function isStrictlyHigher(parent: TaxonomyRank, child: TaxonomyRank): boolean {
  return rankDepth(parent) < rankDepth(child)
}

/// 分类树里的一个节点（`GET /taxonomy/taxa`、`/taxonomy/tree` 等）。
export interface Taxon {
  id: string
  parent_id: string | null
  rank: TaxonomyRank
  scientific_name: string
  chinese_name: string | null
  created_at: string
  updated_at: string
}

/// `GET /taxonomy/taxa/{id}`：后端 `#[serde(flatten)]` 了 taxon，所以字段是平铺的。
export interface TaxonDetail extends Taxon {
  /// 从根到**父节点**的路径，不含自身。
  path: Taxon[]
  children: Taxon[]
}

/// 嵌套树的节点。没有时间戳字段（后端 `TaxonNode` 就不带）。
export interface TaxonNode {
  id: string
  parent_id: string | null
  rank: TaxonomyRank
  scientific_name: string
  chinese_name: string | null
  children: TaxonNode[]
}

export interface RankInfo {
  rank: TaxonomyRank
  depth: number
}

export interface SpeciesList {
  id: string
  name: string
  description: string | null
  created_at: string
  /// 展示顺序，小的排前面（后端 `GET /taxonomy/lists` 已按它排好）。
  position: number
}

/// `GET /taxonomy/lists/{id}`：同样是平铺的 list 字段 + record_count。
export interface SpeciesListDetail extends SpeciesList {
  record_count: number
}

export interface SpeciesRecord {
  id: string
  list_id: string
  taxon_id: string
  scientific_name: string
  chinese_name: string | null
  distribution: string | null
  note: string | null
  source: string | null
  record_no: string | null
  created_at: string
  updated_at: string
}

/// 学名 + 中文名的展示文本。
export function taxonLabel(taxon: Pick<Taxon, 'scientific_name' | 'chinese_name'>): string {
  return taxon.chinese_name === null
    ? taxon.scientific_name
    : `${taxon.chinese_name} ${taxon.scientific_name}`
}
