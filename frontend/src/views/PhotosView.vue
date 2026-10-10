<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { errorMessage } from '../api/http'
import { createPhoto, deletePhoto, listPhotos, updatePhoto } from '../api/photos'
import { getTaxon } from '../api/taxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import ListPager from '../components/ListPager.vue'
import NoteText from '../components/NoteText.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import { clampOffset } from '../pagination'
import {
  MAX_PHOTOS_PER_TAXON,
  RATING_OPTIONS,
  formatFileSize,
  validatePhotoFile,
  type SpeciesPhoto,
} from '../photos'
import { useAuthStore } from '../stores/auth'
import { taxonLabel, type Taxon } from '../taxonomy'
import { formatTimestamp } from '../types'

/// 物种照片页。从物种卡片的「上传照片 / 浏览照片」进来，
/// URL 上带 `?taxon=<id>`；不带 taxon 时浏览全部照片并按关键字搜索。
///
/// 上传走 multipart：拍摄时间手填（默认今天），后端按它的年月建子目录。
/// 单物种最多 9 张、单张不超过 2M，服务端是最终把关，这里只是即时提示。
const PAGE_SIZE = 12

const auth = useAuthStore()
const route = useRoute()
const canManage = computed(() => auth.canManageTaxonomy)

/// 从物种卡片进来时锁定这个物种；否则可以在上传框里自己选。
const lockedTaxonId = computed(() =>
  typeof route.query.taxon === 'string' && route.query.taxon !== '' ? route.query.taxon : '',
)
const taxon = ref<Taxon | null>(null)
const title = computed(() => (taxon.value === null ? '物种照片' : taxonLabel(taxon.value)))

/// 空串转 null（后端的可空字段用 null 表示清空）。
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

// ==========================================================================
// 列表
// ==========================================================================

const photos = ref<SpeciesPhoto[]>([])
const loading = ref(false)
const error = ref('')
const keyword = ref('')
const appliedKeyword = ref('')
const offset = ref(0)
const total = ref(0)
const savedMessage = ref('')

/// 当前物种已经有多少张照片（没锁定物种 / 有关键字时不做这个判断，
/// 上限最终由后端在同一事务里把关）。
const reachedLimit = computed(
  () =>
    lockedTaxonId.value !== '' &&
    appliedKeyword.value === '' &&
    total.value >= MAX_PHOTOS_PER_TAXON,
)

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const page = await listPhotos({
      taxonId: lockedTaxonId.value === '' ? undefined : lockedTaxonId.value,
      q: appliedKeyword.value === '' ? undefined : appliedKeyword.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    })
    // 删照片 / 改筛选条件后当前页可能已经越过末页，退回去重拉一次。
    const clamped = clampOffset(offset.value, page.total, PAGE_SIZE)
    if (clamped !== offset.value) {
      offset.value = clamped
      await load()
      return
    }
    photos.value = page.items
    total.value = page.total
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

async function loadTaxon(): Promise<void> {
  if (lockedTaxonId.value === '') {
    taxon.value = null
    return
  }
  try {
    taxon.value = await getTaxon(lockedTaxonId.value)
  } catch {
    // 物种查不到不影响看照片，标题退回通用的即可。
    taxon.value = null
  }
}

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

async function goToPage(page: number): Promise<void> {
  offset.value = (page - 1) * PAGE_SIZE
  await load()
}

// ==========================================================================
// 上传
// ==========================================================================

const createDialog = ref<HTMLDialogElement | null>(null)
const createForm = reactive({
  taxonId: '',
  takenAt: today(),
  photographer: '',
  location: '',
  note: '',
  /// '' = 未评分
  rating: '',
  isImportant: false,
  file: null as File | null,
  submitting: false,
  error: '',
})

function resetCreateForm(): void {
  createForm.taxonId = lockedTaxonId.value
  createForm.takenAt = today()
  createForm.photographer = ''
  createForm.location = ''
  createForm.note = ''
  createForm.rating = ''
  createForm.isImportant = false
  createForm.file = null
  createForm.error = ''
}

