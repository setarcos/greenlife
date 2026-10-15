/// 分类树与物种名录的类型、阶元辅助函数。
///
/// 与 `backend/src/taxonomy_models.rs` 一一对应，改后端记得同步这里。

export type TaxonomyRank = 'phylum' | 'class' | 'order' | 'family' | 'genus' | 'species'

/// 顺序即深度：门 > 纲 > 目 > 科 > 属 > 种。
///
/// 不含「界」：分类树的根就是门（允许跳级，有些类群直接从纲/目起），
/// 所以类群选择器、卡片的分类阶元都从门开始，没有界这一级。
/// （后端 `taxonomy_rank` 枚举里还留着 `kingdom`，但本项目不用。）
export const RANKS: readonly TaxonomyRank[] = [
  'phylum',
  'class',
  'order',
  'family',
  'genus',
  'species',
]

const RANK_LABELS: Record<TaxonomyRank, string> = {
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

/// 1 最高（门），6 最低（种）。只用来比较两个阶元谁高谁低（`isStrictlyHigher`）。
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
