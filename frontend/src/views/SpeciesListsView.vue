<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { errorMessage } from '../api/http'
import { getRecord, getSpeciesList, listRecords, listSpeciesLists } from '../api/taxonomy'
import {
  createRecord,
  createSpeciesList,
  deleteRecord,
  deleteSpeciesList,
  updateRecord,
  updateSpeciesList,
} from '../api/staffTaxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import ListPager from '../components/ListPager.vue'
import SpeciesRecordTable from '../components/SpeciesRecordTable.vue'
import type { SpeciesCardTarget } from '../components/SpeciesCard.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import { useAuthStore } from '../stores/auth'
import { clampOffset } from '../pagination'
import { pageOffset, queryPage, queryText, setPage, setText } from '../query'
import type { SpeciesList, SpeciesListDetail, SpeciesRecord } from '../taxonomy'
import {
  LIST_NAME_MAX_CHARS,
  RECORD_NO_MAX_CHARS,
  TAXON_NAME_MAX_CHARS,
  charCount,
  validateField,
} from '../validation'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const canManage = computed(() => auth.canManage)

const PAGE_SIZE = 50

const lists = ref<SpeciesList[]>([])
const listsLoading = ref(false)
const listsError = ref('')

const selectedId = ref<string | null>(null)
const detail = ref<SpeciesListDetail | null>(null)

const records = ref<SpeciesRecord[]>([])
const recordsLoading = ref(false)
const recordsError = ref('')

const keyword = ref('')
const appliedKeyword = ref('')
const offset = ref(0)
/// 满足筛选条件的总条数（后端分页响应的 total），页码条靠它算总页数。
const recordsTotal = ref(0)

/// 类群筛选：null 表示不限类群；筛的是**当前这份名录**里的记录（跨名录的检索已并到这里，
/// 只保留了「先选名录、再按类群筛」这一条路）。换名录时不重置它 —— 从分类树
/// 「查看物种」进来后想换一份名录接着看同一个类群，不用重新选一遍。
const taxonId = ref<string | null>(null)
/// 是否连所选类群的下级一起算（后端 `descendants`）。
const descendants = ref(true)
/// 重建 TaxonPicker 用：选择器内部持有状态，从地址栏恢复筛选时要把它丢掉。
const pickerKey = ref(0)
/// 类群选择默认折叠；带 `?taxon=`（分类树的「查看物种」）进来时展开。
const taxonOpen = ref(false)

function onTaxonToggle(event: Event): void {
  taxonOpen.value = (event.target as HTMLDetailsElement).open
}

/// 操作成功后 modal 已关，提示回显在卡片里。
const savedMessage = ref('')

const selectedList = computed(
  () => lists.value.find((list) => list.id === selectedId.value) ?? null,
)

/// 空串统一转成 null（后端的可选字段用 null 表示清空）。
function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed === '' ? null : trimmed
}

async function loadDetail(): Promise<void> {
  const id = selectedId.value
  if (id === null) {
    detail.value = null
    return
  }
  try {
    detail.value = await getSpeciesList(id)
  } catch (e) {
    recordsError.value = errorMessage(e)
  }
}

async function loadRecords(): Promise<void> {
  const id = selectedId.value
  if (id === null) {
    records.value = []
    recordsTotal.value = 0
    return
  }
  recordsLoading.value = true
  recordsError.value = ''
  try {
    const page = await listRecords({
      listId: id,
      taxonId: taxonId.value ?? undefined,
      // descendants 只在选了类群时才有意义。
      descendants: taxonId.value === null ? undefined : descendants.value,
      q: appliedKeyword.value === '' ? undefined : appliedKeyword.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    })
    // 删记录后当前页可能已经越过末页，退回去重拉一次（clampOffset 幂等）。
    const clamped = clampOffset(offset.value, page.total, PAGE_SIZE)
    if (clamped !== offset.value) {
      offset.value = clamped
      writeQuery()
      await loadRecords()
      return
    }
    records.value = page.items
    recordsTotal.value = page.total
  } catch (e) {
    recordsError.value = errorMessage(e)
  } finally {
    recordsLoading.value = false
  }
}