function openCreate(): void {
  savedMessage.value = ''
  resetCreateForm()
  createDialog.value?.showModal()
}

function closeCreate(): void {
  createDialog.value?.close()
}

function pickFile(event: Event): void {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0] ?? null
  createForm.file = file
  if (file !== null) {
    createForm.error = validatePhotoFile(file)
  }
}

async function submitCreate(): Promise<void> {
  createForm.error = ''
  if (createForm.taxonId === '') {
    createForm.error = '请选择物种'
    return
  }
  if (createForm.takenAt === '') {
    createForm.error = '请选择拍摄时间'
    return
  }
  const file = createForm.file
  if (file === null) {
    createForm.error = '请选择照片文件'
    return
  }
  const fileProblem = validatePhotoFile(file)
  if (fileProblem !== '') {
    createForm.error = fileProblem
    return
  }

  createForm.submitting = true
  try {
    await createPhoto({
      file,
      taxonId: createForm.taxonId,
      takenAt: createForm.takenAt,
      photographer: optional(createForm.photographer),
      location: optional(createForm.location),
      note: optional(createForm.note),
      rating: createForm.rating === '' ? null : Number(createForm.rating),
      isImportant: createForm.isImportant,
    })
    closeCreate()
    savedMessage.value = '照片已上传'
    offset.value = 0
    await load()
  } catch (e) {
    createForm.error = errorMessage(e)
  } finally {
    createForm.submitting = false
  }
}

// ==========================================================================
// 编辑（只改元数据，拍摄时间 / 文件不可改）
// ==========================================================================

const editDialog = ref<HTMLDialogElement | null>(null)
const editForm = reactive({
  id: '',
  takenAt: '',
  photographer: '',
  location: '',
  note: '',
  rating: '',
  isImportant: false,
  submitting: false,
  error: '',
})

function openEdit(photo: SpeciesPhoto): void {
  savedMessage.value = ''
  editForm.id = photo.id
  editForm.takenAt = photo.taken_at
  editForm.photographer = photo.photographer ?? ''
  editForm.location = photo.location ?? ''
  editForm.note = photo.note ?? ''
  editForm.rating = photo.rating === null ? '' : String(photo.rating)
  editForm.isImportant = photo.is_important
  editForm.error = ''
  editDialog.value?.showModal()
}

function closeEdit(): void {
  editDialog.value?.close()
}

async function submitEdit(): Promise<void> {
  editForm.error = ''
  editForm.submitting = true
  try {
    await updatePhoto(editForm.id, {
      photographer: optional(editForm.photographer),
      location: optional(editForm.location),
      note: optional(editForm.note),
      rating: editForm.rating === '' ? null : Number(editForm.rating),
      is_important: editForm.isImportant,
    })
    closeEdit()
    savedMessage.value = '照片信息已更新'
    await load()
  } catch (e) {
    editForm.error = errorMessage(e)
  } finally {
    editForm.submitting = false
  }
}

// ==========================================================================
// 删除
// ==========================================================================

const deleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const deleteError = ref('')
const deleting = ref(false)
const pendingPhoto = ref<SpeciesPhoto | null>(null)

function openDelete(photo: SpeciesPhoto): void {
  savedMessage.value = ''
  deleteError.value = ''
  pendingPhoto.value = photo
  deleteDialog.value?.open()
}

async function confirmDelete(): Promise<void> {
  const target = pendingPhoto.value
  if (target === null) {
    return
  }
  deleting.value = true
  deleteError.value = ''
  try {
    await deletePhoto(target.id)
    deleteDialog.value?.close()
    savedMessage.value = '照片已删除'
    await load()
  } catch (e) {
    deleteError.value = errorMessage(e)
  } finally {
    deleting.value = false
  }
}

// ==========================================================================
// 生命周期
// ==========================================================================

/// 物种选择框的 v-model 桥接：TaxonPicker 用 `string | null`，表单里用 `''` 表示未选。
function applyPickedTaxon(value: string | null): void {
  createForm.taxonId = value ?? ''
}

