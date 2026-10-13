import { ref } from 'vue'
import { defineStore } from 'pinia'
import { listUsers } from '../api/users'
import { isPending } from '../types'

/// 「用户管理」菜单红点上显示的数字：待审批（无权限）用户的数量。
///
/// 单独一个 store 而不是塞进 `stores/auth.ts`：它是用户管理的数据，
/// 登录态本身不关心它。AppLayout 在管理员登录后拉一次，
/// UsersView 拿到列表 / 审批通过后同步一次，这样页面之间跳转时红点不会落后。
export const usePendingUsersStore = defineStore('pendingUsers', () => {
  const pendingCount = ref(0)

  /// 拉一次用户列表并数出待审批人数。只应在管理员登录时调用。
  async function refresh(): Promise<void> {
    try {
      const users = await listUsers()
      pendingCount.value = users.filter((user) => isPending(user.role)).length
    } catch {
      // 拿不到列表（网络错误、权限已变化）时宁可不显示红点，
      // 也不要让一个装饰性的数字把页面打断。
      pendingCount.value = 0
    }
  }

  /// UsersView 已经有最新列表时直接写数，避免重复请求。
  function setCount(next: number): void {
    pendingCount.value = next
  }

  return { pendingCount, refresh, setCount }
})
