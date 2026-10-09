<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { errorMessage } from '../api/http'
import { getTaxon, getTree } from '../api/taxonomy'
import { createTaxon, deleteTaxon, updateTaxon } from '../api/staffTaxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import TaxonTreeNode from '../components/TaxonTreeNode.vue'
import { useAuthStore } from '../stores/auth'
import {
  RANKS,
  isStrictlyHigher,
  rankLabel,
  taxonLabel,
  type Taxon,
  type TaxonDetail,
  type TaxonNode,
  type TaxonomyRank,
} from '../taxonomy'
import { TAXON_NAME_MAX_CHARS, charCount, validateField } from '../validation'

/// 树节点和详情都能提供这几个字段，选中状态用这个最小结构即可。
type TaxonRef = Pick<Taxon, 'id' | 'parent_id' | 'rank' | 'scientific_name' | 'chinese_name'>

const auth = useAuthStore()
const canManage = computed(() => auth.canManageTaxonomy)

const tree = ref<TaxonNode[]>([])
const treeLoading = ref(false)
const treeError = ref('')

const selected = ref<TaxonRef | null>(null)
const detail = ref<TaxonDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref('')

const savedMessage = ref('')

async function loadTree(): Promise<void> {
  treeLoading.value = true
  treeError.value = ''
  try {
    tree.value = await getTree()
  } catch (e) {
    treeError.value = errorMessage(e)
  } finally {
    treeLoading.value = false
  }
}

onMounted(() => loadTree())

async function selectNode(node: TaxonRef): Promise<void> {
  selected.value = node
  savedMessage.value = ''
  detailError.value = ''
  detailLoading.value = true
  try {
    detail.value = await getTaxon(node.id)
  } catch (e) {
    detail.value = null
    detailError.value = errorMessage(e)
  } finally {
    detailLoading.value = false
  }
}

// --- 新增节点 ---

const createDialog = ref<HTMLDialogElement | null>(null)
const createForm = reactive({
  parentId: null as string | null,
  parentRank: null as TaxonomyRank | null,
  parentLabel: '（根节点）',
  rank: 'kingdom' as TaxonomyRank,
  scientificName: '',
  chineseName: '',
  submitting: false,
  error: '',
})

/// 子节点阶元必须严格低于父节点；根节点的阶元随便选。允许跳级。
const createRanks = computed(() => {
  const parentRank = createForm.parentRank
  return parentRank === null ? RANKS : RANKS.filter((rank) => isStrictlyHigher(parentRank, rank))
})

function openCreate(parent: TaxonRef | null): void {
  savedMessage.value = ''
  createForm.parentId = parent === null ? null : parent.id
  createForm.parentRank = parent === null ? null : parent.rank
  createForm.parentLabel = parent === null ? '（根节点）' : taxonLabel(parent)
  createForm.rank = createRanks.value[0] ?? 'species'
  createForm.scientificName = ''
  createForm.chineseName = ''
  createForm.error = ''
  if (createDialog.value !== null && !createDialog.value.open) {
    createDialog.value.showModal()
  }
}

function closeCreateDialog(): void {
  createDialog.value?.close()
}

async function submitCreate(): Promise<void> {
  createForm.error = ''
  const problem = validateField('学名', createForm.scientificName, TAXON_NAME_MAX_CHARS)
  if (problem !== '') {
    createForm.error = problem
    return
  }
  const chineseName = createForm.chineseName.trim()
  if (chineseName !== '' && charCount(chineseName) > TAXON_NAME_MAX_CHARS) {
    createForm.error = `中文名最多 ${TAXON_NAME_MAX_CHARS} 个字符`
    return
  }

  createForm.submitting = true
  try {
    const created = await createTaxon({
      parent_id: createForm.parentId,
      rank: createForm.rank,
      scientific_name: createForm.scientificName.trim(),
      chinese_name: chineseName === '' ? null : chineseName,
    })
    closeCreateDialog()
    savedMessage.value = `已新增节点「${taxonLabel(created)}」`
    await loadTree()
    await selectNode(created)
  } catch (e) {
    createForm.error = errorMessage(e)
  } finally {
    createForm.submitting = false
  }
}

// --- 编辑节点 ---

