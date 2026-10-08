import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as authApi from '../api/auth'
import { getToken, setToken } from '../api/http'
import { isAdmin as roleIsAdmin, type User } from '../types'

/// 登录态。token 持久化在 localStorage，user 每次刷新后重新拉取
/// ——权限以服务端当前值为准，不从本地缓存里信任。
export const useAuthStore = defineStore('auth', () => {
  const token = ref<string | null>(getToken())
  const user = ref<User | null>(null)

  const isAuthenticated = computed(() => token.value !== null)
  const isAdmin = computed(() => roleIsAdmin(user.value?.role))

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

  async function updateProfile(payload: { name?: string; password?: string }): Promise<User> {
    const updated = await authApi.updateMe(payload)
    user.value = updated
    return updated
  }

  return { token, user, isAuthenticated, isAdmin, login, logout, loadMe, updateProfile }
})
