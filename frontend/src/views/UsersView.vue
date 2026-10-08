<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { errorMessage } from '../api/http'
import { createUser, deleteUser, listUsers } from '../api/users'
import { useAuthStore } from '../stores/auth'
import { ROLE_OPTIONS, formatTimestamp, roleLabel, type Role, type User } from '../types'
import {
  NAME_MAX_CHARS,
  USERNAME_MAX_CHARS,
  validateName,
  validatePassword,
  validateUsername,
} from '../validation'

const auth = useAuthStore()

const users = ref<User[]>([])
const loading = ref(false)
const listError = ref('')

/// 新增成功后 modal 已经关掉了，提示回显在列表上方。
const savedMessage = ref('')

const createDialog = ref<HTMLDialogElement | null>(null)
const deleteDialog = ref<HTMLDialogElement | null>(null)

/// 待删目标。删除是破坏性操作，先存下来，确认框里的文案和实际删的 id 都取自这里。
const pendingDelete = ref<User | null>(null)
const deleteError = ref('')
const deleting = ref(false)

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
</script>

<template>
  <div class="stack">
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