/// 点左边的名录：搜索和分页回到开头，否则会把上一份名录的关键字带过来。
async function selectList(id: string): Promise<void> {
  savedMessage.value = ''
  await showList(id, 0, '')
}

/// `preferId` 用于新增/改名后把选中项停在刚操作的那一份上；`view` 是从地址栏
/// 恢复时的页码和搜索词，其它入口一律回到开头。
async function loadLists(
  preferId?: string,
  view?: { offset: number; keyword: string },
): Promise<void> {
  listsLoading.value = true
  listsError.value = ''
  try {
    lists.value = await listSpeciesLists()
    // 选了类群时，名录要跟着类群走（见 moveToListWithTaxon 的说明）：当前这份里有
    // 该类群就留下，否则直接选中第一份有的 —— 先加载一份再跳走会闪一下。
    const withTaxon = await listIdsWithTaxon()
    let wanted = preferId ?? selectedId.value
    if (withTaxon !== null && withTaxon.length > 0 && !withTaxon.includes(wanted ?? '')) {
      wanted = withTaxon[0]
    }
    if (wanted !== null && lists.value.some((list) => list.id === wanted)) {
      await showList(wanted, view?.offset ?? 0, view?.keyword ?? '')
    } else if (lists.value.length > 0) {
      await showList(lists.value[0].id, 0, '')
    } else {
      selectedId.value = null
      detail.value = null
      records.value = []
      // 一份名录都不剩（或者选的那份被删了）：地址栏里的 list 也得撤掉。
      writeQuery()
    }
  } catch (e) {
    listsError.value = errorMessage(e)
  } finally {
    listsLoading.value = false
  }
}

// ==========================================================================
// 视角状态 ↔ 地址栏
// ==========================================================================

/// 选中哪份名录、翻到第几页、搜了什么，全部写进地址栏：从这里的卡片跳到别的页面再
/// 按后退、刷新页面、把链接发给别人，都能回到同一屏。只写非默认值，所以没筛选时
/// 链接还是干净的 `/species-lists`。
function writeQuery(): void {
  const query: LocationQueryRaw = {}
  if (selectedId.value !== null) {
    query.list = selectedId.value
  }
  setPage(query, 'page', offset.value, PAGE_SIZE)
  setText(query, 'q', appliedKeyword.value)
  setText(query, 'taxon', taxonId.value ?? '')
  if (taxonId.value !== null && !descendants.value) {
    query.descendants = '0'
  }
  // replace：调一次筛选不该在历史里多留一条。另外这也顺手把 `?record=` 从
  // 地址栏里抹掉（进来时已经用完了）。
  void router.replace({ query })
}

/// 地址栏里的选中名录 / 页码 / 搜索词 / 类群筛选。
function viewFromRoute(): {
  listId: string
  offset: number
  keyword: string
  taxonId: string
  descendants: boolean
} {
  return {
    listId: queryText(route.query.list),
    offset: pageOffset(queryPage(route.query.page), PAGE_SIZE),
    keyword: queryText(route.query.q),
    taxonId: queryText(route.query.taxon),
    descendants: queryText(route.query.descendants) !== '0',
  }
}

/// 把地址栏里的类群筛选同步进状态；有变化才重建选择器（它是内部持有状态的组件）。
function applyTaxon(view: { taxonId: string; descendants: boolean }): boolean {
  const nextTaxon = view.taxonId === '' ? null : view.taxonId
  if (nextTaxon === taxonId.value && view.descendants === descendants.value) {
    return false
  }
  taxonId.value = nextTaxon
  descendants.value = view.descendants
  pickerKey.value += 1
  taxonOpen.value = nextTaxon !== null
  return true
}

/// 有该类群记录的名录 id（按名录顺序，即 `/taxonomy/lists` 的排序）。
/// 查不出来时返回 null，调用方当作「不知道」，不去跳名录，也不打断整页加载。
async function listIdsWithTaxon(): Promise<string[] | null> {
  const taxon = taxonId.value
  if (taxon === null) {
    return []
  }
  try {
    const matched = await listSpeciesLists({ taxonId: taxon, descendants: descendants.value })
    return matched.map((list) => list.id)
  } catch {
    return null
  }
}

