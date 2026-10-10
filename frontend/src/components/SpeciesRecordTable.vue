<script setup lang="ts">
import { ref } from 'vue'
import type { SpeciesRecord } from '../taxonomy'
import NoteText from './NoteText.vue'
import SpeciesCard from './SpeciesCard.vue'

/// 名录记录的只读表格，名录页和物种检索页共用。
/// 操作列的按钮由调用方通过 `actions` 插槽提供（检索页不传就没有这一列）。
/// 学名 / 中文名可点击，打开 SpeciesCard 看完整物种信息。
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

const card = ref<InstanceType<typeof SpeciesCard> | null>(null)

function openCard(record: SpeciesRecord): void {
  card.value?.open(record)
}
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
          <td class="nowrap">
            <button class="name-link" type="button" @click="openCard(record)">
              {{ record.scientific_name }}
            </button>
          </td>
          <td class="nowrap">
            <button
              v-if="record.chinese_name !== null"
              class="name-link"
              type="button"
              @click="openCard(record)"
            >
              {{ record.chinese_name }}
            </button>
            <span v-else class="muted">—</span>
          </td>
          <td v-if="listName" class="muted">{{ listName(record.list_id) }}</td>
          <td class="muted">
            <span class="clip" :title="record.distribution ?? undefined">
              {{ record.distribution ?? '—' }}
            </span>
          </td>
          <td class="muted">
            <NoteText :note="record.note" clip />
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

  <SpeciesCard ref="card" :list-name="listName" />
</template>
