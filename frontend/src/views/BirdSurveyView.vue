<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  createBirdRecord,
  deleteBirdRecord,
  listBirdRecords,
  updateBirdRecord,
} from '../api/birdRecords'
import { errorMessage } from '../api/http'
import { createPhoto, listPhotos } from '../api/photos'
import { findTaxonId, listRecords } from '../api/taxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import NoteText from '../components/NoteText.vue'
import ListPager from '../components/ListPager.vue'
import SpeciesRecordTable from '../components/SpeciesRecordTable.vue'
import SpeciesCard, { type SpeciesCardTarget } from '../components/SpeciesCard.vue'
import { useAuthStore } from '../stores/auth'
import { BIRD_CLASS_SCIENTIFIC_NAME, type BirdRecord } from '../birds'
import { clampOffset } from '../pagination'
import { MAX_PHOTOS_PER_TAXON, validatePhotoFile, type SpeciesPhoto } from '../photos'
import { taxonLabel, type SpeciesRecord } from '../taxonomy'
import { formatTimestamp } from '../types'
import {
  BIRD_LOCATION_MAX_CHARS,
  BIRD_NAME_MAX_CHARS,
  BIRD_OBSERVED_AT_MAX_CHARS,
  BIRD_OBSERVER_MAX_CHARS,
  charCount,
  validateField,
} from '../validation'

/// 「鸟类调查」：三个标签页共用一张表页面的骨架。
///
/// - 鸟种清单：鸟纲（Aves）下的全部名录记录，直接复用 `/taxonomy/records`。
/// - 重要记录：`bird_records` 表，docx 导入 + STAFF 手工补录。
/// - 鸟调记录：还没有表，先放一个空标签。
const auth = useAuthStore()
const router = useRouter()
const canManage = computed(() => auth.canManage)

const TABS = [
  { key: 'species', label: '鸟种清单' },
  { key: 'records', label: '重要记录' },
  { key: 'survey', label: '鸟调记录' },
] as const
type TabKey = (typeof TABS)[number]['key']
const tab = ref<TabKey>('species')

const PAGE_SIZE = 50

/// 空串统一转成 null（后端的可空字段用 null 表示清空）。
function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed === '' ? null : trimmed
}

/// `<input type="date">` 需要的本地日期，不能用 toISOString（那是 UTC）。
function today(): string {
  const now = new Date()
  const month = String(now.getMonth() + 1).padStart(2, '0')
  const day = String(now.getDate()).padStart(2, '0')
  return `${now.getFullYear()}-${month}-${day}`
}

function pad2(value: string): string {
  return value.padStart(2, '0')
}