/// 选中类群后要把「当前这份名录」换到有该类群记录的那一份上（优先留在当前这份）：
/// 在高等植物名录里选鸟纲不该对着空列表，而是跳到有鸟的脊椎动物名录。
/// 返回是否换了名录（换了就得多拉一次名录详情）。
async function moveToListWithTaxon(): Promise<boolean> {
  const ids = await listIdsWithTaxon()
  if (ids === null || ids.length === 0) {
    return false
  }
  if (selectedId.value !== null && ids.includes(selectedId.value)) {
    return false
  }
  selectedId.value = ids[0]
  return true
}

/// 换一份名录 / 换一页 / 换搜索词：更新状态、写回地址栏、重新加载。
async function showList(id: string, nextOffset: number, nextKeyword: string): Promise<void> {
  selectedId.value = id
  keyword.value = nextKeyword
  appliedKeyword.value = nextKeyword
  offset.value = nextOffset
  writeQuery()
  await Promise.all([loadDetail(), loadRecords()])
}

/// 地址栏变了就跟着变：选中名录 / 页码 / 搜索词三者任一不同才重新加载。
/// 我们自己 replace 出去的 query 就是当前状态，读出来一样，所以不会重复请求。
async function syncFromRoute(): Promise<void> {
  const view = viewFromRoute()
  // 类群筛选完全由地址栏驱动（分类树的「查看物种」就是靠 `?taxon=` 传进来的）。
  const taxonChanged = applyTaxon(view)
  // 地址栏里没写名录（比如点顶栏重新进本页）就当还是现在这份。
  const viewId = view.listId === '' ? selectedId.value : view.listId
  if (viewId === null) {
    return
  }
  if (
    viewId === selectedId.value &&
    view.offset === offset.value &&
    view.keyword === appliedKeyword.value
  ) {
    // 只变了类群（浏览器前进 / 后退改的就是它）：不用重选名录，重拉记录即可。
    if (taxonChanged) {
      await loadRecords()
    }
    return
  }
  if (viewId !== selectedId.value) {
    // 名录可能已经不在列表里（被删了 / 链接写错了），交给 loadLists 兜底选一份。
    await loadLists(viewId, view)
    return
  }
  await showList(viewId, view.offset, view.keyword)
}

onMounted(async () => {
  // 从「鸟种清单」的卡片点「编辑」跳过来：`?record=<id>`。
  // 先按 id 取记录（顺便知道它属于哪份名录），选中那份名录后弹出编辑框。
  // 从记录链进来时不用管页码，编辑框是个弹窗，不依赖列表翻到哪一页。
  const record = await recordFromQuery()
  const view = viewFromRoute()
  // 类群筛选在加载记录之前落地，第一屏就带上它。
  applyTaxon(view)
  await loadLists(
    record?.list_id ?? (view.listId === '' ? undefined : view.listId),
    record === null ? view : undefined,
  )
  if (record !== null) {
    // 编辑框提交时用的名录是「当前选中的这一份」，对不上就不能开，
    // 否则保存会把记录挪到另一份名录里。
    if (selectedId.value === record.list_id) {
      openEditRecord(record)
    } else {
      recordsError.value = '这条记录所属的名录已经不在了'
    }
    // `?record=` 已经被 loadLists 写回地址栏时抹掉了，刷新不会又弹一次编辑框。
  }
})

// 地址栏变了但组件没重建（点顶栏的「物种名录」、浏览器前进后退）时跟着变。
watch(
  () => route.query,
  () => void syncFromRoute(),
)

/// `?record=<id>` 指向的那条记录；没带 / 取不到就返回 null。
async function recordFromQuery(): Promise<SpeciesRecord | null> {
  const recordId = route.query.record
  if (typeof recordId !== 'string' || recordId === '') {
    return null
  }
  try {
    return await getRecord(recordId)
  } catch (e) {
    recordsError.value = errorMessage(e)
    return null
  }
}

/// 卡片里的「编辑」：记录就在当前这一页，直接把弹窗套上。
function editRecordFromCard(target: SpeciesCardTarget): void {
  const record = records.value.find((item) => item.id === target.id)
  if (record !== undefined) {
    openEditRecord(record)
  }
}

