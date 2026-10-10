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
    <span class="brand">PKU Greenlife</span>
    <nav class="nav">
      <RouterLink to="/">物种名录</RouterLink>
      <RouterLink to="/tree">分类树</RouterLink>
      <RouterLink to="/search">物种检索</RouterLink>
      <RouterLink to="/birds">鸟类调查</RouterLink>
      <RouterLink to="/logs">维护日志</RouterLink>
      <RouterLink v-if="auth.user" to="/profile">我的资料</RouterLink>
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
    <RouterLink v-else class="login-link" to="/login">登录</RouterLink>
  </header>
  <main class="app-main">
    <RouterView />
  </main>
</template>
