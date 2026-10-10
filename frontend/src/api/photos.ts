import { http } from './http'
import type { Paged } from '../pagination'
import type { SpeciesPhoto } from '../photos'

/// 读接口挂在公开的 `/photos` 下，写接口挂在 `/staff/photos`
/// （STAFF 或 ADMIN），见 backend/src/main.rs。

export interface PhotoQuery {
  /// 只看某个物种的照片。
  taxonId?: string
  /// 只看重要记录。
  important?: boolean
  /// 在拍摄人 / 上传用户名 / 地点 / 备注里模糊搜索。
  q?: string
  limit?: number
  offset?: number
}

/// GET /photos：分页响应带总数，供页码条用。
export async function listPhotos(query: PhotoQuery = {}): Promise<Paged<SpeciesPhoto>> {
  const { data } = await http.get<Paged<SpeciesPhoto>>('/photos', {
    params: {
      taxon_id: query.taxonId,
      important: query.important,
      q: query.q,
      limit: query.limit,
      offset: query.offset,
    },
  })
  return data
}

/// GET /photos/{id}
export async function getPhoto(photoId: string): Promise<SpeciesPhoto> {
  const { data } = await http.get<SpeciesPhoto>(`/photos/${photoId}`)
  return data
}

// --- 写接口（STAFF / ADMIN）---

export interface CreatePhotoPayload {
  file: File
  taxonId: string
  /// yyyy-mm-dd
  takenAt: string
  photographer?: string | null
  location?: string | null
  note?: string | null
  rating?: number | null
  isImportant?: boolean
}

/// POST /staff/photos（multipart/form-data）
export async function createPhoto(payload: CreatePhotoPayload): Promise<SpeciesPhoto> {
  const form = new FormData()
  form.append('file', payload.file)
  form.append('taxon_id', payload.taxonId)
  form.append('taken_at', payload.takenAt)
  if (payload.photographer) {
    form.append('photographer', payload.photographer)
  }
  if (payload.location) {
    form.append('location', payload.location)
  }
  if (payload.note) {
    form.append('note', payload.note)
  }
  if (payload.rating !== null && payload.rating !== undefined) {
    form.append('rating', String(payload.rating))
  }
  form.append('is_important', payload.isImportant === true ? 'true' : 'false')

  // 不手动设 Content-Type：交给 axios 带上 multipart 的 boundary。
  const { data } = await http.post<SpeciesPhoto>('/staff/photos', form)
  return data
}

/// 三态：字段缺失 = 保持不变，`null` = 置为 NULL，有值 = 改成新值。
/// 拍摄时间 / 文件本身不可改。
export interface UpdatePhotoPayload {
  photographer?: string | null
  location?: string | null
  note?: string | null
  rating?: number | null
  is_important?: boolean
}

/// PATCH /staff/photos/{id}
export async function updatePhoto(
  photoId: string,
  payload: UpdatePhotoPayload,
): Promise<SpeciesPhoto> {
  const { data } = await http.patch<SpeciesPhoto>(`/staff/photos/${photoId}`, payload)
  return data
}

/// DELETE /staff/photos/{id}
export async function deletePhoto(photoId: string): Promise<void> {
  await http.delete(`/staff/photos/${photoId}`)
}