async function search(): Promise<void> {
  appliedKeyword.value = keyword.value.trim()
  offset.value = 0
  // 选了类群时先站到有该类群记录的名录上（当前这份里有就不动），再拉记录。
  if (await moveToListWithTaxon()) {
    await loadDetail()
  }
  writeQuery()
  await loadRecords()
}

async function clearSearch(): Promise<void> {
  keyword.value = ''
  appliedKeyword.value = ''
  offset.value = 0
  writeQuery()
  await loadRecords()
}

async function goToPage(page: number): Promise<void> {
  offset.value = (page - 1) * PAGE_SIZE
  writeQuery()
  await loadRecords()
}

// --- 名录的增 / 改 / 删 ---

const listDialog = ref<HTMLDialogElement | null>(null)
const listForm = reactive({
  id: null as string | null,
  name: '',
  description: '',
  /// `<input type="number">` 上 Vue 会自动加 `.number` 修饰符：清空时是空串，
  /// 填了就是 number（见 submitList 里的 `String(...)`）。
  position: '' as string | number,
  submitting: false,
  error: '',
})

/// 与后端 `DEFAULT_LIST_POSITION`、迁移里的列默认值保持一致。
const DEFAULT_LIST_POSITION = 100

/// 清空表单。`openCreateList` / `openEditList` 都会先调它。
function resetListForm(): void {
  listForm.id = null
  listForm.name = ''
  listForm.description = ''
  listForm.position = ''
  listForm.error = ''
}

/// `<dialog>` 的 `close` 事件是**排队异步派发**的（HTML 规范），可能在弹窗已经被
/// 重新打开之后才到达。那时候重置会把刚填好的表单清掉（连着点「编辑名录」就能看到），
/// 所以只在弹窗真的关着时才重置。
function onListDialogClosed(): void {
  if (listDialog.value?.open !== true) {
    resetListForm()
  }
}

function openCreateList(): void {
  savedMessage.value = ''
  resetListForm()
  if (listDialog.value !== null && !listDialog.value.open) {
    listDialog.value.showModal()
  }
}

function openEditList(): void {
  const list = selectedList.value
  if (list === null) {
    return
  }
  savedMessage.value = ''
  resetListForm()
  listForm.id = list.id
  listForm.name = list.name
  listForm.description = list.description ?? ''
  listForm.position = String(list.position)
  if (listDialog.value !== null && !listDialog.value.open) {
    listDialog.value.showModal()
  }
}

function closeListDialog(): void {
  listDialog.value?.close()
}

async function submitList(): Promise<void> {
  listForm.error = ''
  const problem = validateField('名录名', listForm.name, LIST_NAME_MAX_CHARS)
  if (problem !== '') {
    listForm.error = problem
    return
  }

  // 排序：留空走默认值；填了就必须是 ≥ 0 的整数（后端列是 INTEGER）。
  // 注意 `position` 可能是 number（Vue 对 number 输入框自动加了 `.number`）。
  const positionText = String(listForm.position).trim()
  if (positionText !== '' && !/^\d+$/.test(positionText)) {
    listForm.error = '排序必须是 0 或正整数'
    return
  }
  if (Number(positionText) > 2147483647) {
    listForm.error = '排序不能超过 2147483647'
    return
  }
  const position = positionText === '' ? DEFAULT_LIST_POSITION : Number(positionText)

  listForm.submitting = true
  try {
    const name = listForm.name.trim()
    // PATCH 无法把 description 改回 NULL（后端 Option 语义），清空只能存空串。
    const description = listForm.description.trim()
    if (listForm.id === null) {
      const created = await createSpeciesList({
        name,
        description: description === '' ? null : description,
        position,
      })
      closeListDialog()
      savedMessage.value = `已新增名录「${created.name}」`
      await loadLists(created.id)
    } else {
      const updated = await updateSpeciesList(listForm.id, { name, description, position })
      closeListDialog()
      savedMessage.value = `已更新名录「${updated.name}」`
      await loadLists(updated.id)
    }
  } catch (e) {
    listForm.error = errorMessage(e)
  } finally {
    listForm.submitting = false
  }
}

const listDeleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const listDeleteError = ref('')
const deletingList = ref(false)

function openDeleteList(): void {
  if (selectedList.value === null) {
    return
  }
  savedMessage.value = ''
  listDeleteError.value = ''
  listDeleteDialog.value?.open()
}