/// 从「时间」原文里抠出一个拍摄日期（后端 `bird_observed_bounds` 的简化版）：
/// 认不出具体日子就退到当月 1 日 / 当年 1 月 1 日，再认不出用今天。
/// 只用来给弹窗里上传的照片填 `taken_at`（决定年月子目录），不参与时间筛选。
function dateFromObservedAt(value: string): string {
  const text = value.trim()
  const full =
    text.match(/^(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日$/) ??
    text.match(/^(\d{4})[-./](\d{1,2})[-./](\d{1,2})$/) ??
    null
  if (full !== null) {
    return `${full[1]}-${pad2(full[2])}-${pad2(full[3])}`
  }
  const month = text.match(/^(\d{4})\s*年\s*(\d{1,2})\s*月/)
  if (month !== null) {
    return `${month[1]}-${pad2(month[2])}-01`
  }
  const year = text.match(/^(\d{4})/)
  if (year !== null) {
    return `${year[1]}-01-01`
  }
  return today()
}

/// 照片卡片的一行摘要（与「物种照片」页同口径）。
function photoSummary(photo: SpeciesPhoto): string {
  const parts = [photo.taken_at]
  if (photo.photographer !== null && photo.photographer !== '') {
    parts.push(photo.photographer)
  }
  return parts.join(' ')
}

// ==========================================================================
// 鸟种清单：鸟纲下的名录记录
// ==========================================================================

const speciesRecords = ref<SpeciesRecord[]>([])
const speciesLoading = ref(false)
const speciesError = ref('')
const speciesKeyword = ref('')
const speciesAppliedKeyword = ref('')
const speciesOffset = ref(0)
const speciesTotal = ref(0)
/// 鸟纲节点 id，首次加载时从分类树里查出来（不写死 UUID）。
const birdClassId = ref<string | null>(null)

async function loadSpeciesRecords(): Promise<void> {
  if (birdClassId.value === null) {
    speciesRecords.value = []
    return
  }
  speciesLoading.value = true
  speciesError.value = ''
  try {
    const page = await listRecords({
      taxonId: birdClassId.value,
      descendants: true,
      q: speciesAppliedKeyword.value === '' ? undefined : speciesAppliedKeyword.value,
      limit: PAGE_SIZE,
      offset: speciesOffset.value,
    })
    speciesRecords.value = page.items
    speciesTotal.value = page.total
  } catch (e) {
    speciesError.value = errorMessage(e)
  } finally {
    speciesLoading.value = false
  }
}

async function searchSpecies(): Promise<void> {
  speciesAppliedKeyword.value = speciesKeyword.value.trim()
  speciesOffset.value = 0
  await loadSpeciesRecords()
}

async function clearSpeciesSearch(): Promise<void> {
  speciesKeyword.value = ''
  speciesAppliedKeyword.value = ''
  speciesOffset.value = 0
  await loadSpeciesRecords()
}

async function goToSpeciesPage(page: number): Promise<void> {
  speciesOffset.value = (page - 1) * PAGE_SIZE
  await loadSpeciesRecords()
}

// ==========================================================================
// 重要记录
// ==========================================================================

const records = ref<BirdRecord[]>([])
const recordsLoading = ref(false)
const recordsError = ref('')
const recordsKeyword = ref('')
const recordsAppliedKeyword = ref('')
/// 时间范围（`yyyy-mm-dd`，`<input type="date">` 给的就是这个格式）；'' = 不限。
const recordsFrom = ref('')
const recordsTo = ref('')
const recordsAppliedFrom = ref('')
const recordsAppliedTo = ref('')
const recordsOffset = ref(0)
const recordsTotal = ref(0)
const savedMessage = ref('')

/// 鸟纲下的全部鸟种，供「新增 / 编辑重要记录」的下拉框用。
/// 与「鸟种清单」同一个来源（鸟纲的名录记录），所以两边看到的鸟种一致。
const birdOptions = ref<SpeciesRecord[]>([])
const birdOptionsError = ref('')
/// 下拉框上方的筛选词。
const birdKeyword = ref('')

async function loadRecords(): Promise<void> {
  recordsLoading.value = true
  recordsError.value = ''
  try {
    const page = await listBirdRecords({
      q: recordsAppliedKeyword.value === '' ? undefined : recordsAppliedKeyword.value,
      from: recordsAppliedFrom.value === '' ? undefined : recordsAppliedFrom.value,
      to: recordsAppliedTo.value === '' ? undefined : recordsAppliedTo.value,
      limit: PAGE_SIZE,
      offset: recordsOffset.value,
    })
    // 删记录 / 改筛选条件后当前页可能已经越过末页，退回去重拉一次。
    const clamped = clampOffset(recordsOffset.value, page.total, PAGE_SIZE)
    if (clamped !== recordsOffset.value) {
      recordsOffset.value = clamped
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

async function searchRecords(): Promise<void> {
  recordsAppliedKeyword.value = recordsKeyword.value.trim()
  recordsAppliedFrom.value = recordsFrom.value
  recordsAppliedTo.value = recordsTo.value
  recordsOffset.value = 0
  await loadRecords()
}

async function clearRecordsSearch(): Promise<void> {
  recordsKeyword.value = ''
  recordsAppliedKeyword.value = ''
  recordsFrom.value = ''
  recordsTo.value = ''
  recordsAppliedFrom.value = ''
  recordsAppliedTo.value = ''
  recordsOffset.value = 0
  await loadRecords()
}

/// 搜索框里有没有生效的条件（决定要不要显示「清除」）。
const recordsFiltered = computed(
  () =>
    recordsAppliedKeyword.value !== '' ||
    recordsAppliedFrom.value !== '' ||
    recordsAppliedTo.value !== '',
)

async function goToRecordsPage(page: number): Promise<void> {
  recordsOffset.value = (page - 1) * PAGE_SIZE
  await loadRecords()
}

async function loadBirdOptions(): Promise<void> {
  if (birdClassId.value === null) {
    return
  }
  birdOptionsError.value = ''
  try {
    // 266 个鸟种，一次拉完；limit 上限就是 1000（见后端 MAX_LIMIT）。
    const all = await listRecords({
      taxonId: birdClassId.value,
      descendants: true,
      limit: 1000,
    })
    birdOptions.value = all.items
  } catch (e) {
    birdOptionsError.value = errorMessage(e)
  }
}

/// 选中的鸟种不能被筛选掉，否则下拉框会显示成空白。
const filteredBirds = computed(() => {
  const all = birdOptions.value
  const keyword = birdKeyword.value.trim().toLowerCase()
  if (keyword === '') {
    return all
  }
  const matched = all.filter((bird) => taxonLabel(bird).toLowerCase().includes(keyword))
  const chosen = all.find((bird) => bird.taxon_id === form.taxonId)
  if (chosen !== undefined && !matched.includes(chosen)) {
    matched.unshift(chosen)
  }
  return matched
})

/// 选了鸟种就把学名 / 中文名带出来：这两个字段在弹窗里不单独填，
/// 避免手填的名字和关联的物种对不上。
/// 顺带清掉已选照片：照片必须和记录同物种，换了鸟选中的照片就不合法了。
function pickBird(): void {
  const bird = birdOptions.value.find((item) => item.taxon_id === form.taxonId)
  form.scientificName = bird?.scientific_name ?? ''
  form.chineseName = bird?.chinese_name ?? ''
  clearPhoto()
}

// --- 照片：上传新照片 / 从同一鸟种已有照片里挑一张 ---

/// 隐藏的文件输入：由「上传照片」按钮触发。
const photoFileInput = ref<HTMLInputElement | null>(null)

function clearPhoto(): void {
  form.photoId = ''
  form.photoUrl = ''
}

/// 上传新照片。照片自己的元数据从记录带出，不单独填：
/// 拍摄时间取「时间」解析出的日期（解析不出用今天），拍摄人 / 地点 / 备注照抄。
async function uploadRecordPhoto(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0] ?? null
  input.value = ''
  if (file === null) {
    return
  }
  if (form.taxonId === '') {
    form.error = '请先选择鸟种'
    return
  }
  const problem = validatePhotoFile(file)
  if (problem !== '') {
    form.error = problem
    return
  }

  form.error = ''
  form.photoUploading = true
  try {
    const photo = await createPhoto({
      file,
      taxonId: form.taxonId,
      takenAt: dateFromObservedAt(form.observedAt),
      photographer: optional(form.observer),
      location: optional(form.location),
      note: optional(form.note),
    })
    form.photoId = photo.id
    form.photoUrl = photo.url
  } catch (e) {
    form.error = errorMessage(e)
  } finally {
    form.photoUploading = false
  }
}

/// 「选择已有照片」弹窗：列这个鸟种的照片，点一张就选上。
const photoPicker = ref<HTMLDialogElement | null>(null)
const pickerPhotos = ref<SpeciesPhoto[]>([])
const pickerLoading = ref(false)
const pickerError = ref('')

async function openPhotoPicker(): Promise<void> {
  if (form.taxonId === '') {
    return
  }
  pickerError.value = ''
  pickerPhotos.value = []
  pickerLoading.value = true
  photoPicker.value?.showModal()
  try {
    // 每个物种最多 9 张（后端上限），一页就拉完了。
    const page = await listPhotos({ taxonId: form.taxonId, limit: MAX_PHOTOS_PER_TAXON })
    pickerPhotos.value = page.items
  } catch (e) {
    pickerError.value = errorMessage(e)
  } finally {
    pickerLoading.value = false
  }
}

function closePhotoPicker(): void {
  photoPicker.value?.close()
}

function choosePhoto(photo: SpeciesPhoto): void {
  form.photoId = photo.id
  form.photoUrl = photo.url
  closePhotoPicker()
}

// --- 重要记录的增 / 改 / 删 ---

/// 点学名 / 中文名弹出的物种卡片，和「物种名录」页是同一个组件。
/// 鸟类记录没有名录记录的分布 / 编号，所以自己给出一组「记录信息」行。
const card = ref<InstanceType<typeof SpeciesCard> | null>(null)

function openCard(record: BirdRecord): void {
  card.value?.open(record, [
    { label: '记录人', value: record.observer },
    { label: '时间', value: record.observed_at },
    { label: '地点', value: record.location },
    { label: '备注', value: record.note },
    { label: '来源', value: record.source },
    { label: '更新', value: formatTimestamp(record.updated_at) },
  ])
}

/// 「鸟种清单」卡片里的「编辑」：这些是名录记录，跳到「物种名录」页编。
function editSpeciesRecordFromCard(target: SpeciesCardTarget): void {
  if (target.id === undefined) {
    return
  }
  void router.push({ name: 'species-lists', query: { record: target.id } })
}

/// 「重要记录」卡片里的「编辑」：记录就在当前这一页，直接把弹窗套上。
function editBirdRecordFromCard(target: SpeciesCardTarget): void {
  const record = records.value.find((item) => item.id === target.id)
  if (record !== undefined) {
    openEditRecord(record)
  }
}

const dialog = ref<HTMLDialogElement | null>(null)
const form = reactive({
  id: null as string | null,
  /// 选中的鸟种（分类节点的 id）；'' = 还没选。
  taxonId: '',
  scientificName: '',
  chineseName: '',
  observer: '',
  observedAt: '',
  location: '',
  note: '',
  source: '',
  /// 选中的照片（`species_photos.id`）；'' = 没有照片。
  photoId: '',
  /// 已选照片的公开 URL，只用于弹窗里的预览。
  photoUrl: '',
  photoUploading: false,
  submitting: false,
  error: '',
})

function resetForm(): void {
  form.id = null
  form.taxonId = ''
  form.scientificName = ''
  form.chineseName = ''
  form.observer = ''
  form.observedAt = ''
  form.location = ''
  form.note = ''
  form.source = ''
  form.photoId = ''
  form.photoUrl = ''
  form.photoUploading = false
  form.error = ''
  birdKeyword.value = ''
}

function openDialog(): void {
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }
}

function openCreateRecord(): void {
  savedMessage.value = ''
  resetForm()
  openDialog()
}

/// 从物种卡片里新增：物种已经选好了，不用再走一遍 TaxonPicker。
/// `closeCard` 由卡片通过插槽交出来，先把卡片收掉，免得两层弹窗叠在一起。
function openCreateRecordFor(
  target: { taxon_id: string; scientific_name: string; chinese_name: string | null },
  closeCard: () => void,
): void {
  closeCard()
  savedMessage.value = ''
  resetForm()
  form.taxonId = target.taxon_id
  form.scientificName = target.scientific_name
  form.chineseName = target.chinese_name ?? ''
  openDialog()
}

function openEditRecord(record: BirdRecord): void {
  savedMessage.value = ''
  resetForm()
  form.id = record.id
  form.taxonId = record.taxon_id
  form.scientificName = record.scientific_name
  form.chineseName = record.chinese_name ?? ''
  form.observer = record.observer ?? ''
  form.observedAt = record.observed_at ?? ''
  form.location = record.location ?? ''
  form.note = record.note ?? ''
  form.source = record.source ?? ''
  form.photoId = record.photo_id ?? ''
  form.photoUrl = record.photo_url ?? ''
  openDialog()
}

function closeDialog(): void {
  dialog.value?.close()
}

async function submitRecord(): Promise<void> {
  form.error = ''
  if (form.taxonId === '') {
    form.error = '请从列表里选择鸟种'
    return
  }
  const problem = validateField('学名', form.scientificName, BIRD_NAME_MAX_CHARS)
  if (problem !== '') {
    form.error = problem
    return
  }
  const observer = form.observer.trim()
  if (observer !== '' && charCount(observer) > BIRD_OBSERVER_MAX_CHARS) {
    form.error = `记录人最多 ${BIRD_OBSERVER_MAX_CHARS} 个字符`
    return
  }
  const observedAt = form.observedAt.trim()
  if (observedAt !== '' && charCount(observedAt) > BIRD_OBSERVED_AT_MAX_CHARS) {
    form.error = `时间最多 ${BIRD_OBSERVED_AT_MAX_CHARS} 个字符`
    return
  }
  const location = form.location.trim()
  if (location !== '' && charCount(location) > BIRD_LOCATION_MAX_CHARS) {
    form.error = `地点最多 ${BIRD_LOCATION_MAX_CHARS} 个字符`
    return
  }

  form.submitting = true
  try {
    const payload = {
      taxon_id: form.taxonId,
      scientific_name: form.scientificName.trim(),
      chinese_name: optional(form.chineseName),
      observer: optional(form.observer),
      observed_at: optional(form.observedAt),
      location: optional(form.location),
      note: optional(form.note),
      source: optional(form.source),
      photo_id: form.photoId === '' ? null : form.photoId,
    }
    if (form.id === null) {
      const created = await createBirdRecord(payload)
      savedMessage.value = `已新增记录「${created.scientific_name}」`
    } else {
      const updated = await updateBirdRecord(form.id, payload)
      savedMessage.value = `已更新记录「${updated.scientific_name}」`
    }
    closeDialog()
    await loadRecords()
  } catch (e) {
    form.error = errorMessage(e)
  } finally {
    form.submitting = false
  }
}

const deleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const deleteError = ref('')
const deleting = ref(false)
const pendingRecord = ref<BirdRecord | null>(null)

function openDeleteRecord(record: BirdRecord): void {
  savedMessage.value = ''
  deleteError.value = ''
  pendingRecord.value = record
  deleteDialog.value?.open()
}

async function confirmDeleteRecord(): Promise<void> {
  const target = pendingRecord.value
  if (target === null) {
    return
  }
  deleting.value = true
  deleteError.value = ''
  try {
    await deleteBirdRecord(target.id)
    deleteDialog.value?.close()
    savedMessage.value = `已删除记录「${target.scientific_name}」`
    await loadRecords()
  } catch (e) {
    deleteError.value = errorMessage(e)
  } finally {
    deleting.value = false
  }
}

onMounted(async () => {
  try {
    birdClassId.value = await findTaxonId('class', BIRD_CLASS_SCIENTIFIC_NAME)
    if (birdClassId.value === null) {
      speciesError.value = `分类树里没有找到「${BIRD_CLASS_SCIENTIFIC_NAME}」（鸟纲）节点`
    }
  } catch (e) {
    speciesError.value = errorMessage(e)
  }
  await Promise.all([loadSpeciesRecords(), loadRecords(), loadBirdOptions()])
})
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <div>
          <h2>鸟类调查</h2>
          <p class="hint">
            燕园鸟类调查的资料。鸟种清单是鸟纲下的名录记录，重要记录来自《北京大学燕园校区鸟类记录整理.docx》。
          </p>
        </div>
      </div>

      <div class="tabs">
        <button
          v-for="item in TABS"
          :key="item.key"
          class="tab"
          :class="{ active: tab === item.key }"
          type="button"
          @click="tab = item.key"
        >
          {{ item.label }}
        </button>
      </div>

      <!-- 两个标签共用一个提示位：从「鸟种清单」里新增完也在这一屏看得到 -->
      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>

      <!-- 鸟种清单 -->
      <template v-if="tab === 'species'">
        <form class="search-bar" @submit.prevent="searchSpecies">
          <input v-model="speciesKeyword" placeholder="按学名或中文名搜索…" />
          <button type="submit">搜索</button>
          <button
            v-if="speciesAppliedKeyword !== ''"
            class="secondary"
            type="button"
            @click="clearSpeciesSearch"
          >
            清除
          </button>
        </form>

        <p v-if="speciesError" class="alert error">{{ speciesError }}</p>

        <SpeciesRecordTable
          :records="speciesRecords"
          :loading="speciesLoading"
          :on-edit-record="editSpeciesRecordFromCard"
        >
          <template #card-actions="{ record, close }">
            <button v-if="canManage" type="button" @click="openCreateRecordFor(record, close)">
              添加新重要记录
            </button>
          </template>
          <template #empty>
            <span v-if="speciesAppliedKeyword !== ''">
              没有匹配「{{ speciesAppliedKeyword }}」的鸟种
            </span>
            <span v-else>鸟纲下还没有名录记录</span>
          </template>
        </SpeciesRecordTable>

        <ListPager
          :total="speciesTotal"
          :page-size="PAGE_SIZE"
          :page="speciesOffset / PAGE_SIZE + 1"
          :disabled="speciesLoading"
          @change="goToSpeciesPage"
        />
      </template>

      <!-- 重要记录 -->
      <template v-else-if="tab === 'records'">
        <form class="search-bar" @submit.prevent="searchRecords">
          <input v-model="recordsKeyword" placeholder="按学名 / 中文名 / 记录人 / 地点搜索…" />
          <input v-model="recordsFrom" class="date" type="date" aria-label="起始日期" />
          <span class="muted">至</span>
          <input v-model="recordsTo" class="date" type="date" aria-label="结束日期" />
          <button type="submit">搜索</button>
          <button
            v-if="recordsFiltered"
            class="secondary"
            type="button"
            @click="clearRecordsSearch"
          >
            清除
          </button>
          <button v-if="canManage" type="button" @click="openCreateRecord">新增记录</button>
        </form>

        <p class="hint">
          时间范围含端点，按记录的日期区间匹配：写「月底」的落在该月下旬，只写年份的整年都算。
        </p>

        <p v-if="recordsError" class="alert error">{{ recordsError }}</p>
        <p v-if="recordsLoading" class="muted">加载中…</p>

        <div v-else class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th>学名</th>
                <th>中文名</th>
                <th>记录人</th>
                <th>时间</th>
                <th>地点</th>
                <th>备注</th>
                <th v-if="canManage"></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="record in records" :key="record.id">
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
                <td class="muted nowrap">{{ record.observer ?? '—' }}</td>
                <td class="muted nowrap">{{ record.observed_at ?? '—' }}</td>
                <td class="muted">
                  <span class="clip" :title="record.location ?? undefined">
                    {{ record.location ?? '—' }}
                  </span>
                </td>
                <td class="muted">
                  <NoteText :note="record.note" clip />
                </td>
                <td v-if="canManage" class="actions-cell">
                  <button class="secondary" type="button" @click="openEditRecord(record)">
                    编辑
                  </button>
                  <button class="danger" type="button" @click="openDeleteRecord(record)">
                    删除
                  </button>
                </td>
              </tr>
              <tr v-if="records.length === 0">
                <td :colspan="canManage ? 7 : 6" class="muted">
                  <span v-if="recordsFiltered">没有符合当前条件的记录</span>
                  <span v-else>还没有重要记录</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <ListPager
          :total="recordsTotal"
          :page-size="PAGE_SIZE"
          :page="recordsOffset / PAGE_SIZE + 1"
          :disabled="recordsLoading"
          @change="goToRecordsPage"
        />
      </template>

      <!-- 鸟调记录 -->
      <template v-else>
        <p class="muted">鸟调记录还没有开始录入。</p>
      </template>
    </section>
  </div>

  <!-- 重要记录的增 / 改 -->
  <dialog ref="dialog" class="modal modal-wide" @close="resetForm" @click.self="closeDialog">
    <form @submit.prevent="submitRecord">
      <h2>{{ form.id === null ? '新增重要记录' : '编辑重要记录' }}</h2>
      <p v-if="form.error" class="alert error">{{ form.error }}</p>
      <p v-if="birdOptionsError" class="alert error">{{ birdOptionsError }}</p>

      <div class="field">
        <label for="bird-filter">物种（限鸟纲，只选到鸟种）</label>
        <input id="bird-filter" v-model="birdKeyword" placeholder="先输入中文名或学名筛选…" />
        <select id="bird-taxon" v-model="form.taxonId" @change="pickBird">
          <option value="">— 请选择鸟种 —</option>
          <option v-for="bird in filteredBirds" :key="bird.taxon_id" :value="bird.taxon_id">
            {{ taxonLabel(bird) }}
          </option>
        </select>
        <span class="hint">学名与中文名随鸟种自动带出，不用手填</span>
      </div>

      <div class="field">
        <label for="bird-observer">记录人</label>
        <input id="bird-observer" v-model="form.observer" :maxlength="BIRD_OBSERVER_MAX_CHARS" />
      </div>

      <div class="field">
        <label for="bird-observed-at">时间</label>
        <input
          id="bird-observed-at"
          v-model="form.observedAt"
          :maxlength="BIRD_OBSERVED_AT_MAX_CHARS"
        />
        <span class="hint">自由文本，原稿里是「2025年3月19日」「2023年5月底」这类写法</span>
      </div>

      <div class="field">
        <label for="bird-location">地点</label>
        <input id="bird-location" v-model="form.location" :maxlength="BIRD_LOCATION_MAX_CHARS" />
      </div>

      <div class="field">
        <label for="bird-note">备注</label>
        <textarea id="bird-note" v-model="form.note" rows="3"></textarea>
      </div>

      <div class="field">
        <label for="bird-source">来源</label>
        <input id="bird-source" v-model="form.source" />
      </div>

      <div class="field">
        <label>照片</label>
        <div v-if="form.photoUrl !== ''" class="bird-photo">
          <img :src="form.photoUrl" alt="记录照片" />
          <button class="secondary" type="button" @click="clearPhoto">移除</button>
        </div>
        <div v-else class="actions">
          <input
            ref="photoFileInput"
            type="file"
            accept="image/jpeg,image/png,image/gif,image/webp"
            hidden
            @change="uploadRecordPhoto"
          />
          <button
            class="secondary"
            type="button"
            :disabled="form.photoUploading || form.taxonId === ''"
            @click="photoFileInput?.click()"
          >
            {{ form.photoUploading ? '上传中…' : '上传照片' }}
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="form.taxonId === ''"
            @click="openPhotoPicker"
          >
            选择已有照片
          </button>
        </div>
        <span class="hint"
          >可以上传新照片，也可以从这只鸟已有的照片里挑一张。照片存在「物种照片」里，同一物种最多 9
          张；关联后会自动打上「重要记录」标记。</span
        >
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

  <!-- 「选择已有照片」：只列当前鸟种的照片，点一张就选上 -->
  <dialog ref="photoPicker" class="modal modal-wide" @click.self="closePhotoPicker">
    <h2>选择已有照片</h2>
    <p v-if="pickerError" class="alert error">{{ pickerError }}</p>
    <p v-if="pickerLoading" class="muted">加载中…</p>
    <p v-else-if="pickerPhotos.length === 0" class="muted">
      这只鸟还没有照片，先用「上传照片」传一张。
    </p>
    <div v-else class="photo-grid">
      <figure v-for="photo in pickerPhotos" :key="photo.id" class="photo-card">
        <button class="photo-pick" type="button" @click="choosePhoto(photo)">
          <img :src="photo.url" alt="候选照片" loading="lazy" />
        </button>
        <figcaption class="photo-meta">
          <span class="photo-summary">{{ photoSummary(photo) }}</span>
        </figcaption>
      </figure>
    </div>
    <div class="actions">
      <button class="secondary" type="button" @click="closePhotoPicker">取消</button>
    </div>
  </dialog>

  <ConfirmDialog
    ref="deleteDialog"
    title="删除重要记录"
    :message="
      pendingRecord === null
        ? ''
        : `确定删除「${pendingRecord.scientific_name}」的记录？此操作不可撤销。`
    "
    :busy="deleting"
    :error="deleteError"
    @confirm="confirmDeleteRecord"
    @cancel="deleteError = ''"
  />

  <!-- 点学名 / 中文名打开的重要记录卡片：只看分类阶元与记录信息。
       照片在记录的编辑弹窗里挂，卡片上不再放「上传照片」/「新增记录」入口。 -->
  <SpeciesCard ref="card" :on-edit-record="editBirdRecordFromCard" hide-upload-photo />
</template>
