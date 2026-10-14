import { http } from './http'
import type { BirdSurvey, BirdSurveyDetail, BirdSurveyRecord } from '../birdSurveys'
import type { Paged } from '../pagination'

/// 读接口挂在公开的 `/bird-surveys` 下，写接口挂在 `/staff/bird-surveys`
/// （STAFF 或 ADMIN），见 backend/src/main.rs。

export interface BirdSurveyQuery {
  /// 按明细里的鸟种搜：命中中文名 / 学名就返回这次鸟调。
  q?: string
  /// 日期范围（含端点，`yyyy-mm-dd`）。
  from?: string
  to?: string
  limit?: number
  offset?: number
}

/// GET /bird-surveys：分页响应带总数，供页码条用。
export async function listBirdSurveys(query: BirdSurveyQuery = {}): Promise<Paged<BirdSurvey>> {
  const { data } = await http.get<Paged<BirdSurvey>>('/bird-surveys', {
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

/// GET /bird-surveys/{id}：表头 + 全部鸟种明细。
export async function getBirdSurvey(surveyId: string): Promise<BirdSurveyDetail> {
  const { data } = await http.get<BirdSurveyDetail>(`/bird-surveys/${surveyId}`)
  return data
}

/// 新增 / 修改鸟种明细的请求体（整体替换，不做三态）。
export interface SurveyRecordPayload {
  taxon_id: string | null
  scientific_name: string | null
  chinese_name: string | null
  quantity: number | null
  observed_at: string | null
  latitude: number | null
  longitude: number | null
  note: string | null
}

/// 改鸟调表头。可空字段的三态：缺失 = 不改，`null` = 置空，有值 = 改成新值。
export interface CreateBirdSurveyPayload {
  survey_date: string
  leader: string | null
  weather: string | null
  note: string | null
  source?: string | null
}

export interface UpdateBirdSurveyPayload {
  survey_date?: string
  leader?: string | null
  weather?: string | null
  note?: string | null
  source?: string | null
}

/// 导入 GPX 的表单。
export interface ImportBirdSurveyPayload {
  file: File
  leader: string
  /// 缺省时后端取最早 waypoint 的日期。
  surveyDate?: string
  weather?: string
  note?: string
}

/// POST /staff/bird-surveys/import（multipart）：解析 GPX 生成一次鸟调。
export async function importBirdSurvey(
  payload: ImportBirdSurveyPayload,
): Promise<BirdSurveyDetail> {
  const form = new FormData()
  form.append('file', payload.file)
  form.append('leader', payload.leader)
  if (payload.surveyDate !== undefined && payload.surveyDate !== '') {
    form.append('survey_date', payload.surveyDate)
  }
  if (payload.weather !== undefined && payload.weather !== '') {
    form.append('weather', payload.weather)
  }
  if (payload.note !== undefined && payload.note !== '') {
    form.append('note', payload.note)
  }
  const { data } = await http.post<BirdSurveyDetail>('/staff/bird-surveys/import', form)
  return data
}

/// POST /staff/bird-surveys
export async function createBirdSurvey(payload: CreateBirdSurveyPayload): Promise<BirdSurvey> {
  const { data } = await http.post<BirdSurvey>('/staff/bird-surveys', payload)
  return data
}

/// PATCH /staff/bird-surveys/{id}
export async function updateBirdSurvey(
  surveyId: string,
  payload: UpdateBirdSurveyPayload,
): Promise<BirdSurvey> {
  const { data } = await http.patch<BirdSurvey>(`/staff/bird-surveys/${surveyId}`, payload)
  return data
}

/// DELETE /staff/bird-surveys/{id}
export async function deleteBirdSurvey(surveyId: string): Promise<void> {
  await http.delete(`/staff/bird-surveys/${surveyId}`)
}

/// POST /staff/bird-surveys/{id}/records
export async function createSurveyRecord(
  surveyId: string,
  payload: SurveyRecordPayload,
): Promise<BirdSurveyRecord> {
  const { data } = await http.post<BirdSurveyRecord>(
    `/staff/bird-surveys/${surveyId}/records`,
    payload,
  )
  return data
}

/// PATCH /staff/bird-surveys/{id}/records/{recordId}
export async function updateSurveyRecord(
  surveyId: string,
  recordId: string,
  payload: SurveyRecordPayload,
): Promise<BirdSurveyRecord> {
  const { data } = await http.patch<BirdSurveyRecord>(
    `/staff/bird-surveys/${surveyId}/records/${recordId}`,
    payload,
  )
  return data
}

/// DELETE /staff/bird-surveys/{id}/records/{recordId}
export async function deleteSurveyRecord(surveyId: string, recordId: string): Promise<void> {
  await http.delete(`/staff/bird-surveys/${surveyId}/records/${recordId}`)
}