async function confirmDeleteList(): Promise<void> {
  const target = selectedList.value
  if (target === null) {
    return
  }
  deletingList.value = true
  listDeleteError.value = ''
  try {
    await deleteSpeciesList(target.id)
    listDeleteDialog.value?.close()
    savedMessage.value = `已删除名录「${target.name}」及其全部记录`
    selectedId.value = null
    detail.value = null
    await loadLists()
  } catch (e) {
    listDeleteError.value = errorMessage(e)
  } finally {
    deletingList.value = false
  }
}

// --- 记录的增 / 改 / 删 ---

const recordDialog = ref<HTMLDialogElement | null>(null)
/// 每次打开自增，作为 TaxonPicker 的 key —— 强制重建，丢掉上一次的选择。
const recordFormKey = ref(0)
const recordForm = reactive({
  id: null as string | null,
  taxonId: null as string | null,
  scientificName: '',
  chineseName: '',
  distribution: '',
  note: '',
  source: '',
  recordNo: '',
  submitting: false,
  error: '',
})

function resetRecordForm(): void {
  recordForm.id = null
  recordForm.taxonId = null
  recordForm.scientificName = ''
  recordForm.chineseName = ''
  recordForm.distribution = ''
  recordForm.note = ''
  recordForm.source = ''
  recordForm.recordNo = ''
  recordForm.error = ''
}

function openRecordDialog(): void {
  recordFormKey.value += 1
  if (recordDialog.value !== null && !recordDialog.value.open) {
    recordDialog.value.showModal()
  }
}

function openCreateRecord(): void {
  savedMessage.value = ''
  resetRecordForm()
  openRecordDialog()
}

function openEditRecord(record: SpeciesRecord): void {
  savedMessage.value = ''
  resetRecordForm()
  recordForm.id = record.id
  recordForm.taxonId = record.taxon_id
  recordForm.scientificName = record.scientific_name
  recordForm.chineseName = record.chinese_name ?? ''
  recordForm.distribution = record.distribution ?? ''
  recordForm.note = record.note ?? ''
  recordForm.source = record.source ?? ''
  recordForm.recordNo = record.record_no ?? ''
  openRecordDialog()
}

function closeRecordDialog(): void {
  recordDialog.value?.close()
}

async function submitRecord(): Promise<void> {
  recordForm.error = ''
  const listId = selectedId.value
  if (listId === null) {
    return
  }
  if (recordForm.taxonId === null) {
    recordForm.error = '请选择鉴定到的分类节点'
    return
  }
  const problem = validateField('学名', recordForm.scientificName, TAXON_NAME_MAX_CHARS)
  if (problem !== '') {
    recordForm.error = problem
    return
  }
  const recordNo = recordForm.recordNo.trim()
  if (recordNo !== '' && charCount(recordNo) > RECORD_NO_MAX_CHARS) {
    recordForm.error = `编号最多 ${RECORD_NO_MAX_CHARS} 个字符`
    return
  }

  recordForm.submitting = true
  try {
    const payload = {
      list_id: listId,
      taxon_id: recordForm.taxonId,
      scientific_name: recordForm.scientificName.trim(),
      chinese_name: optional(recordForm.chineseName),
      distribution: optional(recordForm.distribution),
      note: optional(recordForm.note),
      source: optional(recordForm.source),
      record_no: optional(recordForm.recordNo),
    }
    if (recordForm.id === null) {
      const created = await createRecord(payload)
      savedMessage.value = `已新增记录「${created.scientific_name}」`
    } else {
      const updated = await updateRecord(recordForm.id, payload)
      savedMessage.value = `已更新记录「${updated.scientific_name}」`
    }
    closeRecordDialog()
    await Promise.all([loadDetail(), loadRecords()])
  } catch (e) {
    recordForm.error = errorMessage(e)
  } finally {
    recordForm.submitting = false
  }
}

const recordDeleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const recordDeleteError = ref('')
const deletingRecord = ref(false)
const pendingRecord = ref<SpeciesRecord | null>(null)

function openDeleteRecord(record: SpeciesRecord): void {
  savedMessage.value = ''
  recordDeleteError.value = ''
  pendingRecord.value = record
  recordDeleteDialog.value?.open()
}

