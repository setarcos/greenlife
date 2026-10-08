<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { errorMessage } from '../api/http'
import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

const username = ref('')
const password = ref('')
const error = ref('')
const submitting = ref(false)

// 从其它页面被踢回登录页时带的提示。
const NOTICES: Record<string, string> = {
  'password-changed': '密码已修改，请用新密码重新登录。',
  // 角色为 NONE 的账号能登录，但连 /staff/me 都是 403，没有任何可用的页面。
  'no-access': '该账号没有任何权限，无法使用。请联系管理员分配权限。',
}
const notice = ref(
  typeof route.query.notice === 'string' ? (NOTICES[route.query.notice] ?? '') : '',
)

async function submit(): Promise<void> {
  error.value = ''
  notice.value = ''
  submitting.value = true
  try {
    await auth.login(username.value, password.value)
    const redirect = route.query.redirect
    await router.replace(typeof redirect === 'string' ? redirect : { name: 'profile' })
  } catch (e) {
    // 401 的响应也会经过 http 拦截器（会把本地登录态清掉），
    // 但错误信息仍然要展示给用户。
    error.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="login-page">
    <form class="card" @submit.prevent="submit">
      <h1>Greenlife 登录</h1>
      <p class="subtitle">请使用用户名和密码登录</p>

      <p v-if="notice" class="alert success">{{ notice }}</p>
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
    </form>
  </div>
</template>
