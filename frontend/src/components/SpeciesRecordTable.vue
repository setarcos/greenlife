<script setup lang="ts">
import type { SpeciesRecord } from '../taxonomy'

/// 名录记录的只读表格，名录页和物种检索页共用。
/// 操作列的按钮由调用方通过 `actions` 插槽提供（检索页不传就没有这一列）。
defineProps<{
  records: SpeciesRecord[]
  loading?: boolean
  /// 传了才显示「名录」列；检索结果是跨名录的，需要它。
  listName?: (listId: string) => string
}>()

defineSlots<{
  actions?: (props: { record: SpeciesRecord }) => unknown
  empty?: () => unknown
}>()
</script>

<template>
  <p v-if="loading" class="muted">加载中…</p>

  <div v-else class="table-wrap">
    <table class="table">
      <thead>
        <tr>
          <th>学名</th>
          <th>中文名</th>
          <th v-if="listName">名录</th>
          <th>分布</th>
          <th>备注</th>
          <th>来源</th>
          <th>编号</th>
          <th v-if="$slots.actions"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="record in records" :key="record.id">
          <!-- 学名 / 中文名完整显示不换行；备注 / 来源不换行，超出部分省略号 + hover 显示全文 -->
          <td class="nowrap">{{ record.scientific_name }}</td>
          <td class="nowrap">{{ record.chinese_name ?? '—' }}</td>
          <td v-if="listName" class="muted">{{ listName(record.list_id) }}</td>
          <td class="muted">{{ record.distribution ?? '—' }}</td>
          <td class="muted">
            <span class="clip" :title="record.note ?? undefined">{{ record.note ?? '—' }}</span>
          </td>
          <td class="muted">
            <span class="clip" :title="record.source ?? undefined">
              {{ record.source ?? '—' }}
            </span>
          </td>
          <td class="muted mono">{{ record.record_no ?? '—' }}</td>
          <td v-if="$slots.actions" class="actions-cell">
            <slot name="actions" :record="record" />
          </td>
        </tr>
        <tr v-if="records.length === 0">
          <td :colspan="6 + (listName ? 1 : 0) + ($slots.actions ? 1 : 0)" class="muted">
            <slot name="empty">没有记录</slot>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