onMounted(async () => {
  await Promise.all([loadTaxon(), load()])
  if (route.query.upload === '1' && canManage.value) {
    openCreate()
  }
})

// 从卡片点进另一个物种时（路由 query 变化）重新加载。
watch(lockedTaxonId, async () => {
  offset.value = 0
  keyword.value = ''
  appliedKeyword.value = ''
  await Promise.all([loadTaxon(), load()])
})
</script>

<template>
  <div class="stack">
    <section class="card">
      <div class="card-head">
        <div>
          <h2>{{ title }}</h2>
          <p class="hint">
            拍摄时间按年月分目录存放；每个物种最多 {{ MAX_PHOTOS_PER_TAXON }} 张，单张不超过 2M。
          </p>
        </div>
        <div class="head-actions">
          <button v-if="canManage" type="button" :disabled="reachedLimit" @click="openCreate">
            上传照片
          </button>
        </div>
      </div>

      <p v-if="reachedLimit" class="alert info">
        该物种已经有 {{ total }} 张照片，达到
        {{ MAX_PHOTOS_PER_TAXON }} 张上限。删除旧照片后才能继续上传。
      </p>

      <form class="search-bar" @submit.prevent="search">
        <input v-model="keyword" placeholder="按拍摄人 / 地点 / 备注搜索…" />
        <button type="submit">搜索</button>
        <button v-if="appliedKeyword !== ''" class="secondary" type="button" @click="clearSearch">
          清除
        </button>
      </form>

      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
      <p v-if="error" class="alert error">{{ error }}</p>
      <p v-if="loading" class="muted">加载中…</p>

      <template v-else-if="photos.length > 0">
        <div class="photo-grid">
          <figure v-for="photo in photos" :key="photo.id" class="photo-card">
            <a :href="photo.url" target="_blank" rel="noopener">
              <img :src="photo.url" :alt="photo.note ?? '物种照片'" loading="lazy" />
            </a>
            <figcaption class="photo-meta">
              <div class="photo-tags">
                <span v-if="photo.is_important" class="tag">重要记录</span>
                <span v-if="photo.rating !== null" class="tag">评分 {{ photo.rating }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">拍摄</span>
                <span>{{ photo.taken_at }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">拍摄人</span>
                <span>{{ photo.photographer ?? '—' }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">上传者</span>
                <span>{{ photo.uploader_username }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">地点</span>
                <span>{{ photo.location ?? '—' }}</span>
              </div>
              <div class="info-row">
                <span class="info-label">备注</span>
                <NoteText :note="photo.note" />
              </div>
              <div class="info-row">
                <span class="info-label">文件</span>
                <span class="muted">
                  {{ formatFileSize(photo.file_size) }}
                  <template v-if="photo.original_filename"
                    >· {{ photo.original_filename }}</template
                  >
                </span>
              </div>
              <div class="info-row">
                <span class="info-label">上传</span>
                <span class="muted">{{ formatTimestamp(photo.created_at) }}</span>
              </div>
              <div v-if="canManage" class="actions">
                <button class="secondary" type="button" @click="openEdit(photo)">编辑</button>
                <button class="danger" type="button" @click="openDelete(photo)">删除</button>
              </div>
            </figcaption>
          </figure>
        </div>

        <ListPager
          :total="total"
          :page-size="PAGE_SIZE"
          :page="offset / PAGE_SIZE + 1"
          :disabled="loading"
          @change="goToPage"
        />
      </template>

      <p v-else class="muted">
        <span v-if="appliedKeyword !== ''">没有匹配「{{ appliedKeyword }}」的照片</span>
        <span v-else-if="lockedTaxonId !== ''">这个物种还没有照片</span>
        <span v-else>还没有任何照片</span>
      </p>
    </section>
  </div>

  <!-- 上传 -->
  <dialog
    ref="createDialog"
    class="modal modal-wide"
    @close="resetCreateForm"
    @click.self="closeCreate"
  >
    <form @submit.prevent="submitCreate">
      <h2>上传照片</h2>
      <p v-if="createForm.error" class="alert error">{{ createForm.error }}</p>

      <div v-if="lockedTaxonId !== ''" class="field">
        <label>物种</label>
        <p>{{ title }}</p>
      </div>
      <div v-else class="field">
        <label>物种</label>
        <TaxonPicker
          :model-value="createForm.taxonId === '' ? null : createForm.taxonId"
          @update:model-value="applyPickedTaxon"
        />
      </div>

      <div class="field">
        <label for="photo-file">照片（≤2M，jpg / png / gif / webp）</label>
        <input
          id="photo-file"
          type="file"
          accept="image/jpeg,image/png,image/gif,image/webp"
          @change="pickFile"
        />
        <span v-if="createForm.file !== null" class="hint">
          已选 {{ createForm.file.name }}（{{ formatFileSize(createForm.file.size) }}）
        </span>
      </div>

      <div class="field">
        <label for="photo-taken-at">拍摄时间</label>
        <input id="photo-taken-at" v-model="createForm.takenAt" type="date" required />
        <span class="hint">按它的年月（yyyy-mm）建立子目录保存</span>
      </div>

      <div class="field">
        <label for="photo-photographer">拍摄人</label>
        <input id="photo-photographer" v-model="createForm.photographer" />
      </div>

      <div class="field">
        <label for="photo-location">地点</label>
        <input id="photo-location" v-model="createForm.location" />
      </div>

      <div class="field">
        <label for="photo-note">备注</label>
        <textarea id="photo-note" v-model="createForm.note" rows="3"></textarea>
      </div>

      <div class="field">
        <label for="photo-rating">评分（1-10）</label>
        <select id="photo-rating" v-model="createForm.rating">
          <option value="">未评分</option>
          <option v-for="score in RATING_OPTIONS" :key="score" :value="String(score)">
            {{ score }}
          </option>
        </select>
      </div>

      <div class="field checkbox">
        <label>
          <input v-model="createForm.isImportant" type="checkbox" />
          重要记录
        </label>
      </div>

      <div class="actions">
        <button type="submit" :disabled="createForm.submitting">
          {{ createForm.submitting ? '上传中…' : '上传' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="createForm.submitting"
          @click="closeCreate"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <!-- 编辑元数据 -->
  <dialog ref="editDialog" class="modal" @click.self="closeEdit">
    <form @submit.prevent="submitEdit">
      <h2>编辑照片信息</h2>
      <p v-if="editForm.error" class="alert error">{{ editForm.error }}</p>

      <div class="field">
        <label>拍摄时间</label>
        <p>{{ editForm.takenAt }}</p>
        <span class="hint">拍摄时间决定存储目录，不可修改</span>
      </div>

      <div class="field">
        <label for="edit-photographer">拍摄人</label>
        <input id="edit-photographer" v-model="editForm.photographer" />
      </div>

      <div class="field">
        <label for="edit-location">地点</label>
        <input id="edit-location" v-model="editForm.location" />
      </div>

      <div class="field">
        <label for="edit-note">备注</label>
        <textarea id="edit-note" v-model="editForm.note" rows="3"></textarea>
      </div>

      <div class="field">
        <label for="edit-rating">评分（1-10）</label>
        <select id="edit-rating" v-model="editForm.rating">
          <option value="">未评分</option>
          <option v-for="score in RATING_OPTIONS" :key="score" :value="String(score)">
            {{ score }}
          </option>
        </select>
      </div>

      <div class="field checkbox">
        <label>
          <input v-model="editForm.isImportant" type="checkbox" />
          重要记录
        </label>
      </div>

      <div class="actions">
        <button type="submit" :disabled="editForm.submitting">
          {{ editForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button class="secondary" type="button" :disabled="editForm.submitting" @click="closeEdit">
          取消
        </button>
      </div>
    </form>
  </dialog>

  <ConfirmDialog
    ref="deleteDialog"
    title="删除照片"
    message="确定删除这张照片？磁盘上的文件也会一并删除，此操作不可撤销。"
    :busy="deleting"
    :error="deleteError"
    @confirm="confirmDelete"
    @cancel="deleteError = ''"
  />
</template>
