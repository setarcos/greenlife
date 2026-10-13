<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { errorMessage } from '../api/http'
import { useAuthStore } from '../stores/auth'
import { formatTimestamp, roleLabel } from '../types'
import {
  CONTACT_MAX_CHARS,
  NAME_MAX_CHARS,
  ORGANIZATION_MAX_CHARS,
  PASSWORD_MAX_BYTES,
  PASSWORD_MIN_CHARS,
  REAL_NAME_MAX_CHARS,
  STUDENT_ID_MAX_CHARS,
  WECHAT_MAX_CHARS,
  charCount,
  validateName,
  validateOptionalField,
  validatePassword,
  validateRealName,
} from '../validation'

const auth = useAuthStore()
const router = useRouter()

/// 注册信息里可以在「个人中心」里查看的几项。用户名与权限单独展示。
const profileFields = computed(() => [
  { label: '昵称', value: auth.user?.name ?? '' },
  { label: '真实姓名', value: auth.user?.real_name ?? '' },
  { label: '学号 / 工号', value: auth.user?.student_id ?? '' },
  { label: '单位', value: auth.user?.organization ?? '' },
  { label: '联系方式', value: auth.user?.contact ?? '' },
  { label: '微信号', value: auth.user?.wechat ?? '' },
])

const profileForm = reactive({
  name: '',
  real_name: '',
  student_id: '',
  organization: '',
  contact: '',
  wechat: '',
  submitting: false,
  error: '',
})

const passwordForm = reactive({
  password: '',
  confirm: '',
  submitting: false,
  error: '',
})

/// 保存成功后 modal 已经关掉了，提示需要有地方落 —— 回显在「账号信息」里。
const savedMessage = ref('')

const profileDialog = ref<HTMLDialogElement | null>(null)
const passwordDialog = ref<HTMLDialogElement | null>(null)

/// JS 的 `.length` 数的是 UTF-16 码元，Rust 的 `chars().count()` 数的是字符，
/// 用 charCount 才能和后端的 `NAME_MAX_CHARS` 对齐（否则一个 emoji 会算成 2 个字符）。
const nameLength = computed(() => charCount(profileForm.name))

/// 每次打开都回到当前值，丢掉上一次没提交的输入和旧错误。
/// 挂在 dialog 的 `close` 事件上，所以「取消 / Esc / 点背景」三条关闭路径都会重置。
function resetProfileForm(): void {
  const user = auth.user
  profileForm.name = user?.name ?? ''
  profileForm.real_name = user?.real_name ?? ''
  profileForm.student_id = user?.student_id ?? ''
  profileForm.organization = user?.organization ?? ''
  profileForm.contact = user?.contact ?? ''
  profileForm.wechat = user?.wechat ?? ''
  profileForm.error = ''
}

function resetPasswordForm(): void {
  passwordForm.password = ''
  passwordForm.confirm = ''
  passwordForm.error = ''
}

function openProfileDialog(): void {
  savedMessage.value = ''
  resetProfileForm()
  // 已经打开时再 showModal() 会抛 InvalidStateError（比如按钮被连点两下）
  if (profileDialog.value !== null && !profileDialog.value.open) {
    profileDialog.value.showModal()
  }
}

function openPasswordDialog(): void {
  savedMessage.value = ''
  resetPasswordForm()
  if (passwordDialog.value !== null && !passwordDialog.value.open) {
    passwordDialog.value.showModal()
  }
}

function closeProfileDialog(): void {
  profileDialog.value?.close()
}

function closePasswordDialog(): void {
  passwordDialog.value?.close()
}

