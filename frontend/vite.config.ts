import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

/// 开发时后端默认监听 127.0.0.1:8080（见 backend/src/main.rs）。
/// 换端口就改这里。
const BACKEND = 'http://127.0.0.1:8080'

export default defineConfig({
  plugins: [vue()],
  server: {
    // 前端所有后端请求都带 `/api` 前缀（见 src/api/http.ts）。
    // 这里模拟 nginx 的 `location /api/ { proxy_pass http://127.0.0.1:8080/; }`：
    // 转发前把 `/api` 剥掉，于是开发环境和线上看到的路径完全一致。
    proxy: {
      '/api': {
        target: BACKEND,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ''),
      },
    },
  },
})
