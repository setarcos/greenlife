<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { errorMessage } from '../api/http'
import { listRecords, listSpeciesLists } from '../api/taxonomy'
import ListPager from '../components/ListPager.vue'
import SpeciesRecordTable from '../components/SpeciesRecordTable.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import type { SpeciesCardTarget } from '../components/SpeciesCard.vue'
import { clampOffset } from '../pagination'
import { pageOffset, queryPage, queryText, setPage, setText } from '../query'
import type { SpeciesList, SpeciesRecord } from '../taxonomy'

const PAGE_SIZE = 50

const route = useRoute()
const router = useRouter()

const lists = ref<SpeciesList[]>([])

/// 筛选条件。空串 / null 都表示「不限」。
const listId = ref('')
/// 从分类树详情点进来时带着 `?taxon=<id>`，直接选中该类群。
const taxonId = ref(
  typeof route.query.taxon === 'string' && route.query.taxon !== '' ? route.query.taxon : null,
)
const descendants = ref(true)
/// 输入框里的关键字和真正用于查询的关键字分开，避免边打字边发请求。
const keyword = ref('')
const appliedKeyword = ref('')
/// 重建 TaxonPicker 用（重置时清掉已选类群）。
const pickerKey = ref(0)
/// 「高级搜索」（类群选择）默认折叠；从分类树带 `?taxon=` 进来时自动展开。
const advancedOpen = ref(taxonId.value !== null)

function onAdvancedToggle(event: Event): void {
  advancedOpen.value = (event.target as HTMLDetailsElement).open
}

const records = ref<SpeciesRecord[]>([])
const loading = ref(false)
const error = ref('')
const offset = ref(0)
const total = ref(0)
const searched = ref(false)

function listName(id: string): string {
  return lists.value.find((list) => list.id === id)?.name ?? id
}

/// 卡片里的「编辑」：检索结果只读，跳到「物种名录」页去编这条记录。
function editRecordFromCard(target: SpeciesCardTarget): void {
  if (target.id === undefined) {
    return
  }
  void router.push({ name: 'species-lists', query: { record: target.id } })
}

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const page = await listRecords({
      listId: listId.value === '' ? undefined : listId.value,
      taxonId: taxonId.value ?? undefined,
      // descendants 只在选了类群时才有意义。
      descendants: taxonId.value === null ? undefined : descendants.value,
      q: appliedKeyword.value === '' ? undefined : appliedKeyword.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    })
    // 筛选条件变了之后当前页可能已经越过末页，退回去重拉一次。
    const clamped = clampOffset(offset.value, page.total, PAGE_SIZE)
    if (clamped !== offset.value) {
      offset.value = clamped
      writeQuery()
      await load()
      return
    }
    records.value = page.items
    total.value = page.total
    searched.value = true
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  // 先把地址栏里的搜索条件读回来，再按它去请求数据。
  applyRouteQueryState()
  try {
    lists.value = await listSpeciesLists()
  } catch (e) {
    error.value = errorMessage(e)
  }
  await load()
})

// 地址栏变了但组件没重建（浏览器前进后退、点顶栏的「物种检索」）时跟着变。
watch(
  () => route.query,
  async () => {
    if (applyRouteQueryState()) {
      await load()
    }
  },
)

// ==========================================================================
// 视角状态 ↔ 地址栏
// ==========================================================================

/// 搜索条件（名录 / 类群 / 关键字 / 页码）写进地址栏：从结果的卡片跳到别的页面再按
/// 后退、刷新页面、把链接发给别人，都能回到同一屏。只写非默认值，所以没筛选时链接
/// 还是干净的 `/search`。
/// 写在搜索 / 重置 / 翻页时，而不是下拉框一变就写：地址栏里存的始终是「出这一屏结果
/// 的那套条件」。
function writeQuery(): void {
  const query: LocationQueryRaw = {}
  setText(query, 'list', listId.value)
  setText(query, 'taxon', taxonId.value ?? '')
  if (taxonId.value !== null && !descendants.value) {
    query.descendants = '0'
  }
  setText(query, 'q', appliedKeyword.value)
  setPage(query, 'page', offset.value, PAGE_SIZE)
  // replace：调一次筛选不该在历史里多留一条。
  void router.replace({ query })
}

