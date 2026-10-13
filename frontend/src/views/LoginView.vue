<script setup lang="ts">
import { reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { register } from '../api/auth'
import { errorMessage } from '../api/http'
import { useAuthStore } from '../stores/auth'
import {
  CONTACT_MAX_CHARS,
  NAME_MAX_CHARS,
  ORGANIZATION_MAX_CHARS,
  REAL_NAME_MAX_CHARS,
  STUDENT_ID_MAX_CHARS,
  USERNAME_MAX_CHARS,
  WECHAT_MAX_CHARS,
  validateName,
  validateOptionalField,
  validatePassword,
  validatePasswordConfirm,
  validateRealName,
  validateUsername,
} from '../validation'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

/// 'login' 与 'register' 是同一张卡片里的两个表单，切换不跳路由：
/// 注册成功后要回到登录表单，跳路由反而要多处理一次查询参数。
const mode = ref<'login' | 'register'>('login')

const username = ref('')
const password = ref('')
const error = ref('')
const submitting = ref(false)

/// 注册成功后回显在登录表单上方。与路由带来的 `notice` 分开，
/// 因为它是本页操作的结果，刷新页面就该消失。
const success = ref('')

// 从其它页面被踢回登录页时带的提示。
const NOTICES: Record<string, string> = {
  'password-changed': '密码已修改，请用新密码重新登录。',
}
const notice = ref(
  typeof route.query.notice === 'string' ? (NOTICES[route.query.notice] ?? '') : '',
)

// --- 注册 ---

const signup = reactive({
  username: '',
  real_name: '',
  name: '',
  password: '',
  confirm: '',
  student_id: '',
  organization: '',
  contact: '',
  wechat: '',
})
const signupError = ref('')
const registering = ref(false)

function switchToRegister(): void {
  mode.value = 'register'
  error.value = ''
  signupError.value = ''
  success.value = ''
}

function switchToLogin(): void {
  mode.value = 'login'
  signupError.value = ''
  success.value = ''
}

async function submitLogin(): Promise<void> {
  error.value = ''
  notice.value = ''
  success.value = ''
  submitting.value = true
  try {
    await auth.login(username.value, password.value)
    const redirect = route.query.redirect
    // 登录后默认回到公开首页（物种名录）。
    await router.replace(typeof redirect === 'string' ? redirect : { name: 'species-lists' })
  } catch (e) {
    // 401 的响应也会经过 http 拦截器（会把本地登录态清掉），
    // 但错误信息仍然要展示给用户。
    error.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}

async function submitSignup(): Promise<void> {
  signupError.value = ''

  const problem =
    validateUsername(signup.username) ||
    validateRealName(signup.real_name) ||
    validateName(signup.name) ||
    validatePassword(signup.password) ||
    validatePasswordConfirm(signup.password, signup.confirm) ||
    validateOptionalField('学号/工号', signup.student_id, STUDENT_ID_MAX_CHARS) ||
    validateOptionalField('单位', signup.organization, ORGANIZATION_MAX_CHARS) ||
    validateOptionalField('联系方式', signup.contact, CONTACT_MAX_CHARS) ||
    validateOptionalField('微信号', signup.wechat, WECHAT_MAX_CHARS)
  if (problem !== '') {
    signupError.value = problem
    return
  }

  registering.value = true
  try {
    await register({
      username: signup.username,
      real_name: signup.real_name,
      name: signup.name,
      password: signup.password,
      student_id: signup.student_id,
      organization: signup.organization,
      contact: signup.contact,
      wechat: signup.wechat,
    })
    // 注册出来的是无权限账号，可以立即登录浏览、维护自己的资料，
    // 只是上传和修改物种数据要等管理员授予 STAFF 权限。
    // 这里不自动登录：回到登录表单让用户自己确认一次。
    username.value = signup.username
    password.value = ''
    signup.password = ''
    signup.confirm = ''
    mode.value = 'login'
    notice.value = ''
    success.value = '注册成功。现在即可登录使用；管理员通过后将获得上传和修改权限。'
  } catch (e) {
    signupError.value = errorMessage(e)
  } finally {
    registering.value = false
  }
}
</script>

<template>
  <div class="login-page">
    <form v-if="mode === 'login'" class="card" @submit.prevent="submitLogin">
      <h1>Greenlife 登录</h1>
      <p class="subtitle">请使用用户名和密码登录</p>

      <p v-if="notice" class="alert success">{{ notice }}</p>
      <p v-if="success" class="alert success">{{ success }}</p>
      <p v-if="error" class="alert error">{{ error }}</p>

      <div class="field">
        <label for="login-username">用户名</label>
        <input
          id="login-username"
          v-model="username"
          name="username"
          autocomplete="username"
          required
          autofocus
        />
      </div>

      <div class="field">
        <label for="login-password">密码</label>
        <input
          id="login-password"
          v-model="password"
          name="password"
          type="password"
          autocomplete="current-password"
          required
        />
      </div>

      <button type="submit" :disabled="submitting">
        {{ submitting ? '登录中…' : '登录' }}
      </button>

      <p class="switch">
        还没有账号？
        <button class="link-inline" type="button" @click="switchToRegister">注册</button>
      </p>
    </form>

    <form v-else class="card wide" @submit.prevent="submitSignup">
      <h1>注册 Greenlife</h1>
      <p class="subtitle">注册后即可登录使用；管理员通过后将获得上传和修改权限</p>

      <div class="field">
        <label for="signup-username">用户名</label>
        <input
          id="signup-username"
          v-model="signup.username"
          name="username"
          autocomplete="username"
          :maxlength="USERNAME_MAX_CHARS"
          required
          autofocus
        />
        <span class="hint">登录标识，全局唯一，注册后不能自己修改</span>
      </div>

      <div class="field">
        <label for="signup-real-name">真实姓名</label>
        <input
          id="signup-real-name"
          v-model="signup.real_name"
          name="real_name"
          autocomplete="name"
          :maxlength="REAL_NAME_MAX_CHARS"
          required
        />
      </div>

      <div class="field">
        <label for="signup-nickname">昵称</label>
        <input
          id="signup-nickname"
          v-model="signup.name"
          name="nickname"
          :maxlength="NAME_MAX_CHARS"
          required
        />
        <span class="hint">站内显示的名字，最多 {{ NAME_MAX_CHARS }} 个字符</span>
      </div>

      <div class="field">
        <label for="signup-password">密码</label>
        <input
          id="signup-password"
          v-model="signup.password"
          type="password"
          autocomplete="new-password"
          required
        />
      </div>

      <div class="field">
        <label for="signup-password-confirm">确认密码</label>
        <input
          id="signup-password-confirm"
          v-model="signup.confirm"
          type="password"
          autocomplete="new-password"
          required
        />
      </div>

      <div class="field">
        <label for="signup-student-id">学号 / 工号（选填）</label>
        <input
          id="signup-student-id"
          v-model="signup.student_id"
          :maxlength="STUDENT_ID_MAX_CHARS"
        />
      </div>

      <div class="field">
        <label for="signup-organization">单位（选填）</label>
        <input
          id="signup-organization"
          v-model="signup.organization"
          autocomplete="organization"
          :maxlength="ORGANIZATION_MAX_CHARS"
        />
      </div>

      <div class="field">
        <label for="signup-contact">联系方式（选填）</label>
        <input id="signup-contact" v-model="signup.contact" :maxlength="CONTACT_MAX_CHARS" />
      </div>

      <div class="field">
        <label for="signup-wechat">微信号（选填）</label>
        <input id="signup-wechat" v-model="signup.wechat" :maxlength="WECHAT_MAX_CHARS" />
      </div>

      <p v-if="signupError" class="alert error">{{ signupError }}</p>

      <button type="submit" :disabled="registering">
        {{ registering ? '注册中…' : '注册' }}
      </button>

      <p class="switch">
        已有账号？
        <button class="link-inline" type="button" @click="switchToLogin">返回登录</button>
      </p>
    </form>
  </div>
</template>
