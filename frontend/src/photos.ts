/// 物种照片的类型与本地校验。
///
/// 与 `backend/src/photo_models.rs` 一一对应，改后端记得同步这里。

export interface SpeciesPhoto {
  id: string
  taxon_id: string
  /// 「拍摄人」，可能和上传者不是同一个人。
  photographer: string | null
  /// 「上传用户名」：上传时从 JWT 取，不随用户改名变化。
  uploader_username: string
  /// 「拍摄时间」：后端是 DATE，形如 "2025-03-19"。
  taken_at: string
  location: string | null
  note: string | null
  /// 「评分」：1-10，null = 未评分。
  rating: number | null
  /// 「是否重要记录」。
  is_important: boolean
  /// UPLOAD_PATH 下的相对路径，形如 "2025-03/<uuid>.jpg"。
  file_path: string
  original_filename: string | null
  file_size: number
  content_type: string | null
  created_at: string
  updated_at: string
  /// 后端拼好的公开 URL（`/uploads/<file_path>`）。
  url: string
}

/// 与后端 `MAX_PHOTO_BYTES` 一致：单张照片 2 MiB。
export const PHOTO_MAX_BYTES = 2 * 1024 * 1024
/// 与后端 `MAX_PHOTOS_PER_TAXON` 一致。
export const MAX_PHOTOS_PER_TAXON = 9
export const RATING_MIN = 1
export const RATING_MAX = 10
/// 评分下拉框的候选（1-10）。
export const RATING_OPTIONS = Array.from(
  { length: RATING_MAX - RATING_MIN + 1 },
  (_, index) => RATING_MIN + index,
)

export const ALLOWED_PHOTO_EXTENSIONS = ['jpg', 'jpeg', 'png', 'gif', 'webp']
export const ALLOWED_PHOTO_MIME_TYPES = ['image/jpeg', 'image/png', 'image/gif', 'image/webp']

/// 上传前的即时反馈（服务端仍会再校验一遍）。
/// 返回第一条错误信息，通过时返回空串。
export function validatePhotoFile(file: File): string {
  if (file.size === 0) {
    return '照片文件是空的'
  }
  if (file.size > PHOTO_MAX_BYTES) {
    return `照片不能超过 2M（当前 ${formatFileSize(file.size)}）`
  }
  // 浏览器给不出类型时退回扩展名，别把能传的文件拦下来。
  if (file.type !== '') {
    if (!ALLOWED_PHOTO_MIME_TYPES.includes(file.type)) {
      return `只支持 ${ALLOWED_PHOTO_EXTENSIONS.join(' / ')} 格式的照片`
    }
  } else {
    const ext = file.name.split('.').pop()?.toLowerCase() ?? ''
    if (!ALLOWED_PHOTO_EXTENSIONS.includes(ext)) {
      return `只支持 ${ALLOWED_PHOTO_EXTENSIONS.join(' / ')} 格式的照片`
    }
  }
  return ''
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`
  }
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
}
