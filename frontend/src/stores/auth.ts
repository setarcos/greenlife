import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as authApi from '../api/auth'
import { getToken, setToken } from '../api/http'
import { isStaff as roleIsStaff, isAdmin as roleIsAdmin, type User } from '../types'

/// 登录态。token 持久化在 localStorage，user 每次刷新后重新拉取
/// ——权限以服务端当前值为准，不从本地缓存里信任。
export const useAuthStore = defineStore('auth', () => {
  const token = ref<string | null>(getToken())
  const user = ref<User | null>(null)

  const isAuthenticated = computed(() => token.value !== null)
  const isAdmin = computed(() => roleIsAdmin(user.value?.role))
  /// 有没有 STAFF 或 ADMIN 权限（与后端 /staff 作用域一致）：分类树 / 名录 /
  /// 维护日志 / 照片的读写都由它决定。
  const isStaff = computed(() => roleIsStaff(user.value?.role))

  async function login(username: string, password: string): Promise<void> {
    const result = await authApi.login(username, password)
    setToken(result.token)
    token.value = result.token
    user.value = result.user
  }

  function logout(): void {
    setToken(null)
    token.value = null
    user.value = null
  }

  /// 拉一次自己的资料。401 由 http 拦截器触发 logout，这里直接抛出去即可。
  async function loadMe(): Promise<void> {
    user.value = await authApi.fetchMe()
  }

  async function updateProfile(payload: authApi.UpdateMePayload): Promise<User> {
    const updated = await authApi.updateMe(payload)
    user.value = updated
    return updated
  }

  return {
    token,
    user,
    isAuthenticated,
    isAdmin,
    isStaff,
    login,
    logout,
    loadMe,
    updateProfile,
  }
})
