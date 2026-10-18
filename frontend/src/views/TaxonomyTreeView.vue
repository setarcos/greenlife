<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { errorMessage } from '../api/http'
import { getTaxon, getTree, listSpeciesLists } from '../api/taxonomy'
import { createTaxon, deleteTaxon, updateTaxon } from '../api/staffTaxonomy'
import ConfirmDialog from '../components/ConfirmDialog.vue'
import TaxonPicker from '../components/TaxonPicker.vue'
import TaxonTreeNode from '../components/TaxonTreeNode.vue'
import { useAuthStore } from '../stores/auth'
import { queryText } from '../query'
import {
  RANKS,
  isStrictlyHigher,
  rankLabel,
  taxonLabel,
  type SpeciesList,
  type Taxon,
  type TaxonDetail,
  type TaxonNode,
  type TaxonomyRank,
} from '../taxonomy'
import { TAXON_NAME_MAX_CHARS, charCount, validateField } from '../validation'

/// 树节点和详情都能提供这几个字段，选中状态用这个最小结构即可。
type TaxonRef = Pick<Taxon, 'id' | 'parent_id' | 'rank' | 'scientific_name' | 'chinese_name'>

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()
const canManage = computed(() => auth.canManage)

const tree = ref<TaxonNode[]>([])
const treeEl = ref<HTMLUListElement | null>(null)
const treeLoading = ref(false)
const treeError = ref('')

/// 展开的节点 id。默认全折叠（整棵树 3000+ 节点，不能一次全画出来），
/// 展开状态提到这里是因为选中一个节点时要把它的祖先链一起展开。
const expandedIds = ref(new Set<string>())

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
  // 地址栏里的 `?taxon=` 可能比树先到（onMounted 里两者并行），树画出来之后再展开、
  // 定位一次。要放在 finally 之后：加载中 `.tree` 根本不在 DOM 里，那时拿不到元素。
  if (selected.value !== null) {
    await revealInTree(selected.value.id)
  }
}

onMounted(async () => {
  // 地址栏里的 `?taxon=` 直接去取那个节点，不用等树加载完。
  await Promise.all([loadTree(), syncFromRoute()])
})

// ==========================================================================
// 树的展开 / 定位
// ==========================================================================

/// 在已加载的树里找 `id` 的祖先（从根到父）。节点不在树里（树还没加载完）时返回 null。
function ancestorIds(nodes: TaxonNode[], id: string, trail: string[] = []): string[] | null {
  for (const node of nodes) {
    if (node.id === id) {
      return trail
    }
    const found = ancestorIds(node.children, id, [...trail, node.id])
    if (found !== null) {
      return found
    }
  }
  return null
}

/// 把 `id` 的祖先链全部展开，让它在左边的树里露出来。
/// 树是独立的滚动容器（`.tree` 是 max-height + overflow），所以展开完还要把它滚到可见处
/// —— 否则在右侧详情里逐级点下去，左边虽然展开了却停在原地。
async function revealInTree(id: string): Promise<void> {
  const trail = ancestorIds(tree.value, id)
  if (trail !== null) {
    for (const ancestorId of trail) {
      expandedIds.value.add(ancestorId)
    }
  }
  await nextTick()
  treeEl.value?.querySelector('.tree-row.selected')?.scrollIntoView({ block: 'nearest' })
}

function toggleExpanded(id: string): void {
  if (expandedIds.value.has(id)) {
    expandedIds.value.delete(id)
  } else {
    expandedIds.value.add(id)
  }
}

/// 选中项一换就展开它的祖先链：点树上的节点（已经看得见）、点右侧详情里的子节点 /
/// 面包屑、带 `?taxon=` 进页、新增完子节点，走的都是这一条。
watch(
  () => selected.value?.id ?? null,
  (id) => {
    if (id !== null) {
      void revealInTree(id)
    }
  },
)

// ==========================================================================
// 选中的节点 ↔ 地址栏
// ==========================================================================

/// 选中的节点写进地址栏：点「查看物种」跳到物种名录（带上类群筛选）、打开物种卡片看
/// 照片，之后按后退回来还是停在这个节点上；刷新和分享链接也一样。
function writeQuery(id: string | null): void {
  void router.replace({ query: id === null ? {} : { taxon: id } })
}

