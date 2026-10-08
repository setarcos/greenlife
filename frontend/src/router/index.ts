import { createRouter, createWebHistory } from 'vue-router'
import { isForbidden } from '../api/http'
import { useAuthStore } from '../stores/auth'
import AppLayout from '../layouts/AppLayout.vue'
import LoginView from '../views/LoginView.vue'
import ProfileView from '../views/ProfileView.vue'
import UsersView from '../views/UsersView.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', name: 'login', component: LoginView },
    {
      path: '/',
      component: AppLayout,
      // meta 会被子路由继承（vue-router 会把所有 matched 记录的 meta 合并进 to.meta），
      // 所以 requiresAuth 写在这一层就够了。
      meta: { requiresAuth: true },
      children: [
        { path: '', name: 'profile', component: ProfileView },
        {
          path: 'admin/users',
          name: 'admin-users',
          component: UsersView,
          meta: { requiresAdmin: true },
        },
      ],
    },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()

  if (!auth.isAuthenticated) {
    return to.meta.requiresAuth ? { name: 'login', query: { redirect: to.fullPath } } : true
  }

  // 已登录但还没有用户资料（刚刷新页面）。必须先拿到它，
  // 否则 requiresAdmin 无从判断；顺带也验证了本地 token 是否还有效。
  if (auth.user === null) {
    try {
      await auth.loadMe()
    } catch (e) {
      // 401（token 失效）已经由拦截器 logout；403 说明账号存在但角色是 NONE，
      // 连 /staff/me 都读不到，此时无法确定自己是谁，只能退回登录页。
      auth.logout()
      return isForbidden(e)
        ? { name: 'login', query: { notice: 'no-access' } }
        : { name: 'login', query: { redirect: to.fullPath } }
    }
  }

  if (to.name === 'login') {
    return { name: 'profile' }
  }
  // 非管理员直接访问 /admin/users 时退回「我的资料」。
  // 注意这只影响前端路由，/admin/* 的接口在服务端另有 ADMIN 校验。
  if (to.meta.requiresAdmin && !auth.isAdmin) {
    return { name: 'profile' }
  }
  return true
})
