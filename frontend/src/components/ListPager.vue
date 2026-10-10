<script setup lang="ts">
import { computed, ref } from 'vue'
import { pageCount } from '../pagination'

/// 分页条：上一页 / 下一页 + 总页数 + 跳到指定页。
///
/// 分页状态由调用方持有（各页面用的是 offset），这里只负责显示和发号施令：
/// 页码越界时夹到 [1, 总页数]，当前页不变则不重复发请求。
const props = defineProps<{
  /// 满足条件的总条数（后端分页响应的 total）。
  total: number
  pageSize: number
  /// 当前页码，从 1 开始。
  page: number
  disabled?: boolean
}>()

const emit = defineEmits<{ change: [page: number] }>()

const totalPages = computed(() => pageCount(props.total, props.pageSize))

/// 跳页输入框里的值。提交后就清空，让输入框始终是个空壳。
///
/// 类型写成 `string | number` 是因为 `<input type="number">` 上 Vue 会自动加
/// `.number` 修饰符：清空时是空串，填了就是 number（同 SpeciesListsView 的
/// 「排序」字段），所以这里一律先 `String(...)` 再判断。
const jumpTo = ref<string | number>('')

function go(page: number): void {
  const next = Math.min(Math.max(1, page), totalPages.value)
  if (next !== props.page) {
    emit('change', next)
  }
}

function submitJump(): void {
  const text = String(jumpTo.value).trim()
  if (!/^\d+$/.test(text)) {
    return
  }
  jumpTo.value = ''
  go(Number(text))
}
</script>

<template>
  <div class="pager">
    <button class="secondary" type="button" :disabled="disabled || page <= 1" @click="go(page - 1)">
      上一页
    </button>
    <span class="muted">第 {{ page }} / {{ totalPages }} 页 · 共 {{ total }} 条</span>
    <button
      class="secondary"
      type="button"
      :disabled="disabled || page >= totalPages"
      @click="go(page + 1)"
    >
      下一页
    </button>
    <span class="pager-jump">
      跳到
      <input
        v-model="jumpTo"
        type="number"
        min="1"
        :max="totalPages"
        :disabled="disabled"
        :aria-label="`跳到第几页（共 ${totalPages} 页）`"
        @keyup.enter="submitJump"
      />
      页
      <button class="secondary" type="button" :disabled="disabled" @click="submitJump">跳转</button>
    </span>
  </div>
</template>
