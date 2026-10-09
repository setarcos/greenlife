<script setup lang="ts">
import { ref } from 'vue'

/// 破坏性操作的确认框。用原生 <dialog>，Esc / 点背景都会走 `cancel`
/// —— 父组件挂 @cancel 或 @close 里重置状态即可。
///
/// 父组件通过模板 ref 调用 `open()`；关闭由本组件自己或父组件调 `close()`。
defineProps<{
  title: string
  message: string
  confirmLabel?: string
  busy?: boolean
  error?: string
}>()

const emit = defineEmits<{ confirm: []; cancel: [] }>()

const dialog = ref<HTMLDialogElement | null>(null)

function open(): void {
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }
}

function close(): void {
  dialog.value?.close()
}

/// 取消 / Esc / 点背景都只关掉，不触发 confirm。
function onClose(): void {
  emit('cancel')
}

defineExpose({ open, close })
</script>

<template>
  <dialog ref="dialog" class="modal" @close="onClose" @click.self="close">
    <h2>{{ title }}</h2>
    <p>{{ message }}</p>
    <p v-if="error" class="alert error">{{ error }}</p>

    <div class="actions">
      <button class="danger" type="button" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '处理中…' : (confirmLabel ?? '删除') }}
      </button>
      <button class="secondary" type="button" :disabled="busy" @click="close">取消</button>
    </div>
  </dialog>
</template>