async function selectNode(node: TaxonRef): Promise<void> {
  selected.value = node
  savedMessage.value = ''
  detailError.value = ''
  writeQuery(node.id)
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

/// 跟着地址栏变：`?taxon=` 指向的节点就选中它，没有就清空选中。
/// 这个节点可能不在当前展开的树里（链接是别人发的），所以直接拿详情来当选中项。
async function syncFromRoute(): Promise<void> {
  const id = queryText(route.query.taxon)
  if (id === '') {
    if (selected.value !== null) {
      selected.value = null
      detail.value = null
    }
    return
  }
  if (selected.value?.id === id) {
    return
  }
  try {
    detail.value = await getTaxon(id)
    selected.value = detail.value
    detailError.value = ''
  } catch {
    // 节点已经不在了：地址栏里的这个 id 没用了。
    detail.value = null
    selected.value = null
    writeQuery(null)
  }
}

// 地址栏变了但组件没重建（浏览器前进后退、点顶栏的「分类树」）时跟着变。
watch(
  () => route.query,
  () => void syncFromRoute(),
)

// --- 新增节点 ---

const createDialog = ref<HTMLDialogElement | null>(null)
/// 名录只在需要时拉（根节点要选名录），`GET /taxonomy/lists` 已经按 position 排好。
const lists = ref<SpeciesList[]>([])
const listsError = ref('')
const createForm = reactive({
  parentId: null as string | null,
  parentRank: null as TaxonomyRank | null,
  parentLabel: '（根节点）',
  /// 根节点必选：这棵树属于哪份名录。子节点跟着根走，恒为 null。
  listId: null as string | null,
  /// 新增根节点时的默认阶元：树根通常就是门（见 `RANKS`）。
  rank: 'phylum' as TaxonomyRank,
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
  createForm.listId = null
  createForm.rank = createRanks.value[0] ?? 'species'
  createForm.scientificName = ''
  createForm.chineseName = ''
  createForm.error = ''
  if (parent === null && lists.value.length === 0) {
    void loadLists()
  }
  if (createDialog.value !== null && !createDialog.value.open) {
    createDialog.value.showModal()
  }
}

/// 拉名录供根节点选择。只拉一次；失败了下次打开弹窗再试。
async function loadLists(): Promise<void> {
  listsError.value = ''
  try {
    lists.value = await listSpeciesLists()
  } catch (e) {
    listsError.value = errorMessage(e)
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
  if (createForm.parentId === null && createForm.listId === null) {
    createForm.error = '请选择根节点所属的物种名录'
    return
  }

  createForm.submitting = true
  try {
    const created = await createTaxon({
      parent_id: createForm.parentId,
      list_id: createForm.listId,
      rank: createForm.rank,
      scientific_name: createForm.scientificName.trim(),
      chinese_name: chineseName === '' ? null : chineseName,
    })
    closeCreateDialog()
    savedMessage.value =
      created.rank === 'species'
        ? `已新增节点「${taxonLabel(created)}」，并已在所属名录中添加一条记录`
        : `已新增节点「${taxonLabel(created)}」`
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
      writeQuery(null)
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
        <ul ref="treeEl" class="tree">
          <TaxonTreeNode
            v-for="node in tree"
            :key="node.id"
            :node="node"
            :selected-id="selected === null ? null : selected.id"
            :expanded-ids="expandedIds"
            @select="selectNode"
            @toggle="toggleExpanded"
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

            <div class="actions">
              <RouterLink
                class="link-button secondary"
                :to="{ name: 'species-lists', query: { taxon: detail.id } }"
              >
                查看物种
              </RouterLink>
              <template v-if="canManage">
                <button type="button" @click="openCreate(detail)">新增子节点</button>
                <button class="secondary" type="button" @click="openEdit(detail)">编辑</button>
                <button class="danger" type="button" @click="openDelete(detail)">删除</button>
              </template>
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

      <div v-if="createForm.parentId === null" class="field">
        <label for="new-taxon-list">物种名录</label>
        <select id="new-taxon-list" v-model="createForm.listId" required>
          <option :value="null" disabled>选择这棵树所属的名录</option>
          <option v-for="list in lists" :key="list.id" :value="list.id">{{ list.name }}</option>
        </select>
        <p v-if="listsError" class="alert error">{{ listsError }}</p>
        <p class="hint">根节点归属的名录决定这棵树里的新种落在哪份名录里。</p>
      </div>

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

      <p v-if="createForm.rank === 'species'" class="hint">
        新增的种会自动在所属名录里添加一条记录（学名、中文名照抄本节点）。
      </p>

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
      <p v-if="editForm.rank === 'species'" class="hint">
        改名会同步更新所属名录里写着旧名的记录（沿用异名、另起中文名的记录不动）。
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
