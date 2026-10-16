<script setup lang="ts">
import { computed, watch } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '../stores/auth'
import { usePendingUsersStore } from '../stores/pendingUsers'
import { roleLabel } from '../types'

const auth = useAuthStore()
const pendingUsers = usePendingUsersStore()
const route = useRoute()
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

/// 退出登录只改登录态，不跳登录页：公开页面（物种名录 / 分类树 / 鸟类调查）
/// 留在原地继续当游客看；需要登录的页面（个人中心 / 维护日志 / 用户管理）
/// 内容依赖 auth.user，留着只会是一片空白，所以退回公开首页。
async function logout(): Promise<void> {
  auth.logout()
  if (route.meta.requiresAuth) {
    await router.push({ name: 'species-lists' })
  }
}

/// 管理界面标签：有 STAFF/ADMIN 权限才显示，点一下在「浏览界面 / 管理界面」
/// 之间切换。状态存在 auth store 里，切换页面不会丢。
function toggleManageMode(): void {
  auth.setManageMode(!auth.manageMode)
}
</script>

<template>
  <header class="app-header">
    <span class="brand">PKU Greenlife</span>
    <nav class="nav">
      <RouterLink to="/">物种名录</RouterLink>
      <RouterLink to="/tree">分类树</RouterLink>
      <RouterLink to="/birds">鸟类调查</RouterLink>
      <!-- 维护日志连读都要 STAFF/ADMIN，游客和无权限用户不显示入口 -->
      <RouterLink v-if="auth.isStaff" to="/logs">维护日志</RouterLink>
      <RouterLink v-if="auth.user" to="/profile">个人中心</RouterLink>
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
      <!-- 有权限的用户点这个标签在「浏览界面 / 管理界面」之间切换：
           浏览界面（绿）下各页面的增删改按钮全部隐藏，管理界面（红）才显示。 -->
      <button
        v-if="auth.isStaff"
        class="manage-switch"
        :class="auth.manageMode ? 'managing' : 'browsing'"
        type="button"
        :title="
          auth.manageMode
            ? '当前是管理界面，点击切换回浏览界面（隐藏增删改按钮）'
            : '当前是浏览界面，点击切换到管理界面（显示增删改按钮）'
        "
        @click="toggleManageMode"
      >
        {{ auth.manageMode ? '管理界面' : '浏览界面' }}
      </button>
      <span>
        {{ auth.user.name }}
        <span class="muted">（{{ roleText }}）</span>
      </span>
      <button class="link" type="button" @click="logout">退出登录</button>
    </div>
    <!-- 带上当前页作为来源：登录成功后直接回到这里，不用再自己找回来 -->
    <RouterLink
      v-else
      class="login-link"
      :to="{ name: 'login', query: { redirect: route.fullPath } }"
    >
      登录
    </RouterLink>
  </header>
  <main class="app-main">
    <RouterView />
  </main>
</template>
