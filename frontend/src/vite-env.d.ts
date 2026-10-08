/// 让 `import App from './App.vue'` 在纯 tsc 下也有类型。
/// （vue-tsc 本身能直接解析 .vue，这个 shim 是给编辑器/`tsc` 兜底的。）
declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<Record<string, never>, Record<string, never>, unknown>
  export default component
}
