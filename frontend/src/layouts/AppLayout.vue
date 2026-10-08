<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, RouterView, useRouter } from 'vue-router'
import { useAuthStore } from '../stores/auth'
import { roleLabel } from '../types'

const auth = useAuthStore()
const router = useRouter()

const roleText = computed(() => (auth.user === null ? '' : roleLabel(auth.user.role)))

async function logout(): Promise<void> {
  auth.logout()
  await router.push({ name: 'login' })
}
</script>

<template>
  <header class="app-header">
    <span class="brand">Greenlife</span>
    <nav class="nav">
      <RouterLink to="/">我的资料</RouterLink>
      <!-- 服务端对 /admin/* 另有 ADMIN 校验，这里只是不显示进不去的入口 -->
      <RouterLink v-if="auth.isAdmin" to="/admin/users">用户管理</RouterLink>
    </nav>
    <div v-if="auth.user" class="account">
      <span>
        {{ auth.user.name }}
        <span class="muted">（{{ roleText }}）</span>
      </span>
      <button class="link" type="button" @click="logout">退出登录</button>
    </div>
  </header>
  <main class="app-main">
    <RouterView />
  </main>
</template>
