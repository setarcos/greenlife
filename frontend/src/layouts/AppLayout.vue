<script setup lang="ts">
import { computed, watch } from 'vue'
import { RouterLink, RouterView, useRouter } from 'vue-router'
import { useAuthStore } from '../stores/auth'
import { usePendingUsersStore } from '../stores/pendingUsers'
import { roleLabel } from '../types'

const auth = useAuthStore()
const pendingUsers = usePendingUsersStore()
const router = useRouter()

const roleText = computed(() => (auth.user === null ? '' : roleLabel(auth.user.role)))

/// 管理员登录时拉一次待审批人数（红点），登出 / 权限变为非管理员时清零。
/// `immediate` 覆盖了「刷新页面时路由守卫已经拉过 loadMe」的情况。
watch(
  () => auth.isAdmin,
  (isAdmin) => {
    if (isAdmin) {
      void pendingUsers.refresh()
    } else {
      pendingUsers.setCount(0)
    }
  },
  { immediate: true },
)

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
      <!-- 维护日志连读都要 STAFF/ADMIN，游客和无权限用户不显示入口 -->
      <RouterLink v-if="auth.isStaff" to="/logs">维护日志</RouterLink>
      <RouterLink v-if="auth.user" to="/profile">我的资料</RouterLink>
      <!-- 服务端对 /admin/* 另有 ADMIN 校验，这里只是不显示进不去的入口 -->
      <RouterLink v-if="auth.isAdmin" to="/admin/users">
        用户管理
        <span
          v-if="pendingUsers.pendingCount > 0"
          class="nav-dot"
          :title="`${pendingUsers.pendingCount} 个待审批用户`"
          >{{ pendingUsers.pendingCount }}</span
        >
      </RouterLink>
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
