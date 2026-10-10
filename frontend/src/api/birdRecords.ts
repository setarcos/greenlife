import { http } from './http'
import type { BirdRecord } from '../birds'

/// 读接口挂在公开的 `/bird-records` 下，写接口挂在 `/staff/bird-records`
/// （STAFF 或 ADMIN），见 backend/src/main.rs。

export interface BirdRecordQuery {
  /// 在学名 / 中文名 / 记录人 / 地点里模糊搜索。
  q?: string
  /// 时间范围（含端点，`yyyy-mm-dd`）。匹配的是归一化后的时间区间：
  /// 记录的时间段与 [from, to] 有重叠就命中，所以「2023年5月底」
  /// 既能被 5 月下旬查到，也能被「2023 年全年」查到。可以只传一端。
  from?: string
  to?: string
  limit?: number
  offset?: number
}

/// GET /bird-records
export async function listBirdRecords(query: BirdRecordQuery = {}): Promise<BirdRecord[]> {
  const { data } = await http.get<BirdRecord[]>('/bird-records', {
    params: {
      q: query.q,
      from: query.from,
      to: query.to,
      limit: query.limit,
      offset: query.offset,
    },
  })
  return data
}

/// GET /bird-records/{id}
export async function getBirdRecord(recordId: string): Promise<BirdRecord> {
  const { data } = await http.get<BirdRecord>(`/bird-records/${recordId}`)
  return data
}

// --- 写接口（STAFF / ADMIN）---

export interface CreateBirdRecordPayload {
  taxon_id: string
  scientific_name: string
  chinese_name: string | null
  observer: string | null
  observed_at: string | null
  location: string | null
  note: string | null
  source: string | null
}

/// 三态：字段缺失 = 保持不变，`null` = 置为 NULL，有值 = 改成新值。
export interface UpdateBirdRecordPayload {
  taxon_id?: string
  scientific_name?: string
  chinese_name?: string | null
  observer?: string | null
  observed_at?: string | null
  location?: string | null
  note?: string | null
  source?: string | null
}

/// POST /staff/bird-records
export async function createBirdRecord(payload: CreateBirdRecordPayload): Promise<BirdRecord> {
  const { data } = await http.post<BirdRecord>('/staff/bird-records', payload)
  return data
}

/// PATCH /staff/bird-records/{id}
export async function updateBirdRecord(
  recordId: string,
  payload: UpdateBirdRecordPayload,
): Promise<BirdRecord> {
  const { data } = await http.patch<BirdRecord>(`/staff/bird-records/${recordId}`, payload)
  return data
}

/// DELETE /staff/bird-records/{id}
export async function deleteBirdRecord(recordId: string): Promise<void> {
  await http.delete(`/staff/bird-records/${recordId}`)
}
