<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { errorMessage } from '../api/http'
import { approveUser, createUser, deleteUser, listUsers, updateUser } from '../api/users'
import { useAuthStore } from '../stores/auth'
import { usePendingUsersStore } from '../stores/pendingUsers'
import { ROLE_OPTIONS, formatTimestamp, isPending, roleLabel, type Role, type User } from '../types'
import {
  NAME_MAX_CHARS,
  USERNAME_MAX_CHARS,
  validateName,
  validatePassword,
  validateUsername,
} from '../validation'

const auth = useAuthStore()
const pendingUsers = usePendingUsersStore()

const users = ref<User[]>([])
const loading = ref(false)
const listError = ref('')

/// 无权限账号 = 自助注册后等待审批的用户。
const pending = computed(() => users.value.filter((user) => isPending(user.role)))

/// 新增成功后 modal 已经关掉了，提示回显在列表上方。
const savedMessage = ref('')

const createDialog = ref<HTMLDialogElement | null>(null)
const deleteDialog = ref<HTMLDialogElement | null>(null)
const detailDialog = ref<HTMLDialogElement | null>(null)

/// 待删目标。删除是破坏性操作，先存下来，确认框里的文案和实际删的 id 都取自这里。
const pendingDelete = ref<User | null>(null)
const deleteError = ref('')
const deleting = ref(false)

/// 查看注册信息的目标。审批通过后列表会重新加载，这个副本仍然指向同一个人。
const detailUser = ref<User | null>(null)

/// 正在审批的用户 id（按钮转圈），以及审批失败的错误。
const approvingId = ref('')
const approveError = ref('')

/// 编辑目标（改权限 / 改密码）。
const editDialog = ref<HTMLDialogElement | null>(null)
const editingUser = ref<User | null>(null)
const editForm = reactive({
  role: 'STAFF' as Role,
  // 留空 = 不改密码。管理员代改密码只有一栏：不需要像注册那样两次确认。
  password: '',
  submitting: false,
  error: '',
})

const form = reactive({
  name: '',
  username: '',
  password: '',
  role: 'STAFF' as Role,
  submitting: false,
  error: '',
})

