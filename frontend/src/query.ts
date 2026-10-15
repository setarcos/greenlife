/// 地址栏 query 的读写工具。
///
/// 列表页把「看的是哪一页 / 筛选了什么 / 选中了哪一项」这类**视角状态**写进 URL：
/// 从卡片跳到别的页面再按后退、刷新页面、把链接发给别人，都能回到同一屏。
///
/// 约定：
/// - 只写非默认值（第 1 页、空筛选词不进 URL），所以平时的链接还是干净的；
/// - 页码参数里放的是「第几页」而不是 offset，和屏幕上的数字一致；
/// - 调用方一律用 `router.replace` —— 调一次筛选不该在历史里多留一条。

import type { LocationQueryRaw } from 'vue-router'

/// query 里的字符串参数。数组（`?a=1&a=2`）和 null 都当作没给。
export function queryText(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

/// query 里的页码，从 1 开始；认不出（空 / 非数字 / 0）就当第 1 页。
export function queryPage(value: unknown): number {
  const text = queryText(value)
  return /^\d+$/.test(text) ? Math.max(1, Number(text)) : 1
}

/// 页码 -> 分页接口要的 offset。
export function pageOffset(page: number, pageSize: number): number {
  return (page - 1) * pageSize
}

/// offset -> 页码。
export function pageNumber(offset: number, pageSize: number): number {
  return Math.floor(offset / pageSize) + 1
}

/// 写页码：第 1 页是默认值，不进 URL。
export function setPage(
  query: LocationQueryRaw,
  key: string,
  offset: number,
  pageSize: number,
): void {
  if (offset > 0) {
    query[key] = String(pageNumber(offset, pageSize))
  }
}

/// 写文本：空串是默认值，不进 URL。
export function setText(query: LocationQueryRaw, key: string, value: string): void {
  if (value !== '') {
    query[key] = value
  }
}