const editDialog = ref<HTMLDialogElement | null>(null)
/// 每次打开自增，作为 TaxonPicker 的 key，强制重建以带上当前父级。
const editFormKey = ref(0)
const editForm = reactive({
  id: '',
  rank: 'species' as TaxonomyRank,
  scientificName: '',
  chineseName: '',
  parentId: null as string | null,
  submitting: false,
  error: '',
})

function openEdit(node: TaxonRef): void {
  savedMessage.value = ''
  editForm.id = node.id
  editForm.rank = node.rank
  editForm.scientificName = node.scientific_name
  editForm.chineseName = node.chinese_name ?? ''
  editForm.parentId = node.parent_id ?? null
  editForm.error = ''
  editFormKey.value += 1
  if (editDialog.value !== null && !editDialog.value.open) {
    editDialog.value.showModal()
  }
}

function closeEditDialog(): void {
  editDialog.value?.close()
}

async function submitEdit(): Promise<void> {
  editForm.error = ''
  const problem = validateField('学名', editForm.scientificName, TAXON_NAME_MAX_CHARS)
  if (problem !== '') {
    editForm.error = problem
    return
  }
  const chineseName = editForm.chineseName.trim()
  if (chineseName !== '' && charCount(chineseName) > TAXON_NAME_MAX_CHARS) {
    editForm.error = `中文名最多 ${TAXON_NAME_MAX_CHARS} 个字符`
    return
  }

  editForm.submitting = true
  try {
    // parent_id 传 null 表示移到根；中文名清空同理（三态字段，见 §2.2）。
    const updated = await updateTaxon(editForm.id, {
      scientific_name: editForm.scientificName.trim(),
      chinese_name: chineseName === '' ? null : chineseName,
      parent_id: editForm.parentId,
    })
    closeEditDialog()
    savedMessage.value = `已更新节点「${taxonLabel(updated)}」`
    await loadTree()
    await selectNode(updated)
  } catch (e) {
    editForm.error = errorMessage(e)
  } finally {
    editForm.submitting = false
  }
}

// --- 删除节点 ---

const deleteDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)
const deleteError = ref('')
const deleting = ref(false)
const pendingDelete = ref<TaxonRef | null>(null)

function openDelete(node: TaxonRef): void {
  savedMessage.value = ''
  deleteError.value = ''
  pendingDelete.value = node
  deleteDialog.value?.open()
}

