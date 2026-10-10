import { http } from './http'
import type { MaintenanceLog } from '../maintenanceLogs'
import type { Paged } from '../pagination'

/// 读接口挂在公开的 `/maintenance-logs` 下，写接口挂在 `/staff/maintenance-logs`
/// （STAFF 或 ADMIN），见 backend/src/main.rs。

export interface MaintenanceLogQuery {
  listId?: string
  /// 在「修订说明」和「修订人」里模糊搜索。
  q?: string
  limit?: number
  offset?: number
}

/// GET /maintenance-logs：分页响应带总数，供页码条用。
export async function listMaintenanceLogs(
  query: MaintenanceLogQuery = {},
): Promise<Paged<MaintenanceLog>> {
  const { data } = await http.get<Paged<MaintenanceLog>>('/maintenance-logs', {
    params: {
      list_id: query.listId,
      q: query.q,
      limit: query.limit,
      offset: query.offset,
    },
  })
  return data
}

/// GET /maintenance-logs/{id}
export async function getMaintenanceLog(logId: string): Promise<MaintenanceLog> {
  const { data } = await http.get<MaintenanceLog>(`/maintenance-logs/${logId}`)
  return data
}

// --- 写接口（STAFF / ADMIN）---

export interface CreateMaintenanceLogPayload {
  /// null = 全局日志。
  list_id: string | null
  entry_date: string | null
  author: string | null
  summary: string
  species_appendix: string | null
}

/// 三态：字段缺失 = 保持不变，`null` = 置为 NULL，有值 = 改成新值。
/// `summary` 是 NOT NULL，只能「缺字段」或「改成新值」。
export interface UpdateMaintenanceLogPayload {
  list_id?: string | null
  entry_date?: string | null
  author?: string | null
  summary?: string
  species_appendix?: string | null
}

/// POST /staff/maintenance-logs
export async function createMaintenanceLog(
  payload: CreateMaintenanceLogPayload,
): Promise<MaintenanceLog> {
  const { data } = await http.post<MaintenanceLog>('/staff/maintenance-logs', payload)
  return data
}

/// PATCH /staff/maintenance-logs/{id}
export async function updateMaintenanceLog(
  logId: string,
  payload: UpdateMaintenanceLogPayload,
): Promise<MaintenanceLog> {
  const { data } = await http.patch<MaintenanceLog>(`/staff/maintenance-logs/${logId}`, payload)
  return data
}

/// DELETE /staff/maintenance-logs/{id}
export async function deleteMaintenanceLog(logId: string): Promise<void> {
  await http.delete(`/staff/maintenance-logs/${logId}`)
}
