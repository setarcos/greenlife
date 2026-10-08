<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { errorMessage } from '../api/http'
import { useAuthStore } from '../stores/auth'
import { formatTimestamp, roleLabel } from '../types'
import {
  NAME_MAX_CHARS,
  PASSWORD_MAX_BYTES,
  PASSWORD_MIN_CHARS,
  charCount,
  validateName,
  validatePassword,
} from '../validation'

const auth = useAuthStore()
const router = useRouter()

const nameForm = reactive({
  name: auth.user?.name ?? '',
  submitting: false,
  error: '',
})

const passwordForm = reactive({
  password: '',
  confirm: '',
  submitting: false,
  error: '',
})

/// 改名成功后 modal 已经关掉了，提示需要有地方落 —— 回显在「账号信息」里。
const savedMessage = ref('')

const nameDialog = ref<HTMLDialogElement | null>(null)
const passwordDialog = ref<HTMLDialogElement | null>(null)

/// JS 的 `.length` 数的是 UTF-16 码元，Rust 的 `chars().count()` 数的是字符，
/// 用 charCount 才能和后端的 `NAME_MAX_CHARS` 对齐（否则一个 emoji 会算成 2 个字符）。
const nameLength = computed(() => charCount(nameForm.name))

/// 每次打开都回到当前值，丢掉上一次没提交的输入和旧错误。
/// 挂在 dialog 的 `close` 事件上，所以「取消 / Esc / 点背景」三条关闭路径都会重置。
function resetNameForm(): void {
  nameForm.name = auth.user?.name ?? ''
  nameForm.error = ''
}

function resetPasswordForm(): void {
  passwordForm.password = ''
  passwordForm.confirm = ''
  passwordForm.error = ''
}

function openNameDialog(): void {
  savedMessage.value = ''
  resetNameForm()
  // 已经打开时再 showModal() 会抛 InvalidStateError（比如按钮被连点两下）
  if (nameDialog.value !== null && !nameDialog.value.open) {
    nameDialog.value.showModal()
  }
}

function openPasswordDialog(): void {
  savedMessage.value = ''
  resetPasswordForm()
  if (passwordDialog.value !== null && !passwordDialog.value.open) {
    passwordDialog.value.showModal()
  }
}

function closeNameDialog(): void {
  nameDialog.value?.close()
}

function closePasswordDialog(): void {
  passwordDialog.value?.close()
}

async function submitName(): Promise<void> {
  nameForm.error = ''

  const problem = validateName(nameForm.name)
  if (problem !== '') {
    nameForm.error = problem
    return
  }

  nameForm.submitting = true
  try {
    // 后端只认它认识的字段：改名字不会动 token_version，当前登录态仍然有效。
    await auth.updateProfile({ name: nameForm.name })
    closeNameDialog()
    savedMessage.value = '名字已更新'
  } catch (e) {
    nameForm.error = errorMessage(e)
  } finally {
    nameForm.submitting = false
  }
}

async function submitPassword(): Promise<void> {
  passwordForm.error = ''

  const problem = validatePassword(passwordForm.password)
  if (problem !== '') {
    passwordForm.error = problem
    return
  }
  if (passwordForm.password !== passwordForm.confirm) {
    passwordForm.error = '两次输入的密码不一致'
    return
  }

  passwordForm.submitting = true
  try {
    await auth.updateProfile({ password: passwordForm.password })
    // 后端改密码时会把 token_version +1，本机这把 token 立即失效，
    // 所以这里主动登出并让用户用新密码登录（与 token 的生命周期保持一致，
    // 而不是留着一把已经作废的 token 等下一个请求 401）。
    auth.logout()
    await router.replace({ name: 'login', query: { notice: 'password-changed' } })
  } catch (e) {
    passwordForm.error = errorMessage(e)
    passwordForm.submitting = false
  }
}
</script>

<template>
  <div class="stack">
    <section v-if="auth.user" class="card">
      <h2>账号信息</h2>
      <div class="field">
        <label>用户名</label>
        <div>
          <span class="mono">{{ auth.user.username }}</span>
          <span class="muted">（用户名是登录标识，不能自己修改）</span>
        </div>
      </div>
      <div class="field">
        <label>权限</label>
        <div>
          <span class="tag">{{ roleLabel(auth.user.role) }}</span>
        </div>
      </div>
      <div class="field">
        <label>注册时间</label>
        <div>{{ formatTimestamp(auth.user.created_at) }}</div>
      </div>

      <div class="actions">
        <button class="secondary" type="button" @click="openNameDialog">修改名字</button>
        <button class="secondary" type="button" @click="openPasswordDialog">修改密码</button>
      </div>
      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
    </section>

    <!--
      原生 <dialog> + showModal()：焦点陷阱、Esc 关闭、背景遮挡都是浏览器自带的，
      不用自己写键盘/焦点处理。`@close` 覆盖「取消 / Esc / 点背景」所有关闭路径。
    -->
    <dialog ref="nameDialog" class="modal" @close="resetNameForm" @click.self="closeNameDialog">
      <form @submit.prevent="submitName">
        <h2>修改名字</h2>
        <p v-if="nameForm.error" class="alert error">{{ nameForm.error }}</p>

        <div class="field">
          <label for="profile-name">名字</label>
          <input id="profile-name" v-model="nameForm.name" :maxlength="NAME_MAX_CHARS" required />
          <span class="hint">{{ nameLength }} / {{ NAME_MAX_CHARS }} 字符</span>
        </div>

        <div class="actions">
          <button type="submit" :disabled="nameForm.submitting">
            {{ nameForm.submitting ? '保存中…' : '保存名字' }}
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="nameForm.submitting"
            @click="closeNameDialog"
          >
            取消
          </button>
        </div>
      </form>
    </dialog>

    <dialog
      ref="passwordDialog"
      class="modal"
      @close="resetPasswordForm"
      @click.self="closePasswordDialog"
    >
      <form @submit.prevent="submitPassword">
        <h2>修改密码</h2>
        <p class="alert info">修改密码后当前登录会失效，需要用新密码重新登录。</p>
        <p v-if="passwordForm.error" class="alert error">{{ passwordForm.error }}</p>

        <div class="field">
          <label for="profile-password">新密码</label>
          <input
            id="profile-password"
            v-model="passwordForm.password"
            type="password"
            autocomplete="new-password"
            required
          />
          <span class="hint">
            至少 {{ PASSWORD_MIN_CHARS }} 个字符，最多 {{ PASSWORD_MAX_BYTES }} 字节
          </span>
        </div>

        <div class="field">
          <label for="profile-password-confirm">确认新密码</label>
          <input
            id="profile-password-confirm"
            v-model="passwordForm.confirm"
            type="password"
            autocomplete="new-password"
            required
          />
        </div>

        <div class="actions">
          <button type="submit" :disabled="passwordForm.submitting">
            {{ passwordForm.submitting ? '提交中…' : '修改密码' }}
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="passwordForm.submitting"
            @click="closePasswordDialog"
          >
            取消
          </button>
        </div>
      </form>
    </dialog>
  </div>
</template>