async function confirmDelete(): Promise<void> {
  const target = pendingDelete.value
  if (target === null) {
    return
  }
  deleting.value = true
  deleteError.value = ''
  try {
    await deleteTaxon(target.id)
    deleteDialog.value?.close()
    savedMessage.value = `已删除节点「${taxonLabel(target)}」`
    if (selected.value?.id === target.id) {
      selected.value = null
      detail.value = null
    }
    await loadTree()
  } catch (e) {
    // 有子节点或记录时后端返回 409，错误信息留在对话框里。
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
        <h2>分类树</h2>
        <div class="head-actions">
          <span class="muted">共 {{ tree.length }} 个根节点</span>
          <button v-if="canManage" type="button" @click="openCreate(null)">新增根节点</button>
          <button class="secondary" type="button" :disabled="treeLoading" @click="loadTree">
            刷新
          </button>
        </div>
      </div>

      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
      <p v-if="treeError" class="alert error">{{ treeError }}</p>
      <p v-if="treeLoading" class="muted">加载中…</p>

      <div v-else class="tree-split">
        <ul class="tree">
          <TaxonTreeNode
            v-for="node in tree"
            :key="node.id"
            :node="node"
            :selected-id="selected === null ? null : selected.id"
            @select="selectNode"
          />
          <li v-if="tree.length === 0" class="muted">分类树还是空的</li>
        </ul>

        <div class="tree-detail">
          <p v-if="detailError" class="alert error">{{ detailError }}</p>

          <template v-if="detail">
            <p class="crumbs">
              <template v-for="ancestor in detail.path" :key="ancestor.id">
                <button class="crumb" type="button" @click="selectNode(ancestor)">
                  {{ taxonLabel(ancestor) }}
                </button>
                <span class="muted">›</span>
              </template>
              <strong>{{ taxonLabel(detail) }}</strong>
            </p>

            <div class="field">
              <label>阶元</label>
              <div>
                <span class="tag">{{ rankLabel(detail.rank) }}</span>
              </div>
            </div>
            <div class="field">
              <label>学名</label>
              <div>{{ detail.scientific_name }}</div>
            </div>
            <div class="field">
              <label>中文名</label>
              <div>{{ detail.chinese_name ?? '—' }}</div>
            </div>
            <div class="field">
              <label>直接子节点</label>
              <div v-if="detail.children.length === 0" class="muted">（无）</div>
              <div v-else class="chip-list">
                <button
                  v-for="child in detail.children"
                  :key="child.id"
                  class="chip"
                  type="button"
                  @click="selectNode(child)"
                >
                  <span class="tag">{{ rankLabel(child.rank) }}</span>
                  {{ taxonLabel(child) }}
                </button>
              </div>
            </div>
            <p class="hint">
              新增子节点时阶元必须严格低于「{{ rankLabel(detail.rank) }}」，允许跳级。
            </p>

            <div v-if="canManage" class="actions">
              <button type="button" @click="openCreate(detail)">新增子节点</button>
              <button class="secondary" type="button" @click="openEdit(detail)">编辑</button>
              <button class="danger" type="button" @click="openDelete(detail)">删除</button>
            </div>
          </template>

          <p v-else-if="detailLoading" class="muted">加载中…</p>
          <p v-else class="muted">点左边的节点查看详情。</p>
        </div>
      </div>
    </section>
  </div>

  <dialog
    ref="createDialog"
    class="modal"
    @close="createForm.error = ''"
    @click.self="closeCreateDialog"
  >
    <form @submit.prevent="submitCreate">
      <h2>新增分类节点</h2>
      <p class="hint">父节点：{{ createForm.parentLabel }}</p>
      <p v-if="createForm.error" class="alert error">{{ createForm.error }}</p>

      <div class="field">
        <label for="new-taxon-rank">阶元</label>
        <select id="new-taxon-rank" v-model="createForm.rank">
          <option v-for="rank in createRanks" :key="rank" :value="rank">
            {{ rankLabel(rank) }}
          </option>
        </select>
      </div>

      <div class="field">
        <label for="new-taxon-scientific">学名</label>
        <input
          id="new-taxon-scientific"
          v-model="createForm.scientificName"
          :maxlength="TAXON_NAME_MAX_CHARS"
          required
        />
      </div>

      <div class="field">
        <label for="new-taxon-chinese">中文名</label>
        <input id="new-taxon-chinese" v-model="createForm.chineseName" />
      </div>

      <div class="actions">
        <button type="submit" :disabled="createForm.submitting">
          {{ createForm.submitting ? '保存中…' : '创建' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="createForm.submitting"
          @click="closeCreateDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <dialog ref="editDialog" class="modal" @close="editForm.error = ''" @click.self="closeEditDialog">
    <form @submit.prevent="submitEdit">
      <h2>编辑分类节点</h2>
      <p class="hint">
        阶元「{{ rankLabel(editForm.rank) }}」不可修改；改阶元等于移动整棵子树，应该新建节点。
      </p>
      <p v-if="editForm.error" class="alert error">{{ editForm.error }}</p>

      <div class="field">
        <label for="edit-taxon-scientific">学名</label>
        <input
          id="edit-taxon-scientific"
          v-model="editForm.scientificName"
          :maxlength="TAXON_NAME_MAX_CHARS"
          required
        />
      </div>

      <div class="field">
        <label for="edit-taxon-chinese">中文名</label>
        <input id="edit-taxon-chinese" v-model="editForm.chineseName" />
      </div>

      <div class="field">
        <label>父节点（只列出阶元更高的候选，清空则移到根）</label>
        <TaxonPicker
          :key="editFormKey"
          v-model="editForm.parentId"
          :max-rank="editForm.rank"
          :disabled="editForm.submitting"
        />
      </div>

      <div class="actions">
        <button type="submit" :disabled="editForm.submitting">
          {{ editForm.submitting ? '保存中…' : '保存' }}
        </button>
        <button
          class="secondary"
          type="button"
          :disabled="editForm.submitting"
          @click="closeEditDialog"
        >
          取消
        </button>
      </div>
    </form>
  </dialog>

  <ConfirmDialog
    ref="deleteDialog"
    title="删除分类节点"
    :message="
      pendingDelete === null
        ? ''
        : `确定删除节点「${taxonLabel(pendingDelete)}」？有子节点或名录记录引用它时会被拒绝。`
    "
    :busy="deleting"
    :error="deleteError"
    @confirm="confirmDelete"
    @cancel="deleteError = ''"
  />
</template>