async function confirmDeleteRecord(): Promise<void> {
  const target = pendingRecord.value
  if (target === null) {
    return
  }
  deletingRecord.value = true
  recordDeleteError.value = ''
  try {
    await deleteRecord(target.id)
    recordDeleteDialog.value?.close()
    savedMessage.value = `已删除记录「${target.scientific_name}」`
    await Promise.all([loadDetail(), loadRecords()])
  } catch (e) {
    recordDeleteError.value = errorMessage(e)
  } finally {
    deletingRecord.value = false
  }
}
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <h2>物种名录</h2>
        <div class="head-actions">
          <span class="muted">共 {{ lists.length }} 份</span>
          <button v-if="canManage" type="button" @click="openCreateList">新增名录</button>
          <button class="secondary" type="button" :disabled="listsLoading" @click="loadLists()">
            刷新
          </button>
        </div>
      </div>

      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
      <p v-if="listsError" class="alert error">{{ listsError }}</p>
      <p v-if="listsLoading" class="muted">加载中…</p>

      <div v-else class="list-grid">
        <button
          v-for="list in lists"
          :key="list.id"
          class="list-item"
          :class="{ active: list.id === selectedId }"
          type="button"
          @click="selectList(list.id)"
        >
          <strong>{{ list.name }}</strong>
          <span class="muted">{{ list.description ?? '（没有描述）' }}</span>
        </button>
        <p v-if="lists.length === 0" class="muted">
          还没有名录{{ canManage ? '，点右上角「新增名录」创建第一份。' : '。' }}
        </p>
      </div>
    </section>

    <section v-if="selectedList" class="card">
      <div class="card-head">
        <div>
          <h2>{{ selectedList.name }}</h2>
          <p class="hint">
            {{ selectedList.description ?? '（没有描述）' }} · 共
            {{ detail === null ? '…' : detail.record_count }} 条记录
          </p>
        </div>
        <div class="head-actions">
          <button v-if="canManage" type="button" @click="openCreateRecord">新增记录</button>
          <button v-if="canManage" class="secondary" type="button" @click="openEditList">
            编辑名录
          </button>
          <button v-if="canManage" class="danger" type="button" @click="openDeleteList">
            删除名录
          </button>
        </div>
      </div>

      <form class="search-bar" @submit.prevent="search">
        <input v-model="keyword" placeholder="按学名或中文名搜索…" />
        <button type="submit">搜索</button>
        <button v-if="appliedKeyword !== ''" class="secondary" type="button" @click="clearSearch">
          清除
        </button>
      </form>

      <details class="advanced-search" :open="taxonOpen" @toggle="onTaxonToggle">
        <summary>
          <span>类群筛选</span>
          <span v-if="taxonId !== null" class="tag">已筛选</span>
          <span class="advanced-toggle">
            <span class="when-closed">展开</span>
            <span class="when-open">收起</span>
          </span>
        </summary>
        <div class="advanced-search-body">
          <p class="hint">
            逐级选择，选到哪一级就按哪一级筛选，默认含下级；不选则不限类群。改完按「搜索」生效。
          </p>
          <TaxonPicker :key="pickerKey" v-model="taxonId" />
          <label class="checkbox">
            <input type="checkbox" v-model="descendants" :disabled="taxonId === null" />
            包含下级类群
          </label>
        </div>
      </details>

      <p v-if="recordsError" class="alert error">{{ recordsError }}</p>

      <SpeciesRecordTable
        :records="records"
        :loading="recordsLoading"
        :on-edit-record="editRecordFromCard"
      >
        <template v-if="canManage" #actions="{ record }">
          <button class="secondary" type="button" @click="openEditRecord(record)">编辑</button>
          <button class="danger" type="button" @click="openDeleteRecord(record)">删除</button>
        </template>
        <template #empty>
          <span v-if="appliedKeyword !== '' && taxonId !== null">
            没有既匹配「{{ appliedKeyword }}」又属于该类群的记录
          </span>
          <span v-else-if="appliedKeyword !== ''">没有匹配「{{ appliedKeyword }}」的记录</span>
          <span v-else-if="taxonId !== null">这份名录里没有该类群的记录</span>
          <span v-else>这份名录还没有记录</span>
        </template>
      </SpeciesRecordTable>

      <ListPager
        :total="recordsTotal"
        :page-size="PAGE_SIZE"
        :page="offset / PAGE_SIZE + 1"
        :disabled="recordsLoading"
        @change="goToPage"
      />
    </section>
  </div>

  <!-- 名录的增 / 改 -->
  <dialog ref="listDialog" class="modal" @close="onListDialogClosed" @click.self="closeListDialog">
    <form @submit.prevent="submitList">
      <h2>{{ listForm.id === null ? '新增名录' : '编辑名录' }}</h2>
      <p v-if="listForm.error" class="alert error">{{ listForm.error }}</p>

      <div class="field">
        <label for="list-name">名录名</label>
        <input id="list-name" v-model="listForm.name" :maxlength="LIST_NAME_MAX_CHARS" required />
        <span class="hint">全局唯一，最多 {{ LIST_NAME_MAX_CHARS }} 个字符</span>
      </div>

      <div class="field">
        <label for="list-description">描述</label>
        <input id="list-description" v-model="listForm.description" />
      </div>

      <div class="field">
        <label for="list-position">排序</label>
        <input id="list-position" v-model="listForm.position" type="number" min="0" step="1" />
        <span class="hint">数字小的排前面，留空按 {{ DEFAULT_LIST_POSITION }}（排最后）</span>
      </div>

      <div class="actions">
        <button type="submit" :disabled="listForm.submitting">
          {{ listForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="listForm.submitting"
          @click="closeListDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <!-- 记录的增 / 改 -->
  <dialog
    ref="recordDialog"
    class="modal modal-wide"
    @close="resetRecordForm"
    @click.self="closeRecordDialog"
  >
    <form @submit.prevent="submitRecord">
      <h2>{{ recordForm.id === null ? '新增记录' : '编辑记录' }}</h2>
      <p class="hint">所属名录：{{ selectedList?.name }}</p>
      <p v-if="recordForm.error" class="alert error">{{ recordForm.error }}</p>

      <div class="field">
        <label>鉴定到的分类节点（必选；只定到属也可以停在「属」）</label>
        <TaxonPicker
          :key="recordFormKey"
          v-model="recordForm.taxonId"
          :disabled="recordForm.submitting"
        />
      </div>

      <div class="field">
        <label for="record-scientific-name">学名</label>
        <input
          id="record-scientific-name"
          v-model="recordForm.scientificName"
          :maxlength="TAXON_NAME_MAX_CHARS"
          required
        />
      </div>

      <div class="field">
        <label for="record-chinese-name">中文名</label>
        <input id="record-chinese-name" v-model="recordForm.chineseName" />
      </div>

      <div class="field">
        <label for="record-distribution">分布</label>
        <input id="record-distribution" v-model="recordForm.distribution" />
      </div>

      <div class="field">
        <label for="record-note">备注</label>
        <input id="record-note" v-model="recordForm.note" />
      </div>

      <div class="field">
        <label for="record-source">来源</label>
        <input id="record-source" v-model="recordForm.source" />
      </div>

      <div class="field">
        <label for="record-no">编号</label>
        <input id="record-no" v-model="recordForm.recordNo" :maxlength="RECORD_NO_MAX_CHARS" />
      </div>

      <div class="actions">
        <button type="submit" :disabled="recordForm.submitting">
          {{ recordForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="recordForm.submitting"
          @click="closeRecordDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <ConfirmDialog
    ref="listDeleteDialog"
    title="删除名录"
    :message="
      selectedList === null
        ? ''
        : `确定删除名录「${selectedList.name}」？它下面的全部记录会一起删除，此操作不可撤销。`
    "
    :busy="deletingList"
    :error="listDeleteError"
    @confirm="confirmDeleteList"
    @cancel="listDeleteError = ''"
  />

  <ConfirmDialog
    ref="recordDeleteDialog"
    title="删除记录"
    :message="
      pendingRecord === null
        ? ''
        : `确定删除记录「${pendingRecord.scientific_name}」？此操作不可撤销。`
    "
    :busy="deletingRecord"
    :error="recordDeleteError"
    @confirm="confirmDeleteRecord"
    @cancel="recordDeleteError = ''"
  />
</template>
