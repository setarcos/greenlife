<script setup lang="ts">
import { computed, ref } from 'vue'
import { errorMessage } from '../api/http'
import { getTaxon } from '../api/taxonomy'
import { RANKS, rankLabel, type SpeciesRecord, type Taxon } from '../taxonomy'
import { formatTimestamp } from '../types'
import NoteText from './NoteText.vue'

/// 物种信息卡片：点表格里的学名 / 中文名打开。
/// 记录本身自带分布 / 备注 / 来源，分类阶元要从 `/taxonomy/taxa/{id}` 取祖先链。
defineProps<{
  /// 和 SpeciesRecordTable 同一个函数：传了才显示「名录」。
  listName?: (listId: string) => string
}>()

const dialog = ref<HTMLDialogElement | null>(null)
const record = ref<SpeciesRecord | null>(null)
const chain = ref<Taxon[]>([])
const loading = ref(false)
const error = ref('')

/// 分类树很小且基本不变，同一 taxa 的祖先链缓存下来，反复点开不再请求。
const chainCache = new Map<string, Taxon[]>()
/// 连点两条记录时，只认最后一次请求的结果。
let requestSeq = 0

/// 界 > 门 > 纲 > 目 > 科 > 属 > 种 固定七行，链上缺的阶元（允许跳级）显示「—」。
const rankRows = computed(() =>
  RANKS.map((rank) => ({
    rank,
    label: rankLabel(rank),
    taxon: chain.value.find((node) => node.rank === rank) ?? null,
  })),
)

async function open(target: SpeciesRecord): Promise<void> {
  const seq = (requestSeq += 1)
  record.value = target
  chain.value = []
  error.value = ''
  loading.value = false
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }

  const cached = chainCache.get(target.taxon_id)
  if (cached !== undefined) {
    chain.value = cached
    return
  }

  loading.value = true
  try {
    const detail = await getTaxon(target.taxon_id)
    const next = [...detail.path, detail]
    chainCache.set(target.taxon_id, next)
    if (seq !== requestSeq) return
    chain.value = next
  } catch (e) {
    if (seq !== requestSeq) return
    error.value = errorMessage(e)
  } finally {
    if (seq === requestSeq) loading.value = false
  }
}

function close(): void {
  dialog.value?.close()
}

defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="modal species-card" @click.self="close">
    <template v-if="record">
      <div class="species-head">
        <div>
          <h2>{{ record.chinese_name ?? record.scientific_name }}</h2>
          <p class="species-sci">
            <i>{{ record.scientific_name }}</i>
            <span v-if="record.chinese_name === null" class="muted">（暂无中文名）</span>
          </p>
        </div>
        <button class="secondary" type="button" @click="close">关闭</button>
      </div>

      <h3>分类阶元</h3>
      <p v-if="error" class="alert error">{{ error }}</p>
      <p v-else-if="loading" class="muted">加载中…</p>
      <div v-else class="rank-list">
        <div v-for="row in rankRows" :key="row.rank" class="rank-row">
          <span class="tag rank-tag">{{ row.label }}</span>
          <span v-if="row.taxon === null" class="muted">—</span>
          <span v-else class="rank-name">
            <span v-if="row.taxon.chinese_name !== null">{{ row.taxon.chinese_name }} </span>
            <i class="muted">{{ row.taxon.scientific_name }}</i>
          </span>
        </div>
      </div>

      <h3>记录信息</h3>
      <div class="info-list">
        <div v-if="listName" class="info-row">
          <span class="info-label">名录</span>
          <span>{{ listName(record.list_id) }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">分布</span>
          <span>{{ record.distribution ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">备注</span>
          <NoteText :note="record.note" />
        </div>
        <div class="info-row">
          <span class="info-label">来源</span>
          <span>{{ record.source ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">编号</span>
          <span class="mono">{{ record.record_no ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">更新</span>
          <span class="muted">{{ formatTimestamp(record.updated_at) }}</span>
        </div>
      </div>
    </template>
  </dialog>
</template>
