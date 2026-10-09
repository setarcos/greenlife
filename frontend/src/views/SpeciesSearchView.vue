<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { errorMessage } from '../api/http'
import { listRecords, listSpeciesLists } from '../api/taxonomy'
import SpeciesRecordTable from '../components/SpeciesRecordTable.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import type { SpeciesList, SpeciesRecord } from '../taxonomy'

const PAGE_SIZE = 50

const lists = ref<SpeciesList[]>([])

/// 筛选条件。空串 / null 都表示「不限」。
const listId = ref('')
const taxonId = ref<string | null>(null)
const descendants = ref(true)
/// 输入框里的关键字和真正用于查询的关键字分开，避免边打字边发请求。
const keyword = ref('')
const appliedKeyword = ref('')
/// 重建 TaxonPicker 用（重置时清掉已选类群）。
const pickerKey = ref(0)

const records = ref<SpeciesRecord[]>([])
const loading = ref(false)
const error = ref('')
const offset = ref(0)
const hasMore = ref(false)
const searched = ref(false)

function listName(id: string): string {
  return lists.value.find((list) => list.id === id)?.name ?? id
}

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    records.value = await listRecords({
      listId: listId.value === '' ? undefined : listId.value,
      taxonId: taxonId.value ?? undefined,
      // descendants 只在选了类群时才有意义。
      descendants: taxonId.value === null ? undefined : descendants.value,
      q: appliedKeyword.value === '' ? undefined : appliedKeyword.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    })
    hasMore.value = records.value.length === PAGE_SIZE
    searched.value = true
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  try {
    lists.value = await listSpeciesLists()
  } catch (e) {
    error.value = errorMessage(e)
  }
  await load()
})

async function search(): Promise<void> {
  appliedKeyword.value = keyword.value.trim()
  offset.value = 0
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
  await load()
}

async function prevPage(): Promise<void> {
  offset.value = Math.max(0, offset.value - PAGE_SIZE)
  await load()
}

async function nextPage(): Promise<void> {
  offset.value += PAGE_SIZE
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

        <div class="field">
          <label>类群（可选，逐级选；选到哪一级就按哪一级筛）</label>
          <TaxonPicker :key="pickerKey" v-model="taxonId" />
          <label class="checkbox">
            <input type="checkbox" v-model="descendants" :disabled="taxonId === null" />
            包含下级类群
          </label>
        </div>

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

      <SpeciesRecordTable :records="records" :loading="loading" :list-name="listName">
        <template #empty>
          <span v-if="searched">没有符合条件的记录</span>
          <span v-else>还没有查询</span>
        </template>
      </SpeciesRecordTable>

      <div v-if="searched" class="pager">
        <button class="secondary" type="button" :disabled="offset === 0" @click="prevPage">
          上一页
        </button>
        <span class="muted">第 {{ offset / PAGE_SIZE + 1 }} 页</span>
        <button class="secondary" type="button" :disabled="!hasMore" @click="nextPage">
          下一页
        </button>
      </div>
    </section>
  </div>
</template>
