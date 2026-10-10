<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { errorMessage } from '../api/http'
import {
  createMaintenanceLog,
  deleteMaintenanceLog,
  listMaintenanceLogs,
  updateMaintenanceLog,
} from '../api/maintenanceLogs'
import { listSpeciesLists } from '../api/taxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import ListPager from '../components/ListPager.vue'
import { useAuthStore } from '../stores/auth'
import type { MaintenanceLog } from '../maintenanceLogs'
import { clampOffset } from '../pagination'
import type { SpeciesList } from '../taxonomy'
import { AUTHOR_MAX_CHARS, ENTRY_DATE_MAX_CHARS, charCount } from '../validation'

const auth = useAuthStore()
const canManage = computed(() => auth.canManageTaxonomy)

const PAGE_SIZE = 50

const lists = ref<SpeciesList[]>([])
const logs = ref<MaintenanceLog[]>([])
const loading = ref(false)
const error = ref('')
const savedMessage = ref('')

/// '' 表示「全部名录」。
const filterListId = ref('')
const keyword = ref('')
const appliedKeyword = ref('')
const offset = ref(0)
const total = ref(0)

function listName(id: string | null): string {
  if (id === null) {
    return '全局'
  }
  return lists.value.find((list) => list.id === id)?.name ?? id
}

/// 空串统一转成 null（后端的可空字段用 null 表示清空）。
function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed === '' ? null : trimmed
}

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const page = await listMaintenanceLogs({
      listId: filterListId.value === '' ? undefined : filterListId.value,
      q: appliedKeyword.value === '' ? undefined : appliedKeyword.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    })
    // 删日志后当前页可能已经越过末页，退回去重拉一次（clampOffset 幂等）。
    const clamped = clampOffset(offset.value, page.total, PAGE_SIZE)
    if (clamped !== offset.value) {
      offset.value = clamped
      await load()
      return
    }
    logs.value = page.items
    total.value = page.total
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

async function clearSearch(): Promise<void> {
  keyword.value = ''
  appliedKeyword.value = ''
  offset.value = 0
  await load()
}

async function filterChanged(): Promise<void> {
  offset.value = 0
  await load()
}

async function goToPage(page: number): Promise<void> {
  offset.value = (page - 1) * PAGE_SIZE
  await load()
}

// --- 增 / 改 / 删 ---

const dialog = ref<HTMLDialogElement | null>(null)
const form = reactive({
  id: null as string | null,
  /// '' = 全局日志（提交时转成 null）。
  listId: '',
  entryDate: '',
  author: '',
  summary: '',
  appendix: '',
  submitting: false,
  error: '',
})

function resetForm(): void {
  form.id = null
  form.listId = ''
  form.entryDate = ''
  form.author = ''
  form.summary = ''
  form.appendix = ''
  form.error = ''
}

/// `<dialog>` 的 `close` 事件是排队异步派发的，可能在弹窗被重新打开之后才到达，
/// 那时重置会清掉刚填好的表单，所以只在弹窗真的关着时才重置。（同 SpeciesListsView）
function onDialogClosed(): void {
  if (dialog.value?.open !== true) {
    resetForm()
  }
}

function openCreate(): void {
  savedMessage.value = ''
  resetForm()
  if (filterListId.value !== '') {
    form.listId = filterListId.value
  }
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }
}

function openEdit(log: MaintenanceLog): void {
  savedMessage.value = ''
  resetForm()
  form.id = log.id
  form.listId = log.list_id ?? ''
  form.entryDate = log.entry_date ?? ''
  form.author = log.author ?? ''
  form.summary = log.summary
  form.appendix = log.species_appendix ?? ''
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }
}

function closeDialog(): void {
  dialog.value?.close()
}

async function submit(): Promise<void> {
  form.error = ''
  if (form.summary.trim() === '') {
    form.error = '修订说明不能为空'
    return
  }
  const entryDate = form.entryDate.trim()
  if (entryDate !== '' && charCount(entryDate) > ENTRY_DATE_MAX_CHARS) {
    form.error = `修订笔记最多 ${ENTRY_DATE_MAX_CHARS} 个字符`
    return
  }
  const author = form.author.trim()
  if (author !== '' && charCount(author) > AUTHOR_MAX_CHARS) {
    form.error = `修订人最多 ${AUTHOR_MAX_CHARS} 个字符`
    return
  }

  form.submitting = true
  try {
    const payload = {
      list_id: form.listId === '' ? null : form.listId,
      entry_date: optional(form.entryDate),
      author: optional(form.author),
      summary: form.summary.trim(),
      species_appendix: optional(form.appendix),
    }
    if (form.id === null) {
      const created = await createMaintenanceLog(payload)
      savedMessage.value = `已新增维护日志「${created.summary.slice(0, 20)}」`
    } else {
      const updated = await updateMaintenanceLog(form.id, payload)
      savedMessage.value = `已更新维护日志「${updated.summary.slice(0, 20)}」`
    }
    closeDialog()
    await load()
  } catch (e) {
    form.error = errorMessage(e)
  } finally {
    form.submitting = false
  }
}

const deleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const deleteError = ref('')
const deleting = ref(false)
const pending = ref<MaintenanceLog | null>(null)

