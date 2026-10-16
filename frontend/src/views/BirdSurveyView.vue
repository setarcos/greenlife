<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  createBirdRecord,
  deleteBirdRecord,
  listBirdRecords,
  updateBirdRecord,
} from '../api/birdRecords'
import {
  createBirdSurvey,
  createSurveyRecord,
  deleteBirdSurvey,
  deleteSurveyRecord,
  getBirdSurvey,
  importBirdSurvey,
  listBirdSurveys,
  updateBirdSurvey,
  updateSurveyRecord,
} from '../api/birdSurveys'
import { errorMessage } from '../api/http'
import { createPhoto, listPhotos } from '../api/photos'
import { findTaxonId, listRecords, listSpeciesLists } from '../api/taxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import NoteText from '../components/NoteText.vue'
import ListPager from '../components/ListPager.vue'
import SpeciesRecordDialog from '../components/SpeciesRecordDialog.vue'
import SpeciesRecordTable from '../components/SpeciesRecordTable.vue'
import SpeciesCard, { type SpeciesCardTarget } from '../components/SpeciesCard.vue'
import { useAuthStore } from '../stores/auth'
import { BIRD_CLASS_SCIENTIFIC_NAME, type BirdRecord } from '../birds'
import type { BirdSurvey, BirdSurveyDetail, BirdSurveyRecord } from '../birdSurveys'
import { clampOffset } from '../pagination'
import { MAX_PHOTOS_PER_TAXON, validatePhotoFile, type SpeciesPhoto } from '../photos'
import { pageOffset, queryPage, queryText, setPage, setText } from '../query'
import { taxonLabel, type SpeciesRecord } from '../taxonomy'
import { formatTimestamp } from '../types'
import {
  BIRD_LOCATION_MAX_CHARS,
  BIRD_NAME_MAX_CHARS,
  BIRD_OBSERVED_AT_MAX_CHARS,
  BIRD_OBSERVER_MAX_CHARS,
  SURVEY_LEADER_MAX_CHARS,
  SURVEY_RECORD_NAME_MAX_CHARS,
  SURVEY_WEATHER_MAX_CHARS,
  charCount,
  validateField,
} from '../validation'

/// 「鸟类调查」：三个标签页共用一张表页面的骨架。
///
/// - 鸟种清单：鸟纲（Aves）下的全部名录记录，直接复用 `/taxonomy/records`。
/// - 重要记录：`bird_records` 表，docx 导入 + STAFF 手工补录。
/// - 鸟调记录：`bird_surveys` / `bird_survey_records`，GPX 导入 + 手工维护。
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const canManage = computed(() => auth.canManage)

const TABS = [
  { key: 'species', label: '鸟种清单' },
  { key: 'records', label: '重要记录' },
  { key: 'survey', label: '鸟调记录' },
] as const
type TabKey = (typeof TABS)[number]['key']
const tab = ref<TabKey>('species')

/// 切标签页。三个标签的数据在挂载时都拉过，切回来不用重新请求。
function selectTab(key: TabKey): void {
  tab.value = key
  writeQuery()
}

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
    // 页码是从地址栏来的，可能已经越过末页（链接是别人发的 / 记录被删了），
    // 退回去重拉一次。
    const clamped = clampOffset(speciesOffset.value, page.total, PAGE_SIZE)
    if (clamped !== speciesOffset.value) {
      speciesOffset.value = clamped
      writeQuery()
      await loadSpeciesRecords()
      return
    }
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
  writeQuery()
  await loadSpeciesRecords()
}

async function clearSpeciesSearch(): Promise<void> {
  speciesKeyword.value = ''
  speciesAppliedKeyword.value = ''
  speciesOffset.value = 0
  writeQuery()
  await loadSpeciesRecords()
}

