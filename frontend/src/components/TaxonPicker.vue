<script setup lang="ts">
import { computed, onMounted, ref, useId, watch } from 'vue'
import { errorMessage } from '../api/http'
import { getTaxon, listTaxa } from '../api/taxonomy'
import { RANKS, rankDepth, rankLabel, taxonLabel, type Taxon, type TaxonomyRank } from '../taxonomy'

/// 逐级下拉框选分类节点（docs/物种分类后台.md §2.1）：
/// 门 → 纲 → 目 → 科 → 属 → 种，每一级的候选是「上一级已选节点的直接子节点」。
///
/// 后端允许跳级（属可以直接挂在纲下），所以某一级的下拉框可能是空的，
/// 但下面几级仍然能列出「跳过中间阶元」的节点 —— 候选查的是
/// `?rank=<本级>&parent_id=<最近一个已选祖先>`，而不是上一级。
///
/// `modelValue` 是选中的最深节点 id；没有选中任何一级时为 `null`。
const props = defineProps<{
  modelValue: string | null
  /// 只提供阶元严格高于它的层级。新增分类节点时传该节点的阶元；
  /// 不传则一路提供到「种」（名录记录选鉴定阶元时用）。
  maxRank?: TaxonomyRank
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [string | null] }>()

const uid = useId()

const ranks = computed(() => {
  const max = props.maxRank
  return max === undefined ? RANKS : RANKS.filter((rank) => rankDepth(rank) < rankDepth(max))
})

const selected = ref<Partial<Record<TaxonomyRank, string>>>({})
const options = ref<Partial<Record<TaxonomyRank, Taxon[]>>>({})
const keyword = ref('')
const loading = ref(false)
const error = ref('')

/// 每一级的候选只取决于「最近一个已选祖先」，所以跳级时也能列出候选。
function deepestParentId(rank: TaxonomyRank): string {
  const higher = ranks.value.filter((r) => rankDepth(r) < rankDepth(rank))
  for (let i = higher.length - 1; i >= 0; i -= 1) {
    const id = selected.value[higher[i]]
    if (id !== undefined) {
      return id
    }
  }
  return 'root'
}

function clearDeeperThan(rank: TaxonomyRank): void {
  for (const r of ranks.value) {
    if (rankDepth(r) > rankDepth(rank)) {
      delete selected.value[r]
    }
  }
}

function currentValue(): string | null {
  let value: string | null = null
  for (const rank of ranks.value) {
    value = selected.value[rank] ?? value
  }
  return value
}

/// 加一个请求序号，避免慢请求把新选择的结果覆盖掉。
let requestId = 0

async function loadOptions(): Promise<void> {
  const token = ++requestId
  loading.value = true
  error.value = ''
  try {
    const next: Partial<Record<TaxonomyRank, Taxon[]>> = {}
    for (const rank of ranks.value) {
      const items = await listTaxa({ rank, parentId: deepestParentId(rank), limit: 1000 })
      if (token !== requestId) {
        return
      }
      next[rank] = items
      // 父级变了以后，原来的选中值可能已经不在候选里，清掉它和更深的层级。
      const chosen = selected.value[rank]
      if (chosen !== undefined && !items.some((item) => item.id === chosen)) {
        delete selected.value[rank]
        clearDeeperThan(rank)
      }
    }
    options.value = next
  } catch (e) {
    if (token === requestId) {
      error.value = errorMessage(e)
    }
  } finally {
    if (token === requestId) {
      loading.value = false
    }
  }
}

async function initializeFrom(taxonId: string): Promise<void> {
  const detail = await getTaxon(taxonId)
  const chain = [...detail.path, detail]
  const next: Partial<Record<TaxonomyRank, string>> = {}
  for (const node of chain) {
    if (ranks.value.includes(node.rank)) {
      next[node.rank] = node.id
    }
  }
  selected.value = next
}

async function select(rank: TaxonomyRank, event: Event): Promise<void> {
  const value = (event.target as HTMLSelectElement).value
  if (value === '') {
    delete selected.value[rank]
  } else {
    selected.value[rank] = value
  }
  clearDeeperThan(rank)
  emit('update:modelValue', currentValue())
  await loadOptions()
}

async function clear(): Promise<void> {
  selected.value = {}
  emit('update:modelValue', null)
  await loadOptions()
}

function matches(rank: TaxonomyRank): Taxon[] {
  const items = options.value[rank] ?? []
  const kw = keyword.value.trim().toLowerCase()
  if (kw === '') {
    return items
  }
  return items.filter((item) => taxonLabel(item).toLowerCase().includes(kw))
}

const selectedLabel = computed(() => {
  let label = ''
  for (const rank of ranks.value) {
    const id = selected.value[rank]
    if (id === undefined) {
      continue
    }
    const item = (options.value[rank] ?? []).find((t) => t.id === id)
    label = item === undefined ? id : taxonLabel(item)
  }
  return label
})

onMounted(async () => {
  if (props.modelValue !== null) {
    try {
      await initializeFrom(props.modelValue)
    } catch (e) {
      error.value = errorMessage(e)
    }
  }
  await loadOptions()
})

// 外部把 modelValue 清掉/换成别的节点（例如表单重置）时同步过来。
// 自己 emit 出去再回流的值等于 currentValue()，直接忽略，避免多打一次请求。
watch(
  () => props.modelValue,
  (next) => {
    if (next === currentValue()) {
      return
    }
    void (async () => {
      if (next === null) {
        selected.value = {}
      } else {
        try {
          await initializeFrom(next)
        } catch (e) {
          error.value = errorMessage(e)
          return
        }
      }
      await loadOptions()
    })()
  },
)
</script>

<template>
  <div class="picker">
    <p v-if="error" class="alert error">{{ error }}</p>

    <div class="field">
      <label :for="`${uid}-filter`">筛选候选（按名字过滤下拉框，可选）</label>
      <input
        :id="`${uid}-filter`"
        v-model="keyword"
        :disabled="disabled"
        placeholder="输入学名或中文名…"
      />
    </div>

    <div v-for="rank in ranks" :key="rank" class="field">
      <label :for="`${uid}-${rank}`">{{ rankLabel(rank) }}</label>
      <select
        :id="`${uid}-${rank}`"
        :value="selected[rank] ?? ''"
        :disabled="disabled"
        @change="select(rank, $event)"
      >
        <option value="">（未选）</option>
        <option v-for="item in matches(rank)" :key="item.id" :value="item.id">
          {{ taxonLabel(item) }}
        </option>
      </select>
    </div>

    <div class="picker-foot">
      <span class="hint">
        <span v-if="loading">加载中…</span>
        <span v-else>已选：{{ selectedLabel === '' ? '未选' : selectedLabel }}</span>
      </span>
      <button class="secondary" type="button" :disabled="disabled || loading" @click="clear">
        清空选择
      </button>
    </div>
  </div>
</template>
