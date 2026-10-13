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
  /// 「管理界面 / 浏览界面」开关。有 STAFF/ADMIN 权限的用户可以把它打开，
  /// 打开后才显示各页面的增删改入口；缺省关闭（浏览界面）。
  /// 只存在内存里：刷新页面或重新登录都回到浏览界面，不会忘记切回来。
  const manageMode = ref(false)
  /// 能不能看到增删改按钮：既要有权限，又要显式切到管理界面。
  /// 这里只管界面显隐，服务端对写接口另有权限校验。
  const canManage = computed(() => isStaff.value && manageMode.value)

  function setManageMode(value: boolean): void {
    manageMode.value = value
  }

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
    manageMode.value = false
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
    manageMode,
    canManage,
    setManageMode,
    login,
    logout,
    loadMe,
    updateProfile,
  }
})