async function submitProfile(): Promise<void> {
  profileForm.error = ''

  const problem =
    validateName(profileForm.name) ||
    validateRealName(profileForm.real_name) ||
    validateOptionalField('学号/工号', profileForm.student_id, STUDENT_ID_MAX_CHARS) ||
    validateOptionalField('单位', profileForm.organization, ORGANIZATION_MAX_CHARS) ||
    validateOptionalField('联系方式', profileForm.contact, CONTACT_MAX_CHARS) ||
    validateOptionalField('微信号', profileForm.wechat, WECHAT_MAX_CHARS)
  if (problem !== '') {
    profileForm.error = problem
    return
  }

  profileForm.submitting = true
  try {
    // 选填字段传空串 = 清空（后端把它归一成 NULL），所以这里原样提交即可。
    // 请求体里没有 username / role，也就改不到它们。
    await auth.updateProfile({
      name: profileForm.name,
      real_name: profileForm.real_name,
      student_id: profileForm.student_id,
      organization: profileForm.organization,
      contact: profileForm.contact,
      wechat: profileForm.wechat,
    })
    closeProfileDialog()
    savedMessage.value = '注册信息已更新'
  } catch (e) {
    profileForm.error = errorMessage(e)
  } finally {
    profileForm.submitting = false
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

      <div v-for="item in profileFields" :key="item.label" class="field">
        <label>{{ item.label }}</label>
        <div v-if="item.value">{{ item.value }}</div>
        <div v-else class="muted">未填写</div>
      </div>

      <div class="field">
        <label>注册时间</label>
        <div>{{ formatTimestamp(auth.user.created_at) }}</div>
      </div>

      <div class="actions">
        <button class="secondary" type="button" @click="openProfileDialog">修改注册信息</button>
        <button class="secondary" type="button" @click="openPasswordDialog">修改密码</button>
      </div>
      <p v-if="savedMessage" class="alert success">{{ savedMessage }}</p>
    </section>

    <!--
      原生 <dialog> + showModal()：焦点陷阱、Esc 关闭、背景遮挡都是浏览器自带的，
      不用自己写键盘/焦点处理。`@close` 覆盖「取消 / Esc / 点背景」所有关闭路径。
    -->
    <dialog
      ref="profileDialog"
      class="modal modal-wide"
      @close="resetProfileForm"
      @click.self="closeProfileDialog"
    >
      <form @submit.prevent="submitProfile">
        <h2>修改注册信息</h2>
        <p v-if="profileForm.error" class="alert error">{{ profileForm.error }}</p>

        <div class="field">
          <label for="profile-name">昵称</label>
          <input
            id="profile-name"
            v-model="profileForm.name"
            :maxlength="NAME_MAX_CHARS"
            required
          />
          <span class="hint">{{ nameLength }} / {{ NAME_MAX_CHARS }} 字符</span>
        </div>

        <div class="field">
          <label for="profile-real-name">真实姓名</label>
          <input
            id="profile-real-name"
            v-model="profileForm.real_name"
            :maxlength="REAL_NAME_MAX_CHARS"
            required
          />
        </div>

        <div class="field">
          <label for="profile-student-id">学号 / 工号</label>
          <input
            id="profile-student-id"
            v-model="profileForm.student_id"
            :maxlength="STUDENT_ID_MAX_CHARS"
          />
        </div>

        <div class="field">
          <label for="profile-organization">单位</label>
          <input
            id="profile-organization"
            v-model="profileForm.organization"
            :maxlength="ORGANIZATION_MAX_CHARS"
          />
        </div>

        <div class="field">
          <label for="profile-contact">联系方式</label>
          <input
            id="profile-contact"
            v-model="profileForm.contact"
            :maxlength="CONTACT_MAX_CHARS"
          />
        </div>

        <div class="field">
          <label for="profile-wechat">微信号</label>
          <input id="profile-wechat" v-model="profileForm.wechat" :maxlength="WECHAT_MAX_CHARS" />
        </div>
        <p class="hint">选填字段留空表示清除已填写的内容。</p>

        <div class="actions">
          <button type="submit" :disabled="profileForm.submitting">
            {{ profileForm.submitting ? '保存中…' : '保存' }}
          </button>
          <button
            class="secondary"
            type="button"
            :disabled="profileForm.submitting"
            @click="closeProfileDialog"
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