function openDelete(log: MaintenanceLog): void {
  savedMessage.value = ''
  deleteError.value = ''
  pending.value = log
  deleteDialog.value?.open()
}

async function confirmDelete(): Promise<void> {
  const target = pending.value
  if (target === null) {
    return
  }
  deleting.value = true
  deleteError.value = ''
  try {
    await deleteMaintenanceLog(target.id)
    deleteDialog.value?.close()
    savedMessage.value = '已删除维护日志'
    await load()
  } catch (e) {
    deleteError.value = errorMessage(e)
  } finally {
    deleting.value = false
  }
}
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <div>
          <h2>维护日志</h2>
          <p class="hint">各名录的修订记录（修订笔记 / 修订人 / 修订说明 / 物种附录）</p>
        </div>
        <div class="head-actions">
          <button v-if="canManage" type="button" @click="openCreate">新增日志</button>
          <button class="secondary" type="button" :disabled="loading" @click="load">刷新</button>
        </div>
      </div>

      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
      <p v-if="error" class="alert error">{{ error }}</p>

      <form class="search-bar" @submit.prevent="search">
        <select v-model="filterListId" @change="filterChanged">
          <option value="">全部名录</option>
          <option v-for="list in lists" :key="list.id" :value="list.id">{{ list.name }}</option>
        </select>
        <input v-model="keyword" placeholder="按修订说明或修订人搜索…" />
        <button type="submit">搜索</button>
        <button v-if="appliedKeyword !== ''" class="secondary" type="button" @click="clearSearch">
          清除
        </button>
      </form>

      <p v-if="loading" class="muted">加载中…</p>

      <div v-else class="log-list">
        <article v-for="log in logs" :key="log.id" class="log-item">
          <div class="log-head">
            <div class="log-meta">
              <strong>{{ log.entry_date ?? '-' }}</strong>
              <span class="muted">{{ log.author ?? '未署名' }}</span>
              <span class="tag">{{ listName(log.list_id) }}</span>
            </div>
            <div v-if="canManage" class="head-actions">
              <button class="secondary" type="button" @click="openEdit(log)">编辑</button>
              <button class="danger" type="button" @click="openDelete(log)">删除</button>
            </div>
          </div>
          <p class="log-summary pre-wrap">{{ log.summary }}</p>
          <div v-if="log.species_appendix !== null" class="log-appendix">
            <span class="muted">物种附录</span>
            <p class="pre-wrap">{{ log.species_appendix }}</p>
          </div>
        </article>
        <p v-if="logs.length === 0" class="muted">
          <span v-if="appliedKeyword !== ''">没有匹配「{{ appliedKeyword }}」的维护日志</span>
          <span v-else>还没有维护日志</span>
        </p>
      </div>

      <ListPager
        v-if="!loading"
        :total="total"
        :page-size="PAGE_SIZE"
        :page="offset / PAGE_SIZE + 1"
        :disabled="loading"
        @change="goToPage"
      />
    </section>
  </div>

  <!-- 增 / 改 -->
  <dialog ref="dialog" class="modal modal-wide" @close="onDialogClosed" @click.self="closeDialog">
    <form @submit.prevent="submit">
      <h2>{{ form.id === null ? '新增维护日志' : '编辑维护日志' }}</h2>
      <p v-if="form.error" class="alert error">{{ form.error }}</p>

      <div class="field">
        <label for="log-list">名录</label>
        <select id="log-list" v-model="form.listId">
          <option value="">全局（不属于任何名录）</option>
          <option v-for="list in lists" :key="list.id" :value="list.id">{{ list.name }}</option>
        </select>
      </div>

      <div class="field">
        <label for="log-date">修订笔记</label>
        <input
          id="log-date"
          v-model="form.entryDate"
          :maxlength="ENTRY_DATE_MAX_CHARS"
          placeholder="20230402 / 2022.11.25 - 至今…"
        />
        <span class="hint">日期或一段说明，原样保存，最多 {{ ENTRY_DATE_MAX_CHARS }} 个字符</span>
      </div>

      <div class="field">
        <label for="log-author">修订人</label>
        <input id="log-author" v-model="form.author" :maxlength="AUTHOR_MAX_CHARS" />
      </div>

      <div class="field">
        <label for="log-summary">修订说明</label>
        <textarea id="log-summary" v-model="form.summary" rows="3"></textarea>
      </div>

      <div class="field">
        <label for="log-appendix">物种附录</label>
        <textarea id="log-appendix" v-model="form.appendix" rows="3"></textarea>
      </div>

      <div class="actions">
        <button type="submit" :disabled="form.submitting">
          {{ form.submitting ? '保存中…' : '保存' }}
        </button>
        <button class="secondary" type="button" :disabled="form.submitting" @click="closeDialog">
          取消
        </button>
      </div>
    </form>
  </dialog>

  <ConfirmDialog
    ref="deleteDialog"
    title="删除维护日志"
    :message="
      pending === null
        ? ''
        : `确定删除「${pending.summary.slice(0, 30)}…」这条维护日志？此操作不可撤销。`
    "
    :busy="deleting"
    :error="deleteError"
    @confirm="confirmDelete"
    @cancel="deleteError = ''"
  />
</template>
