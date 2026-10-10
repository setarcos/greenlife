import { createRouter, createWebHistory } from 'vue-router'
import { isForbidden } from '../api/http'
import { useAuthStore } from '../stores/auth'
import AppLayout from '../layouts/AppLayout.vue'
import LoginView from '../views/LoginView.vue'
import MaintenanceLogsView from '../views/MaintenanceLogsView.vue'
import ProfileView from '../views/ProfileView.vue'
import SpeciesListsView from '../views/SpeciesListsView.vue'
import SpeciesSearchView from '../views/SpeciesSearchView.vue'
import TaxonomyTreeView from '../views/TaxonomyTreeView.vue'
import UsersView from '../views/UsersView.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', name: 'login', component: LoginView },
    {
      path: '/',
      component: AppLayout,
      // 物种浏览（名录 / 分类树 / 检索 / 维护日志）是公开的，和后端 /taxonomy、
      // /maintenance-logs 读接口一致；只有「我的资料」和用户管理要登录。
      children: [
        { path: '', name: 'species-lists', component: SpeciesListsView },
        { path: 'tree', name: 'taxonomy-tree', component: TaxonomyTreeView },
        { path: 'search', name: 'species-search', component: SpeciesSearchView },
        { path: 'logs', name: 'maintenance-logs', component: MaintenanceLogsView },
        {
          path: 'profile',
          name: 'profile',
          component: ProfileView,
          meta: { requiresAuth: true },
        },
        {
          path: 'admin/users',
          name: 'admin-users',
          component: UsersView,
          meta: { requiresAuth: true, requiresAdmin: true },
        },
      ],
    },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()

  if (to.name === 'login') {
    if (!auth.isAuthenticated) {
      return true
    }
    // 已经登录还点登录页：先确认这把 token 还有效，再回首页。
    if (auth.user === null) {
      try {
        await auth.loadMe()
      } catch {
        auth.logout()
        return true
      }
    }
    return { name: 'species-lists' }
  }

  // 已登录但还没有用户资料（刚刷新页面）。公开页面也要拉一次，否则顶栏
  // 不会显示当前用户；失败（401/403）就当作游客继续浏览。
  if (auth.isAuthenticated && auth.user === null) {
    try {
      await auth.loadMe()
    } catch (e) {
      auth.logout()
      if (isForbidden(e) && to.meta.requiresAuth) {
        return { name: 'login', query: { notice: 'no-access' } }
      }
    }
  }

  if (to.meta.requiresAuth && !auth.isAuthenticated) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
  // 非管理员直接访问 /admin/users 时退回「我的资料」。
  // 注意这只影响前端路由，/admin/* 的接口在服务端另有 ADMIN 校验。
  if (to.meta.requiresAdmin && !auth.isAdmin) {
    return { name: 'profile' }
  }
  return true
})
