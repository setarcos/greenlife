import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { setUnauthorizedHandler } from './api/http'
import { useAuthStore } from './stores/auth'
import './styles.css'

const app = createApp(App)
app.use(createPinia())

// 任何请求返回 401（token 过期、改密码后被作废、账号被删）都立刻清掉本地登录态
// 并回到登录页。放在这里而不是 http.ts 里，是为了避免 http → router/store 的循环导入。
const auth = useAuthStore()
setUnauthorizedHandler(() => {
  auth.logout()
  const current = router.currentRoute.value
  if (current.name !== 'login') {
    void router.replace({ name: 'login', query: { redirect: current.fullPath } })
  }
})

app.use(router)
app.mount('#app')
