import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '../stores/auth'
import AppLayout from '../layouts/AppLayout.vue'
import BirdSurveyView from '../views/BirdSurveyView.vue'
import LoginView from '../views/LoginView.vue'
import MaintenanceLogsView from '../views/MaintenanceLogsView.vue'
import PhotosView from '../views/PhotosView.vue'
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
      // 物种浏览（名录 / 分类树 / 检索 / 鸟类调查）是公开的，和后端 /taxonomy、
      // /bird-records 读接口一致；「维护日志」连读都要 STAFF/ADMIN（无权限用户和
      // 游客都进不去），「我的资料」要登录，用户管理要 ADMIN。
      children: [
        { path: '', name: 'species-lists', component: SpeciesListsView },
        { path: 'tree', name: 'taxonomy-tree', component: TaxonomyTreeView },
        { path: 'search', name: 'species-search', component: SpeciesSearchView },
        { path: 'birds', name: 'bird-survey', component: BirdSurveyView },
        // 物种照片：公开可读，从物种卡片进入（`?taxon=<id>`）。
        { path: 'photos', name: 'photos', component: PhotosView },
        // 维护日志：唯一一个连读都要 STAFF/ADMIN 的页面，后端 /staff/maintenance-logs
        // 同步加了这个限制。
        {
          path: 'logs',
          name: 'maintenance-logs',
          component: MaintenanceLogsView,
          meta: { requiresAuth: true, requiresStaff: true },
        },
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
  // 不会显示当前用户；失败（token 失效 / 账号已删除）就当作游客继续浏览。
  // `GET /me` 只要求登录，无权限账号也能拉到，所以这里不会因为「权限不足」登出。
  if (auth.isAuthenticated && auth.user === null) {
    try {
      await auth.loadMe()
    } catch {
      auth.logout()
    }
  }

  if (to.meta.requiresAuth && !auth.isAuthenticated) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
  // 需要 STAFF 的页面（维护日志）：游客已在上面被送去登录，无权限账号退回首页。
  // 同样只是界面层便利，/staff/* 的接口在服务端另有 PermissionGuard。
  if (to.meta.requiresStaff && !auth.isStaff) {
    return { name: 'species-lists' }
  }
  // 非管理员直接访问 /admin/users 时退回「我的资料」。
  // 注意这只影响前端路由，/admin/* 的接口在服务端另有 ADMIN 校验。
  if (to.meta.requiresAdmin && !auth.isAdmin) {
    return { name: 'profile' }
  }
  return true
})