async function load(): Promise<void> {
  loading.value = true
  listError.value = ''
  try {
    users.value = await listUsers()
    // 菜单红点的数字与这张列表同源，免得再请求一次。
    pendingUsers.setCount(pending.value.length)
  } catch (e) {
    listError.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)

/// 挂在 dialog 的 `close` 事件上，所以「取消 / Esc / 点背景」都会重置。
/// 权限也一并回到默认的 STAFF：宁可每次重选，也不要因为下拉框停在上一次选的
/// 「管理员」而误建一个管理员账号。
function resetForm(): void {
  form.name = ''
  form.username = ''
  form.password = ''
  form.role = 'STAFF'
  form.error = ''
}

function openCreateDialog(): void {
  savedMessage.value = ''
  resetForm()
  // 已经打开时再 showModal() 会抛 InvalidStateError（比如按钮被连点两下）
  if (createDialog.value !== null && !createDialog.value.open) {
    createDialog.value.showModal()
  }
}

function closeCreateDialog(): void {
  createDialog.value?.close()
}

async function submit(): Promise<void> {
  form.error = ''

  const problem =
    validateName(form.name) || validateUsername(form.username) || validatePassword(form.password)
  if (problem !== '') {
    form.error = problem
    return
  }

  form.submitting = true
  try {
    const created = await createUser({
      name: form.name,
      username: form.username,
      password: form.password,
      role: form.role,
    })
    closeCreateDialog()
    savedMessage.value = `已创建用户「${created.name}」（${created.username}）`
    await load()
  } catch (e) {
    form.error = errorMessage(e)
  } finally {
    form.submitting = false
  }
}

/// 审批通过：后端给这个账号加上 STAFF 权限位。
async function approve(user: User): Promise<void> {
  savedMessage.value = ''
  approveError.value = ''
  approvingId.value = user.id
  try {
    await approveUser(user.id)
    savedMessage.value = `已通过「${user.real_name || user.name}」（${user.username}）的注册，已授予会员权限`
    await load()
  } catch (e) {
    approveError.value = errorMessage(e)
  } finally {
    approvingId.value = ''
  }
}

/// 后端也会拒绝管理员删除自己（400），界面上已经把「我」那一行的按钮藏掉了。
function openDeleteDialog(user: User): void {
  savedMessage.value = ''
  listError.value = ''
  deleteError.value = ''
  pendingDelete.value = user
  if (deleteDialog.value !== null && !deleteDialog.value.open) {
    deleteDialog.value.showModal()
  }
}

function closeDeleteDialog(): void {
  deleteDialog.value?.close()
}

/// 挂在 dialog 的 `close` 事件上：取消 / Esc / 点背景都要把待删目标清掉，
/// 否则下次打开会先闪一下上一个用户的信息。
function onDeleteClosed(): void {
  pendingDelete.value = null
  deleteError.value = ''
}

async function confirmDelete(): Promise<void> {
  const user = pendingDelete.value
  if (user === null) {
    return
  }

  deleteError.value = ''
  deleting.value = true
  try {
    await deleteUser(user.id)
    closeDeleteDialog()
    savedMessage.value = `已删除用户「${user.name}」（${user.username}）`
    await load()
  } catch (e) {
    // 失败时留在对话框里显示错误，而不是默默关掉
    deleteError.value = errorMessage(e)
  } finally {
    deleting.value = false
  }
}

function openDetailDialog(user: User): void {
  detailUser.value = user
  if (detailDialog.value !== null && !detailDialog.value.open) {
    detailDialog.value.showModal()
  }
}

function closeDetailDialog(): void {
  detailDialog.value?.close()
}

function onDetailClosed(): void {
  detailUser.value = null
}

/// 打开编辑弹窗。每次都用该用户当前的值重置，避免带上一次编辑的残留。
function openEditDialog(user: User): void {
  savedMessage.value = ''
  editingUser.value = user
  editForm.role = user.role
  editForm.password = ''
  editForm.error = ''
  if (editDialog.value !== null && !editDialog.value.open) {
    editDialog.value.showModal()
  }
}

function closeEditDialog(): void {
  editDialog.value?.close()
}

/// 挂在 dialog 的 `close` 事件上：取消 / Esc / 点背景都要把编辑目标清掉。
function onEditClosed(): void {
  editingUser.value = null
  editForm.password = ''
  editForm.error = ''
}

async function submitEdit(): Promise<void> {
  const user = editingUser.value
  if (user === null) {
    return
  }

  editForm.error = ''

  // 密码可以不填（不改）；填了就必须满足后端同一套策略。
  if (editForm.password !== '') {
    const problem = validatePassword(editForm.password)
    if (problem !== '') {
      editForm.error = problem
      return
    }
  }

  editForm.submitting = true
  try {
    await updateUser(user.id, {
      role: editForm.role,
      // 空串会被后端当成「提供了空密码」报 400，所以留空时干脆不带这个字段。
      password: editForm.password === '' ? undefined : editForm.password,
    })
    closeEditDialog()
    savedMessage.value = `已更新用户「${user.name}」（${user.username}）的权限/密码`
    await load()
  } catch (e) {
    editForm.error = errorMessage(e)
  } finally {
    editForm.submitting = false
  }
}
</script>

<template>
  <div class="stack">
    <!-- 无权限账号单独一块：管理员一眼能看到谁在等审批，通过按钮也在这里 -->
    <section v-if="pending.length > 0" class="card">
      <div class="card-head">
        <h2>待审批用户</h2>
        <span class="muted">共 {{ pending.length }} 人</span>
      </div>

      <p v-if="approveError" class="alert error">{{ approveError }}</p>

      <table class="table">
        <thead>
          <tr>
            <th>昵称</th>
            <th>真实姓名</th>
            <th>用户名</th>
            <th>注册时间</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="user in pending" :key="user.id">
            <td>{{ user.name }}</td>
            <td>{{ user.real_name || '—' }}</td>
            <td class="mono">{{ user.username }}</td>
            <td class="muted">{{ formatTimestamp(user.created_at) }}</td>
            <td class="actions-cell">
              <button type="button" :disabled="approvingId === user.id" @click="approve(user)">
                {{ approvingId === user.id ? '处理中…' : '通过' }}
              </button>
              <button class="secondary" type="button" @click="openDetailDialog(user)">
                注册信息
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <section class="card">
      <div class="card-head">
        <h2>用户列表</h2>
        <div class="head-actions">
          <span class="muted">共 {{ users.length }} 个用户</span>
          <button type="button" @click="openCreateDialog">新增用户</button>
          <button class="secondary" type="button" :disabled="loading" @click="load">刷新</button>
        </div>
      </div>

      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
      <p v-if="listError" class="alert error">{{ listError }}</p>
      <p v-if="loading" class="muted">加载中…</p>

      <table v-else class="table">
        <thead>
          <tr>
            <th>名字</th>
            <th>用户名</th>
            <th>权限</th>
            <th>注册时间</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="user in users" :key="user.id">
            <td>
              {{ user.name }}
              <!-- 后端不允许管理员删除自己，所以自己那一行不给删除按钮 -->
              <span v-if="user.id === auth.user?.id" class="muted">（我）</span>
            </td>
            <td class="mono">{{ user.username }}</td>
            <td>
              <span class="tag">{{ roleLabel(user.role) }}</span>
            </td>
            <td class="muted">{{ formatTimestamp(user.created_at) }}</td>
            <td class="actions-cell">
              <!-- 后端拒绝管理员改自己的权限（400），所以自己那一行不给编辑入口；
                   改自己的密码走「我的资料」 -->
              <button
                v-if="user.id !== auth.user?.id"
                class="secondary"
                type="button"
                @click="openEditDialog(user)"
              >
                编辑
              </button>
              <button class="secondary" type="button" @click="openDetailDialog(user)">
                注册信息
              </button>
              <button
                v-if="user.id !== auth.user?.id"
                class="danger"
                type="button"
                @click="openDeleteDialog(user)"
              >
                删除
              </button>
            </td>
          </tr>
          <tr v-if="users.length === 0">
            <td colspan="5" class="muted">还没有用户</td>
          </tr>
        </tbody>
      </table>
    </section>

    <dialog ref="createDialog" class="modal" @close="resetForm" @click.self="closeCreateDialog">
      <form @submit.prevent="submit">
        <h2>新增用户</h2>
        <p v-if="form.error" class="alert error">{{ form.error }}</p>

        <div class="field">
          <label for="new-name">名字</label>
          <input id="new-name" v-model="form.name" :maxlength="NAME_MAX_CHARS" required />
        </div>

        <div class="field">
          <label for="new-username">用户名</label>
          <input
            id="new-username"
            v-model="form.username"
            :maxlength="USERNAME_MAX_CHARS"
            required
          />
          <span class="hint">用户名是登录标识，全局唯一，创建后不能改</span>
        </div>

        <div class="field">
          <label for="new-password">初始密码</label>
          <input
            id="new-password"
            v-model="form.password"
            type="password"
            autocomplete="new-password"
            required
          />
        </div>

        <div class="field">
          <label for="new-role">权限</label>
          <select id="new-role" v-model="form.role">
            <option v-for="option in ROLE_OPTIONS" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
          <span class="hint">「无权限」的账号能登录，但除了登录什么都做不了</span>
        </div>

        <div class="actions">
          <button type="submit" :disabled="form.submitting">
            {{ form.submitting ? '创建中…' : '创建用户' }}
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="form.submitting"
            @click="closeCreateDialog"
          >
            取消
          </button>
        </div>
      </form>
    </dialog>

    <dialog
      ref="detailDialog"
      class="modal modal-wide"
      @close="onDetailClosed"
      @click.self="closeDetailDialog"
    >
      <template v-if="detailUser">
        <h2>注册信息</h2>
        <div class="info-list">
          <div class="info-row">
            <span class="muted">用户名</span>
            <span class="mono">{{ detailUser.username }}</span>
          </div>
          <div class="info-row">
            <span class="muted">昵称</span>
            <span>{{ detailUser.name }}</span>
          </div>
          <div class="info-row">
            <span class="muted">真实姓名</span>
            <span>{{ detailUser.real_name || '未填写' }}</span>
          </div>
          <div class="info-row">
            <span class="muted">学号 / 工号</span>
            <span>{{ detailUser.student_id || '未填写' }}</span>
          </div>
          <div class="info-row">
            <span class="muted">单位</span>
            <span>{{ detailUser.organization || '未填写' }}</span>
          </div>
          <div class="info-row">
            <span class="muted">联系方式</span>
            <span>{{ detailUser.contact || '未填写' }}</span>
          </div>
          <div class="info-row">
            <span class="muted">微信号</span>
            <span>{{ detailUser.wechat || '未填写' }}</span>
          </div>
          <div class="info-row">
            <span class="muted">权限</span>
            <span class="tag">{{ roleLabel(detailUser.role) }}</span>
          </div>
          <div class="info-row">
            <span class="muted">注册时间</span>
            <span>{{ formatTimestamp(detailUser.created_at) }}</span>
          </div>
        </div>
      </template>

      <div class="actions">
        <button class="secondary" type="button" @click="closeDetailDialog">关闭</button>
      </div>
    </dialog>

    <dialog ref="editDialog" class="modal" @close="onEditClosed" @click.self="closeEditDialog">
      <form @submit.prevent="submitEdit">
        <h2>编辑用户</h2>
        <p v-if="editingUser" class="muted">
          {{ editingUser.name }}（<span class="mono">{{ editingUser.username }}</span
          >）
        </p>
        <p v-if="editForm.error" class="alert error">{{ editForm.error }}</p>

        <div class="field">
          <label for="edit-role">权限</label>
          <select id="edit-role" v-model="editForm.role">
            <option v-for="option in ROLE_OPTIONS" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
        </div>

        <div class="field">
          <label for="edit-password">新密码</label>
          <input
            id="edit-password"
            v-model="editForm.password"
            type="password"
            autocomplete="new-password"
          />
          <span class="hint">留空表示不修改；修改后该用户需要重新登录</span>
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

    <dialog
      ref="deleteDialog"
      class="modal"
      @close="onDeleteClosed"
      @click.self="closeDeleteDialog"
    >
      <h2>删除用户</h2>
      <p v-if="pendingDelete">
        确定删除用户「{{ pendingDelete.name }}」（{{ pendingDelete.username }}）？
        <strong>此操作不可撤销。</strong>
      </p>
      <p v-if="deleteError" class="alert error">{{ deleteError }}</p>

      <div class="actions">
        <button class="danger" type="button" :disabled="deleting" @click="confirmDelete">
          {{ deleting ? '删除中…' : '删除' }}
        </button>
        <button class="secondary" type="button" :disabled="deleting" @click="closeDeleteDialog">
          取消
        </button>
      </div>
    </dialog>
  </div>
</template>
