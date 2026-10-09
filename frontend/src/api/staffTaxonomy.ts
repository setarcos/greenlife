import { http } from './http'
import type { SpeciesList, SpeciesRecord, Taxon, TaxonomyRank } from '../taxonomy'

/// 写接口挂在 `/staff/taxonomy`（STAFF 或 ADMIN），见 backend/src/main.rs。

// --- 分类树 ---

export interface CreateTaxonPayload {
  /// 为空表示根节点。
  parent_id: string | null
  rank: TaxonomyRank
  scientific_name: string
  chinese_name: string | null
}

/// 三态：字段缺失 = 保持不变，`null` = 置为 NULL（移到根 / 清空中文名），
/// 有值 = 改成新值。见 docs/物种分类后台.md §2.2。
///
/// `rank` 不可改 —— 改阶元等于移动整棵子树，语义上应该新建节点。
export interface UpdateTaxonPayload {
  parent_id?: string | null
  scientific_name?: string
  chinese_name?: string | null
}

/// POST /staff/taxonomy/taxa
export async function createTaxon(payload: CreateTaxonPayload): Promise<Taxon> {
  const { data } = await http.post<Taxon>('/staff/taxonomy/taxa', payload)
  return data
}

/// PATCH /staff/taxonomy/taxa/{id}
export async function updateTaxon(taxonId: string, payload: UpdateTaxonPayload): Promise<Taxon> {
  const { data } = await http.patch<Taxon>(`/staff/taxonomy/taxa/${taxonId}`, payload)
  return data
}

/// DELETE /staff/taxonomy/taxa/{id}：有子节点或记录时后端返回 409。
export async function deleteTaxon(taxonId: string): Promise<void> {
  await http.delete(`/staff/taxonomy/taxa/${taxonId}`)
}

// --- 名录 ---

export interface CreateListPayload {
  name: string
  description: string | null
}

/// 后端的 `UpdateSpeciesListDto` 是普通 `Option`：字段缺失 / `null` 都表示
/// 「保持不变」，**无法把 description 改回 NULL**，只能改成空字符串。
export interface UpdateListPayload {
  name?: string
  description?: string
}

/// POST /staff/taxonomy/lists
export async function createSpeciesList(payload: CreateListPayload): Promise<SpeciesList> {
  const { data } = await http.post<SpeciesList>('/staff/taxonomy/lists', payload)
  return data
}

/// PATCH /staff/taxonomy/lists/{id}
export async function updateSpeciesList(
  listId: string,
  payload: UpdateListPayload,
): Promise<SpeciesList> {
  const { data } = await http.patch<SpeciesList>(`/staff/taxonomy/lists/${listId}`, payload)
  return data
}

/// DELETE /staff/taxonomy/lists/{id}：级联删掉名录里的全部记录。
export async function deleteSpeciesList(listId: string): Promise<void> {
  await http.delete(`/staff/taxonomy/lists/${listId}`)
}

// --- 名录记录 ---

export interface CreateRecordPayload {
  list_id: string
  /// 鉴定到的最细阶元（通常是 species，只定到属时是 genus）。
  taxon_id: string
  scientific_name: string
  chinese_name: string | null
  distribution: string | null
  note: string | null
  source: string | null
  record_no: string | null
}

/// 与 UpdateTaxonPayload 相同的三态语义。
export interface UpdateRecordPayload {
  list_id?: string
  taxon_id?: string
  scientific_name?: string
  chinese_name?: string | null
  distribution?: string | null
  note?: string | null
  source?: string | null
  record_no?: string | null
}

/// POST /staff/taxonomy/records
export async function createRecord(payload: CreateRecordPayload): Promise<SpeciesRecord> {
  const { data } = await http.post<SpeciesRecord>('/staff/taxonomy/records', payload)
  return data
}

/// PATCH /staff/taxonomy/records/{id}
export async function updateRecord(
  recordId: string,
  payload: UpdateRecordPayload,
): Promise<SpeciesRecord> {
  const { data } = await http.patch<SpeciesRecord>(`/staff/taxonomy/records/${recordId}`, payload)
  return data
}

/// DELETE /staff/taxonomy/records/{id}
export async function deleteRecord(recordId: string): Promise<void> {
  await http.delete(`/staff/taxonomy/records/${recordId}`)
}
