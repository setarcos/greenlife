<script setup lang="ts">
/// 备注的统一渲染：把里面的 iNaturalist 链接渲染成可点击的 tag。
/// 表格里用 `clip`（单行省略号），物种卡片里不传（完整换行、不截断）。

/// 备注里的链接（`scripts/up.sql` 里约 140 条形如「记录：https://…/observations/123」）。
/// 链接后面常紧跟中文标点和说明文字，所以不能用 `\S` 收尾，得排除空白和常见分隔符。
const INATURALIST_LINK = /https?:\/\/(?:www\.)?inaturalist\.org\/[^\s；;，,、）)】」』"'<>]+/

type NotePart = { text: string; url: null } | { text: null; url: string }

/// 把备注切成「普通文字」和「iNaturalist 链接」片段。
function noteParts(note: string): NotePart[] {
  const parts: NotePart[] = []
  let rest = note
  for (;;) {
    const match = INATURALIST_LINK.exec(rest)
    if (match === null) break
    if (match.index > 0) parts.push({ text: rest.slice(0, match.index), url: null })
    parts.push({ text: null, url: match[0] })
    rest = rest.slice(match.index + match[0].length)
  }
  if (rest !== '') parts.push({ text: rest, url: null })
  return parts
}

defineProps<{
  note: string | null
  /// 单行省略号显示（表格单元格）；完整显示时不用传。
  clip?: boolean
}>()
</script>

<template>
  <span v-if="!note" :class="clip ? 'clip' : null">—</span>
  <span v-else :class="clip ? 'note' : 'note-full'" :title="note">
    <template v-for="(part, index) in noteParts(note)" :key="index">
      <a
        v-if="part.url"
        class="tag"
        :href="part.url"
        :title="part.url"
        target="_blank"
        rel="noopener noreferrer"
      >
        iNaturalist
      </a>
      <span v-else :class="clip ? 'clip' : null">{{ part.text }}</span>
    </template>
  </span>
</template>
