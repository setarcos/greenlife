import { http } from './http'
import type {
  RankInfo,
  SpeciesList,
  SpeciesListDetail,
  SpeciesRecord,
  Taxon,
  TaxonDetail,
  TaxonNode,
  TaxonomyRank,
} from '../taxonomy'

/// 所有读接口都挂在公开的 `/taxonomy` scope 下，未登录也能调用。

export interface TaxonQuery {
  rank?: TaxonomyRank
  /// 一个 UUID，或字符串 `'root'`（只看根节点）。缺省不过滤父级。
  parentId?: string
  /// 学名 / 中文名模糊搜索。
  q?: string
  limit?: number
  offset?: number
}

export interface TreeQuery {
  /// 只返回以该节点为根的子树；缺省返回整片森林。
  rootId?: string
  /// 最大展开层数；缺省不限。
  depth?: number
}

export interface RecordQuery {
  listId?: string
  taxonId?: string
  /// taxon_id 是否包含其所有下级（点「门」就能看到门下所有记录）。
  descendants?: boolean
  q?: string
  limit?: number
  offset?: number
}

/// GET /taxonomy/ranks：阶元及深度，供前端拼下拉框。
export async function listRanks(): Promise<RankInfo[]> {
  const { data } = await http.get<RankInfo[]>('/taxonomy/ranks')
  return data
}

/// GET /taxonomy/taxa：扁平列表，支持 rank / parent_id / q / limit / offset。
export async function listTaxa(query: TaxonQuery = {}): Promise<Taxon[]> {
  const { data } = await http.get<Taxon[]>('/taxonomy/taxa', {
    params: {
      rank: query.rank,
      parent_id: query.parentId,
      q: query.q,
      limit: query.limit,
      offset: query.offset,
    },
  })
  return data
}

/// GET /taxonomy/taxa/{id}：节点 + 根到父的路径 + 直接子节点。
export async function getTaxon(taxonId: string): Promise<TaxonDetail> {
  const { data } = await http.get<TaxonDetail>(`/taxonomy/taxa/${taxonId}`)
  return data
}

/// GET /taxonomy/tree：嵌套树。
export async function getTree(query: TreeQuery = {}): Promise<TaxonNode[]> {
  const { data } = await http.get<TaxonNode[]>('/taxonomy/tree', {
    params: { root_id: query.rootId, depth: query.depth },
  })
  return data
}

/// GET /taxonomy/lists：所有名录。
export async function listSpeciesLists(): Promise<SpeciesList[]> {
  const { data } = await http.get<SpeciesList[]>('/taxonomy/lists')
  return data
}

/// GET /taxonomy/lists/{id}：名录详情（含 record_count）。
export async function getSpeciesList(listId: string): Promise<SpeciesListDetail> {
  const { data } = await http.get<SpeciesListDetail>(`/taxonomy/lists/${listId}`)
  return data
}

/// GET /taxonomy/records：记录列表。
export async function listRecords(query: RecordQuery = {}): Promise<SpeciesRecord[]> {
  const { data } = await http.get<SpeciesRecord[]>('/taxonomy/records', {
    params: {
      list_id: query.listId,
      taxon_id: query.taxonId,
      descendants: query.descendants,
      q: query.q,
      limit: query.limit,
      offset: query.offset,
    },
  })
  return data
}

/// GET /taxonomy/records/{id}：单条记录。
export async function getRecord(recordId: string): Promise<SpeciesRecord> {
  const { data } = await http.get<SpeciesRecord>(`/taxonomy/records/${recordId}`)
  return data
}
