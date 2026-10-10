/// 分页列表的统一响应，对应后端 `taxonomy_handlers::Paged`。
///
/// 各列表接口都返回它：`items` 是本页数据，`total` 是满足条件的总条数。
/// 有 `total` 才能显示总页数、直接跳到某一页 —— 光靠「这一页有没有装满」
/// 只能猜下一屏还有没有。
export interface Paged<T> {
  items: T[]
  total: number
}

/// 总页数。空列表也算 1 页，免得显示成「第 1 / 0 页」。
export function pageCount(total: number, pageSize: number): number {
  return Math.max(1, Math.ceil(total / pageSize))
}

/// 删掉当前页最后几条之后，offset 可能已经越过末页（那一页现在是空的）。
/// 返回应该退回去的 offset；没越界时原样返回。
export function clampOffset(offset: number, total: number, pageSize: number): number {
  const lastOffset = (pageCount(total, pageSize) - 1) * pageSize
  return Math.min(Math.max(0, offset), lastOffset)
}