async function goToSpeciesPage(page: number): Promise<void> {
  speciesOffset.value = (page - 1) * PAGE_SIZE
  writeQuery()
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

async function searchRecords(): Promise<void> {
  recordsAppliedKeyword.value = recordsKeyword.value.trim()
  recordsAppliedFrom.value = recordsFrom.value
  recordsAppliedTo.value = recordsTo.value
  recordsOffset.value = 0
  writeQuery()
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
  writeQuery()
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
  writeQuery()
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

/// 「鸟种清单」卡片里的「编辑」：这些是名录记录，就地开弹窗编，编完留在这一屏。
/// 记录属于哪份名录就还写回哪份 —— 这份清单是跨名录的，名录由记录自己带。
const speciesRecordDialog = ref<InstanceType<typeof SpeciesRecordDialog> | null>(null)
/// 名录 id → 名录名，编辑弹窗里的「所属名录」那一行要用。
const listNames = ref<Record<string, string>>({})

async function loadListNames(): Promise<void> {
  try {
    const lists = await listSpeciesLists()
    listNames.value = Object.fromEntries(lists.map((list) => [list.id, list.name]))
  } catch {
    // 拿不到名字只影响弹窗里那一行的显示，不用报错。
  }
}

function editSpeciesRecordFromCard(target: SpeciesCardTarget): void {
  const record = speciesRecords.value.find((item) => item.id === target.id)
  if (record === undefined) {
    return
  }
  savedMessage.value = ''
  speciesRecordDialog.value?.open({
    listId: record.list_id,
    listName: listNames.value[record.list_id],
    record,
  })
}

/// 保存成功：回显提示，刷新鸟种清单（名字改了，重要记录的下拉框也跟着变）。
async function onSpeciesRecordSaved(record: SpeciesRecord, created: boolean): Promise<void> {
  savedMessage.value = `已${created ? '新增' : '更新'}记录「${record.scientific_name}」`
  await Promise.all([loadSpeciesRecords(), loadBirdOptions()])
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

// ==========================================================================
// 鸟调记录：表头（日期 / 领队 / 天气 / 鸟种数 / 备注）+ GPX 导入 + 明细维护
// ==========================================================================

const surveys = ref<BirdSurvey[]>([])
const surveysLoading = ref(false)
const surveysError = ref('')
const surveysKeyword = ref('')
const surveysAppliedKeyword = ref('')
/// 日期范围（`yyyy-mm-dd`）；'' = 不限。
const surveysFrom = ref('')
const surveysTo = ref('')
const surveysAppliedFrom = ref('')
const surveysAppliedTo = ref('')
const surveysOffset = ref(0)
const surveysTotal = ref(0)

async function loadSurveys(): Promise<void> {
  surveysLoading.value = true
  surveysError.value = ''
  try {
    const page = await listBirdSurveys({
      q: surveysAppliedKeyword.value === '' ? undefined : surveysAppliedKeyword.value,
      from: surveysAppliedFrom.value === '' ? undefined : surveysAppliedFrom.value,
      to: surveysAppliedTo.value === '' ? undefined : surveysAppliedTo.value,
      limit: PAGE_SIZE,
      offset: surveysOffset.value,
    })
    const clamped = clampOffset(surveysOffset.value, page.total, PAGE_SIZE)
    if (clamped !== surveysOffset.value) {
      surveysOffset.value = clamped
      writeQuery()
      await loadSurveys()
      return
    }
    surveys.value = page.items
    surveysTotal.value = page.total
  } catch (e) {
    surveysError.value = errorMessage(e)
  } finally {
    surveysLoading.value = false
  }
}

async function searchSurveys(): Promise<void> {
  surveysAppliedKeyword.value = surveysKeyword.value.trim()
  surveysAppliedFrom.value = surveysFrom.value
  surveysAppliedTo.value = surveysTo.value
  surveysOffset.value = 0
  writeQuery()
  await loadSurveys()
}

async function clearSurveysSearch(): Promise<void> {
  surveysKeyword.value = ''
  surveysAppliedKeyword.value = ''
  surveysFrom.value = ''
  surveysTo.value = ''
  surveysAppliedFrom.value = ''
  surveysAppliedTo.value = ''
  surveysOffset.value = 0
  writeQuery()
  await loadSurveys()
}

const surveysFiltered = computed(
  () =>
    surveysAppliedKeyword.value !== '' ||
    surveysAppliedFrom.value !== '' ||
    surveysAppliedTo.value !== '',
)

async function goToSurveysPage(page: number): Promise<void> {
  surveysOffset.value = (page - 1) * PAGE_SIZE
  writeQuery()
  await loadSurveys()
}

/// 明细弹窗里的「位置」列：经纬度都在才显示。
function formatCoordinates(record: BirdSurveyRecord): string {
  if (record.latitude === null || record.longitude === null) {
    return '—'
  }
  return `${record.latitude.toFixed(5)}, ${record.longitude.toFixed(5)}`
}

/// 「位置」后面的地图链接：高德 URI API，`position` 是「经度,纬度」。
/// GPX 里是 GPS 原始坐标（WGS-84），要靠 `coordinate=wgs84` 让高德自己纠偏。
function amapUrl(record: BirdSurveyRecord): string | null {
  if (record.latitude === null || record.longitude === null) {
    return null
  }
  const position = `${record.longitude},${record.latitude}`
  const params = new URLSearchParams({
    position,
    coordinate: 'wgs84',
    name: record.chinese_name,
    src: 'greenlife',
  })
  return `https://uri.amap.com/marker?${params.toString()}`
}

// --- 明细弹窗 ---

const detailDialog = ref<HTMLDialogElement | null>(null)
const detail = ref<BirdSurveyDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref('')

async function loadDetail(surveyId: string): Promise<void> {
  detailLoading.value = true
  detailError.value = ''
  try {
    detail.value = await getBirdSurvey(surveyId)
  } catch (e) {
    detailError.value = errorMessage(e)
  } finally {
    detailLoading.value = false
  }
}

async function openDetail(survey: BirdSurvey): Promise<void> {
  savedMessage.value = ''
  detail.value = null
  detailError.value = ''
  expandedRecordIds.value = new Set()
  if (detailDialog.value !== null && !detailDialog.value.open) {
    detailDialog.value.showModal()
  }
  await loadDetail(survey.id)
}

/// 明细里哪些记录展开了「时间 / 位置 / 备注」。
const expandedRecordIds = ref<Set<string>>(new Set())

function toggleRecordDetails(recordId: string): void {
  const next = new Set(expandedRecordIds.value)
  if (next.has(recordId)) {
    next.delete(recordId)
  } else {
    next.add(recordId)
  }
  expandedRecordIds.value = next
}

function closeDetail(): void {
  detailDialog.value?.close()
}

/// 明细增删改后表头也要重拉：鸟种数是后端按明细现算的。
async function refreshDetail(): Promise<void> {
  if (detail.value !== null) {
    await loadDetail(detail.value.id)
  }
  await loadSurveys()
}

// --- 导入 GPX ---

const importDialog = ref<HTMLDialogElement | null>(null)
const importForm = reactive({
  file: null as File | null,
  leader: '',
  surveyDate: '',
  weather: '',
  note: '',
  importing: false,
  error: '',
  result: '',
})

/// 从 `20180522北大鸟调多云转晴.gpx` 里猜出日期和天气，作为导入表单的默认值。
/// 猜不出就留空让用户自己填（后端缺日期时也会退到最早 waypoint 的日期）。
function parseGpxFilename(filename: string): { date: string; weather: string } {
  const base = filename.replace(/\.[^.]+$/, '')
  const match = base.match(/^(\d{4})(\d{2})(\d{2})/)
  const date = match === null ? '' : `${match[1]}-${match[2]}-${match[3]}`
  const rest = (match === null ? base : base.slice(8)).replace(/北大鸟调|鸟调记录|鸟调/g, '').trim()
  return { date, weather: rest }
}

function resetImportForm(): void {
  importForm.file = null
  importForm.leader = ''
  importForm.surveyDate = ''
  importForm.weather = ''
  importForm.note = ''
  importForm.error = ''
  importForm.result = ''
}

function openImport(): void {
  savedMessage.value = ''
  resetImportForm()
  if (importDialog.value !== null && !importDialog.value.open) {
    importDialog.value.showModal()
  }
}

function closeImport(): void {
  importDialog.value?.close()
}

function onImportFileChange(event: Event): void {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0] ?? null
  importForm.file = file
  if (file === null) {
    return
  }
  const guessed = parseGpxFilename(file.name)
  if (importForm.surveyDate === '') {
    importForm.surveyDate = guessed.date
  }
  if (importForm.weather === '') {
    importForm.weather = guessed.weather
  }
}

async function submitImport(): Promise<void> {
  importForm.error = ''
  importForm.result = ''
  if (importForm.file === null) {
    importForm.error = '请选择 GPX 文件'
    return
  }
  const leader = importForm.leader.trim()
  if (leader === '') {
    importForm.error = '请填写领队'
    return
  }
  if (charCount(leader) > SURVEY_LEADER_MAX_CHARS) {
    importForm.error = `领队最多 ${SURVEY_LEADER_MAX_CHARS} 个字符`
    return
  }
  const weather = importForm.weather.trim()
  if (charCount(weather) > SURVEY_WEATHER_MAX_CHARS) {
    importForm.error = `天气最多 ${SURVEY_WEATHER_MAX_CHARS} 个字符`
    return
  }

  importForm.importing = true
  try {
    const created = await importBirdSurvey({
      file: importForm.file,
      leader,
      surveyDate: importForm.surveyDate,
      weather,
      note: importForm.note.trim(),
    })
    const unmatched = [
      ...new Set(
        created.records
          .filter((record) => record.taxon_id === null)
          .map((record) => record.chinese_name),
      ),
    ]
    let message = `已导入 ${created.survey_date} 的鸟调：${created.species_count} 个鸟种 / ${created.records.length} 条观察`
    if (unmatched.length > 0) {
      message += `。未匹配（保留原名）：${unmatched.join('、')}`
    }
    importForm.result = message
    await loadSurveys()
  } catch (e) {
    importForm.error = errorMessage(e)
  } finally {
    importForm.importing = false
  }
}

// --- 新增 / 编辑鸟调表头 ---

const surveyDialog = ref<HTMLDialogElement | null>(null)
const surveyForm = reactive({
  id: null as string | null,
  surveyDate: '',
  leader: '',
  weather: '',
  note: '',
  submitting: false,
  error: '',
})

function resetSurveyForm(): void {
  surveyForm.id = null
  surveyForm.surveyDate = today()
  surveyForm.leader = ''
  surveyForm.weather = ''
  surveyForm.note = ''
  surveyForm.error = ''
}

function openCreateSurvey(): void {
  savedMessage.value = ''
  resetSurveyForm()
  if (surveyDialog.value !== null && !surveyDialog.value.open) {
    surveyDialog.value.showModal()
  }
}

function openEditSurvey(survey: BirdSurvey): void {
  savedMessage.value = ''
  resetSurveyForm()
  surveyForm.id = survey.id
  surveyForm.surveyDate = survey.survey_date
  surveyForm.leader = survey.leader ?? ''
  surveyForm.weather = survey.weather ?? ''
  surveyForm.note = survey.note ?? ''
  if (surveyDialog.value !== null && !surveyDialog.value.open) {
    surveyDialog.value.showModal()
  }
}

function closeSurveyDialog(): void {
  surveyDialog.value?.close()
}

async function submitSurvey(): Promise<void> {
  surveyForm.error = ''
  if (surveyForm.surveyDate === '') {
    surveyForm.error = '请选择日期'
    return
  }
  if (charCount(surveyForm.leader.trim()) > SURVEY_LEADER_MAX_CHARS) {
    surveyForm.error = `领队最多 ${SURVEY_LEADER_MAX_CHARS} 个字符`
    return
  }
  if (charCount(surveyForm.weather.trim()) > SURVEY_WEATHER_MAX_CHARS) {
    surveyForm.error = `天气最多 ${SURVEY_WEATHER_MAX_CHARS} 个字符`
    return
  }

  surveyForm.submitting = true
  try {
    if (surveyForm.id === null) {
      const created = await createBirdSurvey({
        survey_date: surveyForm.surveyDate,
        leader: optional(surveyForm.leader),
        weather: optional(surveyForm.weather),
        note: optional(surveyForm.note),
      })
      savedMessage.value = `已新增 ${created.survey_date} 的鸟调记录`
    } else {
      const updated = await updateBirdSurvey(surveyForm.id, {
        survey_date: surveyForm.surveyDate,
        leader: optional(surveyForm.leader),
        weather: optional(surveyForm.weather),
        note: optional(surveyForm.note),
      })
      savedMessage.value = `已更新 ${updated.survey_date} 的鸟调记录`
      if (detail.value !== null && detail.value.id === updated.id) {
        await loadDetail(updated.id)
      }
    }
    closeSurveyDialog()
    await loadSurveys()
  } catch (e) {
    surveyForm.error = errorMessage(e)
  } finally {
    surveyForm.submitting = false
  }
}

// --- 新增 / 编辑鸟种明细 ---

/// 下拉框里「未匹配」那一项的值：选它就不关联分类，自己填中文名。
const MANUAL_TAXON = '__manual__'

const surveyRecordDialog = ref<HTMLDialogElement | null>(null)
const surveyRecordBirdKeyword = ref('')
const surveyRecordForm = reactive({
  id: null as string | null,
  taxonId: MANUAL_TAXON,
  scientificName: '',
  chineseName: '',
  quantity: '',
  observedAt: '',
  latitude: '',
  longitude: '',
  note: '',
  submitting: false,
  error: '',
})

function resetSurveyRecordForm(): void {
  surveyRecordForm.id = null
  surveyRecordForm.taxonId = MANUAL_TAXON
  surveyRecordForm.scientificName = ''
  surveyRecordForm.chineseName = ''
  surveyRecordForm.quantity = ''
  surveyRecordForm.observedAt = ''
  surveyRecordForm.latitude = ''
  surveyRecordForm.longitude = ''
  surveyRecordForm.note = ''
  surveyRecordForm.error = ''
  surveyRecordBirdKeyword.value = ''
}

function openCreateSurveyRecord(): void {
  savedMessage.value = ''
  resetSurveyRecordForm()
  if (surveyRecordDialog.value !== null && !surveyRecordDialog.value.open) {
    surveyRecordDialog.value.showModal()
  }
}

/// `<input type="datetime-local">` 只认 `yyyy-mm-ddThh:mm`，naive datetime 截掉秒。
function toLocalInput(value: string | null): string {
  return value === null ? '' : value.slice(0, 16)
}

function openEditSurveyRecord(record: BirdSurveyRecord): void {
  savedMessage.value = ''
  resetSurveyRecordForm()
  surveyRecordForm.id = record.id
  surveyRecordForm.taxonId = record.taxon_id ?? MANUAL_TAXON
  surveyRecordForm.scientificName = record.scientific_name ?? ''
  surveyRecordForm.chineseName = record.chinese_name
  surveyRecordForm.quantity = record.quantity === null ? '' : String(record.quantity)
  surveyRecordForm.observedAt = toLocalInput(record.observed_at)
  surveyRecordForm.latitude = record.latitude === null ? '' : String(record.latitude)
  surveyRecordForm.longitude = record.longitude === null ? '' : String(record.longitude)
  surveyRecordForm.note = record.note ?? ''
  if (surveyRecordDialog.value !== null && !surveyRecordDialog.value.open) {
    surveyRecordDialog.value.showModal()
  }
}

function closeSurveyRecordDialog(): void {
  surveyRecordDialog.value?.close()
}

/// 选了鸟种就把学名 / 中文名带出来；具体名字以后端从分类树查到的为准。
function pickSurveyRecordBird(): void {
  const bird = birdOptions.value.find((item) => item.taxon_id === surveyRecordForm.taxonId)
  surveyRecordForm.scientificName = bird?.scientific_name ?? ''
  surveyRecordForm.chineseName = bird?.chinese_name ?? ''
}

/// 选中的鸟种不能被筛选词筛掉，否则下拉框会显示成空白。
const filteredSurveyRecordBirds = computed(() => {
  const all = birdOptions.value
  const keyword = surveyRecordBirdKeyword.value.trim().toLowerCase()
  if (keyword === '') {
    return all
  }
  const matched = all.filter((bird) => taxonLabel(bird).toLowerCase().includes(keyword))
  const chosen = all.find((bird) => bird.taxon_id === surveyRecordForm.taxonId)
  if (chosen !== undefined && !matched.includes(chosen)) {
    matched.unshift(chosen)
  }
  return matched
})

/// 数字输入框：空串 -> null。
function optionalNumber(value: string): number | null {
  const text = value.trim()
  return text === '' ? null : Number(text)
}

/// `datetime-local` 给的是 `yyyy-mm-ddThh:mm`，补上秒让 chrono 能解析。
function normalizeObservedAt(value: string): string | null {
  const text = value.trim()
  if (text === '') {
    return null
  }
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(text) ? `${text}:00` : text
}

async function submitSurveyRecord(): Promise<void> {
  surveyRecordForm.error = ''
  if (detail.value === null) {
    return
  }
  const manual = surveyRecordForm.taxonId === MANUAL_TAXON
  if (manual && surveyRecordForm.chineseName.trim() === '') {
    surveyRecordForm.error = '请填写鸟种名称'
    return
  }
  if (manual && charCount(surveyRecordForm.chineseName.trim()) > SURVEY_RECORD_NAME_MAX_CHARS) {
    surveyRecordForm.error = `鸟种名称最多 ${SURVEY_RECORD_NAME_MAX_CHARS} 个字符`
    return
  }
  const quantity = optionalNumber(surveyRecordForm.quantity)
  if (quantity !== null && (!Number.isInteger(quantity) || quantity < 0)) {
    surveyRecordForm.error = '数量必须是非负整数'
    return
  }
  const latitude = optionalNumber(surveyRecordForm.latitude)
  if (latitude !== null && (Number.isNaN(latitude) || latitude < -90 || latitude > 90)) {
    surveyRecordForm.error = '纬度必须在 -90 到 90 之间'
    return
  }
  const longitude = optionalNumber(surveyRecordForm.longitude)
  if (longitude !== null && (Number.isNaN(longitude) || longitude < -180 || longitude > 180)) {
    surveyRecordForm.error = '经度必须在 -180 到 180 之间'
    return
  }

  surveyRecordForm.submitting = true
  try {
    const payload = {
      taxon_id: manual ? null : surveyRecordForm.taxonId,
      scientific_name: manual ? optional(surveyRecordForm.scientificName) : null,
      chinese_name: manual ? surveyRecordForm.chineseName.trim() : null,
      quantity,
      observed_at: normalizeObservedAt(surveyRecordForm.observedAt),
      latitude,
      longitude,
      note: optional(surveyRecordForm.note),
    }
    if (surveyRecordForm.id === null) {
      const created = await createSurveyRecord(detail.value.id, payload)
      savedMessage.value = `已添加鸟种「${created.chinese_name}」`
    } else {
      const updated = await updateSurveyRecord(detail.value.id, surveyRecordForm.id, payload)
      savedMessage.value = `已更新鸟种「${updated.chinese_name}」`
    }
    closeSurveyRecordDialog()
    await refreshDetail()
  } catch (e) {
    surveyRecordForm.error = errorMessage(e)
  } finally {
    surveyRecordForm.submitting = false
  }
}

// --- 删除鸟调 / 删除明细 ---

const surveyDeleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const surveyDeleteError = ref('')
const surveyDeleting = ref(false)
const pendingSurvey = ref<BirdSurvey | null>(null)

function openDeleteSurvey(survey: BirdSurvey): void {
  savedMessage.value = ''
  surveyDeleteError.value = ''
  pendingSurvey.value = survey
  surveyDeleteDialog.value?.open()
}

async function confirmDeleteSurvey(): Promise<void> {
  const target = pendingSurvey.value
  if (target === null) {
    return
  }
  surveyDeleting.value = true
  surveyDeleteError.value = ''
  try {
    await deleteBirdSurvey(target.id)
    surveyDeleteDialog.value?.close()
    savedMessage.value = `已删除 ${target.survey_date} 的鸟调记录`
    if (detail.value !== null && detail.value.id === target.id) {
      closeDetail()
    }
    await loadSurveys()
  } catch (e) {
    surveyDeleteError.value = errorMessage(e)
  } finally {
    surveyDeleting.value = false
  }
}

const surveyRecordDeleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const surveyRecordDeleteError = ref('')
const surveyRecordDeleting = ref(false)
const pendingSurveyRecord = ref<BirdSurveyRecord | null>(null)

function openDeleteSurveyRecord(record: BirdSurveyRecord): void {
  savedMessage.value = ''
  surveyRecordDeleteError.value = ''
  pendingSurveyRecord.value = record
  surveyRecordDeleteDialog.value?.open()
}

async function confirmDeleteSurveyRecord(): Promise<void> {
  const target = pendingSurveyRecord.value
  if (target === null || detail.value === null) {
    return
  }
  surveyRecordDeleting.value = true
  surveyRecordDeleteError.value = ''
  try {
    await deleteSurveyRecord(detail.value.id, target.id)
    surveyRecordDeleteDialog.value?.close()
    savedMessage.value = `已删除鸟种「${target.chinese_name}」`
    await refreshDetail()
  } catch (e) {
    surveyRecordDeleteError.value = errorMessage(e)
  } finally {
    surveyRecordDeleting.value = false
  }
}

// ==========================================================================
// 视角状态 ↔ 地址栏
// ==========================================================================

/// 当前在哪个标签页、翻到第几页、筛选了什么，全部写进地址栏：从这里的物种卡片跳到
/// 「物种照片」再按后退、刷新页面、把链接发给别人，都能回到同一屏。三个标签各占一套
/// 参数（`s_` / `r_` / `v_` 前缀），互不干扰；只写非默认值，所以没筛选时链接还是
/// 干净的 `/birds`。
function writeQuery(): void {
  const query: LocationQueryRaw = {}
  if (tab.value !== 'species') {
    query.tab = tab.value
  }
  setPage(query, 's_page', speciesOffset.value, PAGE_SIZE)
  setText(query, 's_q', speciesAppliedKeyword.value)
  setPage(query, 'r_page', recordsOffset.value, PAGE_SIZE)
  setText(query, 'r_q', recordsAppliedKeyword.value)
  setText(query, 'r_from', recordsAppliedFrom.value)
  setText(query, 'r_to', recordsAppliedTo.value)
  setPage(query, 'v_page', surveysOffset.value, PAGE_SIZE)
  setText(query, 'v_q', surveysAppliedKeyword.value)
  setText(query, 'v_from', surveysAppliedFrom.value)
  setText(query, 'v_to', surveysAppliedTo.value)
  // replace：调一次筛选不该在历史里多留一条。
  void router.replace({ query })
}

/// 从地址栏读回视角状态，返回哪些标签的数据需要重拉。首次挂载、以及「路由变了但
/// 组件没重建」（点顶栏的「鸟类调查」、浏览器前进后退）都走这里。
/// 我们自己刚 replace 出去的 query 就是当前状态，读出来跟 ref 一样，所以这里不会
/// 重复发请求。
function applyRouteQuery(): TabKey[] {
  const query = route.query
  const changed: TabKey[] = []

  const tabParam = queryText(query.tab)
  tab.value = TABS.find((item) => item.key === tabParam)?.key ?? 'species'

  const speciesPage = pageOffset(queryPage(query.s_page), PAGE_SIZE)
  const speciesWord = queryText(query.s_q)
  if (speciesPage !== speciesOffset.value || speciesWord !== speciesAppliedKeyword.value) {
    speciesOffset.value = speciesPage
    speciesAppliedKeyword.value = speciesWord
    speciesKeyword.value = speciesWord
    changed.push('species')
  }

  const recordsPage = pageOffset(queryPage(query.r_page), PAGE_SIZE)
  const recordsWord = queryText(query.r_q)
  const recordsStart = queryText(query.r_from)
  const recordsEnd = queryText(query.r_to)
  if (
    recordsPage !== recordsOffset.value ||
    recordsWord !== recordsAppliedKeyword.value ||
    recordsStart !== recordsAppliedFrom.value ||
    recordsEnd !== recordsAppliedTo.value
  ) {
    recordsOffset.value = recordsPage
    recordsAppliedKeyword.value = recordsWord
    recordsKeyword.value = recordsWord
    recordsAppliedFrom.value = recordsStart
    recordsFrom.value = recordsStart
    recordsAppliedTo.value = recordsEnd
    recordsTo.value = recordsEnd
    changed.push('records')
  }

  const surveysPage = pageOffset(queryPage(query.v_page), PAGE_SIZE)
  const surveysWord = queryText(query.v_q)
  const surveysStart = queryText(query.v_from)
  const surveysEnd = queryText(query.v_to)
  if (
    surveysPage !== surveysOffset.value ||
    surveysWord !== surveysAppliedKeyword.value ||
    surveysStart !== surveysAppliedFrom.value ||
    surveysEnd !== surveysAppliedTo.value
  ) {
    surveysOffset.value = surveysPage
    surveysAppliedKeyword.value = surveysWord
    surveysKeyword.value = surveysWord
    surveysAppliedFrom.value = surveysStart
    surveysFrom.value = surveysStart
    surveysAppliedTo.value = surveysEnd
    surveysTo.value = surveysEnd
    changed.push('survey')
  }

  return changed
}

/// 按地址栏重新对齐并重拉需要的数据。
async function syncFromRoute(): Promise<void> {
  const changed = applyRouteQuery()
  const loads: Promise<void>[] = []
  if (changed.includes('species')) {
    loads.push(loadSpeciesRecords())
  }
  if (changed.includes('records')) {
    loads.push(loadRecords())
  }
  if (changed.includes('survey')) {
    loads.push(loadSurveys())
  }
  await Promise.all(loads)
}

onMounted(async () => {
  // 先把地址栏里的标签页 / 页码 / 筛选词读回来，再按它去请求数据。
  applyRouteQuery()
  try {
    birdClassId.value = await findTaxonId('class', BIRD_CLASS_SCIENTIFIC_NAME)
    if (birdClassId.value === null) {
      speciesError.value = `分类树里没有找到「${BIRD_CLASS_SCIENTIFIC_NAME}」（鸟纲）节点`
    }
  } catch (e) {
    speciesError.value = errorMessage(e)
  }
  await Promise.all([
    loadSpeciesRecords(),
    loadRecords(),
    loadBirdOptions(),
    loadListNames(),
    loadSurveys(),
  ])
})

// 地址栏变了但组件没重建（点顶栏的「鸟类调查」、浏览器前进后退）时跟着变。
watch(
  () => route.query,
  () => void syncFromRoute(),
)
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <div>
          <h2>鸟类调查</h2>
          <p class="hint">
            燕园鸟类调查的资料。鸟种清单是鸟纲下的名录记录，重要记录来自《北京大学燕园校区鸟类记录整理.docx》，鸟调记录来自各次样线调查的
            GPX。
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
          @click="selectTab(item.key)"
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
        <form class="search-bar" @submit.prevent="searchSurveys">
          <input v-model="surveysKeyword" placeholder="按鸟种搜索…" />
          <input v-model="surveysFrom" class="date" type="date" aria-label="起始日期" />
          <span class="muted">至</span>
          <input v-model="surveysTo" class="date" type="date" aria-label="结束日期" />
          <button type="submit">搜索</button>
          <button
            v-if="surveysFiltered"
            class="secondary"
            type="button"
            @click="clearSurveysSearch"
          >
            清除
          </button>
          <button v-if="canManage" type="button" @click="openImport">导入 GPX</button>
          <button v-if="canManage" class="secondary" type="button" @click="openCreateSurvey">
            新增鸟调
          </button>
        </form>

        <p class="hint">
          每行是一次鸟调，按日期从新到旧排列；「鸟种数」按明细里的不同鸟种现算。
          搜索按明细里的鸟种名匹配，点日期可以看详细的鸟种记录。
        </p>

        <p v-if="surveysError" class="alert error">{{ surveysError }}</p>
        <p v-if="surveysLoading" class="muted">加载中…</p>

        <div v-else class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                <th>日期</th>
                <th>领队</th>
                <th>天气</th>
                <th>鸟种数</th>
                <th>备注</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="survey in surveys" :key="survey.id">
                <td class="nowrap">
                  <button class="name-link" type="button" @click="openDetail(survey)">
                    {{ survey.survey_date }}
                  </button>
                </td>
                <td class="muted nowrap">{{ survey.leader ?? '—' }}</td>
                <td class="muted nowrap">{{ survey.weather ?? '—' }}</td>
                <td class="muted mono">{{ survey.species_count }}</td>
                <td class="muted">
                  <NoteText :note="survey.note" clip />
                </td>
                <td class="actions-cell">
                  <button class="secondary" type="button" @click="openDetail(survey)">详情</button>
                  <template v-if="canManage">
                    <button class="secondary" type="button" @click="openEditSurvey(survey)">
                      编辑
                    </button>
                    <button class="danger" type="button" @click="openDeleteSurvey(survey)">
                      删除
                    </button>
                  </template>
                </td>
              </tr>
              <tr v-if="surveys.length === 0">
                <td :colspan="6" class="muted">
                  <span v-if="surveysFiltered">没有符合当前条件的鸟调记录</span>
                  <span v-else>还没有鸟调记录</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <ListPager
          :total="surveysTotal"
          :page-size="PAGE_SIZE"
          :page="surveysOffset / PAGE_SIZE + 1"
          :disabled="surveysLoading"
          @change="goToSurveysPage"
        />
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

  <!-- 鸟种清单里「编辑」就地用的名录记录弹窗（与「物种名录」页共用） -->
  <SpeciesRecordDialog ref="speciesRecordDialog" @saved="onSpeciesRecordSaved" />

  <!-- 导入鸟调 GPX -->
  <dialog ref="importDialog" class="modal modal-wide" @click.self="closeImport">
    <form @submit.prevent="submitImport">
      <h2>导入鸟调 GPX</h2>
      <p v-if="importForm.error" class="alert error">{{ importForm.error }}</p>
      <p v-if="importForm.result" class="alert success">{{ importForm.result }}</p>

      <div class="field">
        <label for="import-file">GPX 文件</label>
        <input
          id="import-file"
          type="file"
          accept=".gpx,application/gpx+xml"
          @change="onImportFileChange"
        />
        <span class="hint">文件名形如 20180522北大鸟调多云转晴.gpx 时会自动填好日期和天气</span>
      </div>

      <div class="field">
        <label for="import-leader">领队</label>
        <input
          id="import-leader"
          v-model="importForm.leader"
          :maxlength="SURVEY_LEADER_MAX_CHARS"
        />
      </div>

      <div class="field">
        <label for="import-date">日期</label>
        <input id="import-date" v-model="importForm.surveyDate" class="date" type="date" />
        <span class="hint">留空时用 GPX 里最早的观察时间</span>
      </div>

      <div class="field">
        <label for="import-weather">天气</label>
        <input
          id="import-weather"
          v-model="importForm.weather"
          :maxlength="SURVEY_WEATHER_MAX_CHARS"
        />
      </div>

      <div class="field">
        <label for="import-note">备注</label>
        <textarea id="import-note" v-model="importForm.note" rows="2"></textarea>
      </div>

      <div class="actions">
        <button type="submit" :disabled="importForm.importing">
          {{ importForm.importing ? '导入中…' : '导入' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="importForm.importing"
          @click="closeImport"
        >
          {{ importForm.result === '' ? '取消' : '关闭' }}
        </button>
      </div>
    </form>
  </dialog>

  <!-- 鸟调明细：表头 + 逐条鸟种 -->
  <dialog ref="detailDialog" class="modal modal-wide modal-detail" @click.self="closeDetail">
    <template v-if="detail !== null">
      <div class="log-head">
        <h2>{{ detail.survey_date }} 鸟调记录</h2>
        <div v-if="canManage" class="head-actions">
          <button class="secondary" type="button" @click="openEditSurvey(detail)">修改</button>
        </div>
      </div>

      <div class="info-list">
        <div class="info-row">
          <span class="info-label">领队</span>
          <span>{{ detail.leader ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">天气</span>
          <span>{{ detail.weather ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">鸟种数</span>
          <span>{{ detail.species_count }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">来源</span>
          <span>{{ detail.source ?? '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-label">备注</span>
          <span>{{ detail.note ?? '—' }}</span>
        </div>
      </div>

      <div class="head-actions">
        <span class="muted">共 {{ detail.records.length }} 条观察</span>
        <button v-if="canManage" type="button" @click="openCreateSurveyRecord">添加鸟种</button>
      </div>

      <p v-if="detailError" class="alert error">{{ detailError }}</p>

      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>鸟种</th>
              <th>学名</th>
              <th>数量</th>
              <th></th>
              <th v-if="canManage"></th>
            </tr>
          </thead>
          <tbody>
            <!-- 「详情」按一下才展开时间 / 位置 / 备注 -->
            <template v-for="record in detail.records" :key="record.id">
              <tr class="record-row">
                <td class="nowrap">
                  {{ record.chinese_name }}
                  <span
                    v-if="record.taxon_id === null"
                    class="tag"
                    title="名录里没有这个名字，只保留原名"
                    >未匹配</span
                  >
                </td>
                <td class="muted nowrap">{{ record.scientific_name ?? '—' }}</td>
                <td class="muted mono">{{ record.quantity ?? '—' }}</td>
                <td class="nowrap">
                  <button
                    class="tag tag-button"
                    type="button"
                    :aria-expanded="expandedRecordIds.has(record.id)"
                    @click="toggleRecordDetails(record.id)"
                  >
                    {{ expandedRecordIds.has(record.id) ? '收起' : '详情' }}
                  </button>
                </td>
                <td v-if="canManage" class="actions-cell">
                  <button class="secondary" type="button" @click="openEditSurveyRecord(record)">
                    编辑
                  </button>
                  <button class="danger" type="button" @click="openDeleteSurveyRecord(record)">
                    删除
                  </button>
                </td>
              </tr>
              <tr v-if="expandedRecordIds.has(record.id)" class="record-meta-row">
                <td :colspan="canManage ? 5 : 4">
                  <div class="record-meta">
                    <span>
                      <span class="muted">时间</span>
                      {{ record.observed_at === null ? '—' : formatTimestamp(record.observed_at) }}
                    </span>
                    <span>
                      <span class="muted">位置</span>
                      {{ formatCoordinates(record) }}
                      <a
                        v-if="amapUrl(record) !== null"
                        class="map-link"
                        :href="amapUrl(record) ?? undefined"
                        target="_blank"
                        rel="noopener noreferrer"
                        >地图</a
                      >
                    </span>
                    <span
                      v-if="record.note !== null && record.note.trim() !== ''"
                      class="record-meta-note note-full"
                    >
                      <span class="muted">备注</span>
                      <NoteText :note="record.note" />
                    </span>
                  </div>
                </td>
              </tr>
            </template>
            <tr v-if="detail.records.length === 0">
              <td :colspan="canManage ? 5 : 4" class="muted">这次鸟调还没有鸟种记录</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
    <p v-else-if="detailLoading" class="muted">加载中…</p>
    <p v-else-if="detailError" class="alert error">{{ detailError }}</p>

    <div class="actions">
      <button class="secondary" type="button" @click="closeDetail">关闭</button>
    </div>
  </dialog>

  <!-- 新增 / 编辑鸟调表头 -->
  <dialog ref="surveyDialog" class="modal" @click.self="closeSurveyDialog">
    <form @submit.prevent="submitSurvey">
      <h2>{{ surveyForm.id === null ? '新增鸟调' : '编辑鸟调' }}</h2>
      <p v-if="surveyForm.error" class="alert error">{{ surveyForm.error }}</p>

      <div class="field">
        <label for="survey-date">日期</label>
        <input id="survey-date" v-model="surveyForm.surveyDate" class="date" type="date" />
      </div>
      <div class="field">
        <label for="survey-leader">领队</label>
        <input
          id="survey-leader"
          v-model="surveyForm.leader"
          :maxlength="SURVEY_LEADER_MAX_CHARS"
        />
      </div>
      <div class="field">
        <label for="survey-weather">天气</label>
        <input
          id="survey-weather"
          v-model="surveyForm.weather"
          :maxlength="SURVEY_WEATHER_MAX_CHARS"
        />
      </div>
      <div class="field">
        <label for="survey-note">备注</label>
        <textarea id="survey-note" v-model="surveyForm.note" rows="2"></textarea>
      </div>

      <div class="actions">
        <button type="submit" :disabled="surveyForm.submitting">
          {{ surveyForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="surveyForm.submitting"
          @click="closeSurveyDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <!-- 新增 / 编辑鸟种明细 -->
  <dialog ref="surveyRecordDialog" class="modal modal-wide" @click.self="closeSurveyRecordDialog">
    <form @submit.prevent="submitSurveyRecord">
      <h2>{{ surveyRecordForm.id === null ? '添加鸟种' : '编辑鸟种' }}</h2>
      <p v-if="surveyRecordForm.error" class="alert error">{{ surveyRecordForm.error }}</p>
      <p v-if="birdOptionsError" class="alert error">{{ birdOptionsError }}</p>

      <div class="field">
        <label for="survey-record-bird-filter">鸟种</label>
        <input
          id="survey-record-bird-filter"
          v-model="surveyRecordBirdKeyword"
          placeholder="先输入中文名或学名筛选…"
        />
        <select
          id="survey-record-bird"
          v-model="surveyRecordForm.taxonId"
          @change="pickSurveyRecordBird"
        >
          <option :value="MANUAL_TAXON">— 未匹配（自己填名称）—</option>
          <option
            v-for="bird in filteredSurveyRecordBirds"
            :key="bird.taxon_id"
            :value="bird.taxon_id"
          >
            {{ taxonLabel(bird) }}
          </option>
        </select>
        <span class="hint"
          >选择鸟种后学名 / 中文名自动带出；名录里没有的名字选「未匹配」并手填</span
        >
      </div>

      <template v-if="surveyRecordForm.taxonId === MANUAL_TAXON">
        <div class="field">
          <label for="survey-record-name">中文名</label>
          <input
            id="survey-record-name"
            v-model="surveyRecordForm.chineseName"
            :maxlength="SURVEY_RECORD_NAME_MAX_CHARS"
          />
        </div>
        <div class="field">
          <label for="survey-record-sci">学名</label>
          <input
            id="survey-record-sci"
            v-model="surveyRecordForm.scientificName"
            :maxlength="SURVEY_RECORD_NAME_MAX_CHARS"
          />
          <span class="hint">未匹配时学名可以留空</span>
        </div>
      </template>

      <div class="field">
        <label for="survey-record-quantity">数量</label>
        <input
          id="survey-record-quantity"
          v-model="surveyRecordForm.quantity"
          type="number"
          min="0"
          step="1"
        />
      </div>
      <div class="field">
        <label for="survey-record-time">时间</label>
        <input
          id="survey-record-time"
          v-model="surveyRecordForm.observedAt"
          type="datetime-local"
        />
      </div>
      <div class="field">
        <label for="survey-record-latitude">纬度</label>
        <input
          id="survey-record-latitude"
          v-model="surveyRecordForm.latitude"
          type="number"
          step="any"
        />
      </div>
      <div class="field">
        <label for="survey-record-longitude">经度</label>
        <input
          id="survey-record-longitude"
          v-model="surveyRecordForm.longitude"
          type="number"
          step="any"
        />
      </div>
      <div class="field">
        <label for="survey-record-note">备注</label>
        <textarea id="survey-record-note" v-model="surveyRecordForm.note" rows="2"></textarea>
      </div>

      <div class="actions">
        <button type="submit" :disabled="surveyRecordForm.submitting">
          {{ surveyRecordForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="surveyRecordForm.submitting"
          @click="closeSurveyRecordDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <ConfirmDialog
    ref="surveyDeleteDialog"
    title="删除鸟调记录"
    :message="
      pendingSurvey === null
        ? ''
        : `确定删除 ${pendingSurvey.survey_date} 的鸟调记录？该次调查的全部鸟种明细会一起删掉，此操作不可撤销。`
    "
    :busy="surveyDeleting"
    :error="surveyDeleteError"
    @confirm="confirmDeleteSurvey"
    @cancel="surveyDeleteError = ''"
  />

  <ConfirmDialog
    ref="surveyRecordDeleteDialog"
    title="删除鸟种明细"
    :message="
      pendingSurveyRecord === null
        ? ''
        : `确定删除「${pendingSurveyRecord.chinese_name}」这条记录？此操作不可撤销。`
    "
    :busy="surveyRecordDeleting"
    :error="surveyRecordDeleteError"
    @confirm="confirmDeleteSurveyRecord"
    @cancel="surveyRecordDeleteError = ''"
  />
</template>
