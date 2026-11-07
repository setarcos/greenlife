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
  <!-- 通栏容器：缎带定位在它的左上角，即窗口左边、header 正下方。不直接用 fixed + 写死的 top，免得窄屏顶栏换行变高时错位 -->
  <div class="app-body">
    <a
      class="github-corner"
      href="https://github.com/setarcos/greenlife"
      target="_blank"
      rel="noopener noreferrer"
      title="在 GitHub 上查看源码"
      aria-label="在 GitHub 上查看源码"
    >
      <svg width="60" height="60" viewBox="0 0 250 250" aria-hidden="true">
        <path d="M0,0 L115,115 L130,115 L142,142 L250,250 L250,0 Z" />
        <path
          class="octo-arm"
          fill="currentColor"
          d="M128.3,109.0 C113.8,99.7 119.0,89.6 119.0,89.6 C122.0,82.7 120.5,78.6 120.5,78.6 C119.2,72.0 123.4,76.3 123.4,76.3 C127.3,80.9 125.5,87.3 125.5,87.3 C122.9,97.6 130.6,101.9 134.4,103.2"
        />
        <path
          class="octo-body"
          fill="currentColor"
          d="M115.0,115.0 C114.9,115.1 118.7,116.5 119.8,115.4 L133.7,101.6 C136.9,99.2 139.9,98.4 142.2,98.6 C133.8,88.0 127.5,74.4 143.8,58.0 C148.5,53.4 154.0,51.2 159.7,51.0 C160.3,49.4 163.2,43.6 171.4,40.1 C171.4,40.1 176.1,42.5 178.8,56.2 C183.1,58.6 187.2,61.8 190.9,65.4 C194.5,69.0 197.7,73.2 200.1,77.6 C213.8,80.2 216.3,84.9 216.3,84.9 C212.7,93.1 206.9,96.0 205.4,96.6 C205.1,102.4 203.0,107.8 198.3,112.5 C181.9,128.9 168.3,122.5 157.7,114.1 C157.9,116.9 156.7,120.9 152.7,124.9 L141.0,136.5 C139.8,137.7 141.6,141.9 141.8,141.8 Z"
        />
      </svg>
    </a>
    <main class="app-main">
      <RouterView />
    </main>
  </div>
</template>