/// 从地址栏读回搜索条件；返回是否和当前状态不同（我们自己 replace 出去的 query
/// 读回来是一样的，那种情况就不用重新请求）。
function applyRouteQueryState(): boolean {
  const query = route.query
  const nextList = queryText(query.list)
  const nextTaxon = queryText(query.taxon)
  const nextDescendants = queryText(query.descendants) !== '0'
  const nextKeyword = queryText(query.q)
  const nextOffset = pageOffset(queryPage(query.page), PAGE_SIZE)
  if (
    nextList === listId.value &&
    nextTaxon === (taxonId.value ?? '') &&
    nextDescendants === descendants.value &&
    nextKeyword === appliedKeyword.value &&
    nextOffset === offset.value
  ) {
    return false
  }
  listId.value = nextList
  taxonId.value = nextTaxon === '' ? null : nextTaxon
  descendants.value = nextDescendants
  keyword.value = nextKeyword
  appliedKeyword.value = nextKeyword
  offset.value = nextOffset
  // 类群选择器是内部持有状态的组件，重置它才能和地址栏对上。
  pickerKey.value += 1
  advancedOpen.value = nextTaxon !== ''
  return true
}

async function search(): Promise<void> {
  appliedKeyword.value = keyword.value.trim()
  offset.value = 0
  writeQuery()
  await load()
}

async function reset(): Promise<void> {
  listId.value = ''
  taxonId.value = null
  descendants.value = true
  keyword.value = ''
  appliedKeyword.value = ''
  offset.value = 0
  pickerKey.value += 1
  advancedOpen.value = false
  writeQuery()
  await load()
}

async function goToPage(page: number): Promise<void> {
  offset.value = (page - 1) * PAGE_SIZE
  writeQuery()
  await load()
}
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <h2>物种检索</h2>
        <div class="head-actions">
          <button class="secondary" type="button" :disabled="loading" @click="reset">重置</button>
        </div>
      </div>

      <form class="search-form" @submit.prevent="search">
        <div class="field">
          <label for="search-list">名录</label>
          <select id="search-list" v-model="listId">
            <option value="">全部名录</option>
            <option v-for="list in lists" :key="list.id" :value="list.id">{{ list.name }}</option>
          </select>
        </div>

        <div class="field">
          <label for="search-keyword">关键字</label>
          <input id="search-keyword" v-model="keyword" placeholder="学名或中文名…" />
        </div>

        <details class="advanced-search" :open="advancedOpen" @toggle="onAdvancedToggle">
          <summary>
            <span>高级搜索：类群</span>
            <span v-if="taxonId !== null" class="tag">已筛选</span>
            <span class="advanced-toggle">
              <span class="when-closed">展开</span>
              <span class="when-open">收起</span>
            </span>
          </summary>
          <div class="advanced-search-body">
            <p class="hint">逐级选择，选到哪一级就按哪一级筛选；不选则不限类群。</p>
            <TaxonPicker :key="pickerKey" v-model="taxonId" />
            <label class="checkbox">
              <input type="checkbox" v-model="descendants" :disabled="taxonId === null" />
              包含下级类群
            </label>
          </div>
        </details>

        <div class="actions">
          <button type="submit" :disabled="loading">{{ loading ? '查询中…' : '搜索' }}</button>
        </div>
      </form>
    </section>

    <section class="card">
      <div class="card-head">
        <h2>结果</h2>
        <span class="muted">本页 {{ records.length }} 条</span>
      </div>

      <p v-if="error" class="alert error">{{ error }}</p>

      <SpeciesRecordTable
        :records="records"
        :loading="loading"
        :list-name="listName"
        hide-list-column
        :on-edit-record="editRecordFromCard"
      >
        <template #empty>
          <span v-if="searched">没有符合条件的记录</span>
          <span v-else>还没有查询</span>
        </template>
      </SpeciesRecordTable>

      <ListPager
        v-if="searched"
        :total="total"
        :page-size="PAGE_SIZE"
        :page="offset / PAGE_SIZE + 1"
        :disabled="loading"
        @change="goToPage"
      />
    </section>
  </div>
</template>
