<script setup lang="ts">
import { reactive, ref } from 'vue'
import { errorMessage } from '../api/http'
import { createRecord, updateRecord } from '../api/staffTaxonomy'
import type { SpeciesRecord } from '../taxonomy'
import { RECORD_NO_MAX_CHARS, TAXON_NAME_MAX_CHARS, charCount, validateField } from '../validation'
import TaxonPicker from './TaxonPicker.vue'

/// 名录记录的「新增 / 编辑」弹窗。物种名录页用它编当前选中的那份名录；
/// 「鸟类调查」的鸟种清单是跨名录的（鸟纲下的全部记录），也用它就地编辑 ——
/// 记录属于哪份名录就还写回哪份，所以名录由调用方在 `open()` 时给出来，
/// 弹窗本身不去读页面状态。
const emit = defineEmits<{ saved: [record: SpeciesRecord, created: boolean] }>()

/// 空串统一转成 null（后端的可选字段用 null 表示清空）。
function optional(value: string): string | null {
  const trimmed = value.trim()
  return trimmed === '' ? null : trimmed
}

const dialog = ref<HTMLDialogElement | null>(null)
/// 每次打开自增，作为 TaxonPicker 的 key —— 强制重建，丢掉上一次的选择。
const pickerKey = ref(0)
/// 记录写回的名录；打开时定下，提交时不再从别处读。
const listId = ref<string | null>(null)
/// 顶部「所属名录」那一行用的名字；调用方没给就不显示。
const listName = ref<string | null>(null)
const form = reactive({
  id: null as string | null,
  taxonId: null as string | null,
  scientificName: '',
  chineseName: '',
  distribution: '',
  note: '',
  source: '',
  recordNo: '',
  submitting: false,
  error: '',
})

function resetForm(): void {
  form.id = null
  form.taxonId = null
  form.scientificName = ''
  form.chineseName = ''
  form.distribution = ''
  form.note = ''
  form.source = ''
  form.recordNo = ''
  form.error = ''
}

/// 打开弹窗。`record` 不传就是新增；`listName` 只是显示用的所属名录名。
function open(target: { listId: string; listName?: string | null; record?: SpeciesRecord }): void {
  resetForm()
  listId.value = target.listId
  listName.value = target.listName ?? null
  const record = target.record
  if (record !== undefined) {
    form.id = record.id
    form.taxonId = record.taxon_id
    form.scientificName = record.scientific_name
    form.chineseName = record.chinese_name ?? ''
    form.distribution = record.distribution ?? ''
    form.note = record.note ?? ''
    form.source = record.source ?? ''
    form.recordNo = record.record_no ?? ''
  }
  pickerKey.value += 1
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }
}

function close(): void {
  dialog.value?.close()
}

async function submit(): Promise<void> {
  form.error = ''
  const targetListId = listId.value
  if (targetListId === null) {
    return
  }
  if (form.taxonId === null) {
    form.error = '请选择鉴定到的分类节点'
    return
  }
  const problem = validateField('学名', form.scientificName, TAXON_NAME_MAX_CHARS)
  if (problem !== '') {
    form.error = problem
    return
  }
  const recordNo = form.recordNo.trim()
  if (recordNo !== '' && charCount(recordNo) > RECORD_NO_MAX_CHARS) {
    form.error = `编号最多 ${RECORD_NO_MAX_CHARS} 个字符`
    return
  }

  form.submitting = true
  try {
    const payload = {
      list_id: targetListId,
      taxon_id: form.taxonId,
      scientific_name: form.scientificName.trim(),
      chinese_name: optional(form.chineseName),
      distribution: optional(form.distribution),
      note: optional(form.note),
      source: optional(form.source),
      record_no: optional(form.recordNo),
    }
    const id = form.id
    const record = id === null ? await createRecord(payload) : await updateRecord(id, payload)
    close()
    // 提示与列表刷新交给调用方：两个页面各自刷自己那份数据。
    emit('saved', record, id === null)
  } catch (e) {
    form.error = errorMessage(e)
  } finally {
    form.submitting = false
  }
}

defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="modal modal-wide" @click.self="close">
    <form @submit.prevent="submit">
      <h2>{{ form.id === null ? '新增记录' : '编辑记录' }}</h2>
      <p v-if="listName !== null" class="hint">所属名录：{{ listName }}</p>
      <p v-if="form.error" class="alert error">{{ form.error }}</p>

      <div class="field">
        <label>鉴定到的分类节点（必选；只定到属也可以停在「属」）</label>
        <TaxonPicker :key="pickerKey" v-model="form.taxonId" :disabled="form.submitting" />
      </div>

      <div class="field">
        <label for="record-scientific-name">学名</label>
        <input
          id="record-scientific-name"
          v-model="form.scientificName"
          :maxlength="TAXON_NAME_MAX_CHARS"
          required
        />
      </div>

      <div class="field">
        <label for="record-chinese-name">中文名</label>
        <input id="record-chinese-name" v-model="form.chineseName" />
      </div>

      <div class="field">
        <label for="record-distribution">分布</label>
        <input id="record-distribution" v-model="form.distribution" />
      </div>

      <div class="field">
        <label for="record-note">备注</label>
        <input id="record-note" v-model="form.note" />
      </div>

      <div class="field">
        <label for="record-source">来源</label>
        <input id="record-source" v-model="form.source" />
      </div>

      <div class="field">
        <label for="record-no">编号</label>
        <input id="record-no" v-model="form.recordNo" :maxlength="RECORD_NO_MAX_CHARS" />
      </div>

      <div class="actions">
        <button type="submit" :disabled="form.submitting">
          {{ form.submitting ? '保存中…' : '保存' }}
        </button>
        <button class="secondary" type="button" :disabled="form.submitting" @click="close">
          取消
        </button>
      </div>
    </form>
  </dialog>
</template>
