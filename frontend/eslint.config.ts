import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

export default defineConfigWithVueTs(
  // essential：只包含「会出错」的规则（v-for 缺 key、直接改 props、模板里用 this 等），
  // 不含 max-attributes-per-line 这类排版规则 —— 排版由 prettier 唯一决定，
  // 所以这里不需要再加 eslint-config-prettier。
  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,

  {
    // node_modules 和 .git 是 flat config 的默认忽略项，dist 要自己写
    ignores: ['dist/**'],
  },
)
