<script lang="ts">
/// 卡片至少要知道的三样：物种是谁 + 挂在哪个分类节点上。
/// 名录记录的其余字段（所属名录 / 分布 / 编号…）是可选的——鸟类的「重要记录」
/// 没有它们，打开卡片时用 `open()` 的第二个参数直接给出自己的「记录信息」行。
///
/// 放在普通 `<script>` 块里是为了能导出：SpeciesRecordTable 要把卡片的动作按钮
/// 透传给上层，插槽 prop 用的就是这个类型。
export interface SpeciesCardTarget {
  taxon_id: string
  scientific_name: string
  chinese_name: string | null
  list_id?: string
  distribution?: string | null
  note?: string | null
  source?: string | null
  record_no?: string | null
  updated_at?: string
}
</script>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { errorMessage } from '../api/http'
import { listPhotos } from '../api/photos'
import { getTaxon } from '../api/taxonomy'
import { useAuthStore } from '../stores/auth'
import { RANKS, rankLabel, type Taxon } from '../taxonomy'
import { formatTimestamp } from '../types'
import NoteText from './NoteText.vue'

/// 物种信息卡片：点表格里的学名 / 中文名打开。
/// 记录本身自带分布 / 备注 / 来源，分类阶元要从 `/taxonomy/taxa/{id}` 取祖先链。
defineProps<{
  /// 和 SpeciesRecordTable 同一个函数：传了才显示「名录」。
  listName?: (listId: string) => string
}>()

/// 「记录信息」里的一行。
interface InfoRow {
  label: string
  value: string | null
  /// 等宽显示（编号这类）。
  mono?: boolean
}

const dialog = ref<HTMLDialogElement | null>(null)
const record = ref<SpeciesCardTarget | null>(null)
/// 调用方给的行；为 null 时按名录记录的字段渲染默认行。
const rows = ref<InfoRow[] | null>(null)
const chain = ref<Taxon[]>([])
const loading = ref(false)
const error = ref('')

/// 这个物种已有的照片数：> 0 才显示「浏览照片」链接。
const photoCount = ref(0)
const auth = useAuthStore()
const canUpload = computed(() => auth.canManageTaxonomy)

/// 分类树很小且基本不变，同一 taxa 的祖先链缓存下来，反复点开不再请求。
const chainCache = new Map<string, Taxon[]>()
/// 连点两条记录时，只认最后一次请求的结果。
let requestSeq = 0

/// 界 > 门 > 纲 > 目 > 科 > 属 > 种 固定七行，链上缺的阶元（允许跳级）显示「—」。
const rankRows = computed(() =>
  RANKS.map((rank) => ({
    rank,
    label: rankLabel(rank),
    taxon: chain.value.find((node) => node.rank === rank) ?? null,
  })),
)

/// 打开卡片。`info` 传了就代替默认的「记录信息」行。
async function open(target: SpeciesCardTarget, info?: InfoRow[]): Promise<void> {
  const seq = (requestSeq += 1)
  record.value = target
  rows.value = info ?? null
  chain.value = []
  error.value = ''
  loading.value = false
  photoCount.value = 0
  if (dialog.value !== null && !dialog.value.open) {
    dialog.value.showModal()
  }

  // 照片数只用于决定要不要显示「浏览照片」链接，拿第一页的 total 就够了。
  // 失败就当没有照片（公开接口，拒绝访问时不该把卡片整体打挂）。
  void listPhotos({ taxonId: target.taxon_id, limit: 1 })
    .then((page) => {
      if (seq === requestSeq) {
        photoCount.value = page.total
      }
    })
    .catch(() => {})

  const cached = chainCache.get(target.taxon_id)
  if (cached !== undefined) {
    chain.value = cached
    return
  }

  loading.value = true
  try {
    const detail = await getTaxon(target.taxon_id)
    const next = [...detail.path, detail]
    chainCache.set(target.taxon_id, next)
    if (seq !== requestSeq) return
    chain.value = next
  } catch (e) {
    if (seq !== requestSeq) return
    error.value = errorMessage(e)
  } finally {
    if (seq === requestSeq) loading.value = false
  }
}

function close(): void {
  dialog.value?.close()
}

defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="modal species-card" @click.self="close">
    <template v-if="record">
      <div class="species-head">
        <div>
          <h2>{{ record.chinese_name ?? record.scientific_name }}</h2>
          <p class="species-sci">
            <i>{{ record.scientific_name }}</i>
            <span v-if="record.chinese_name === null" class="muted">（暂无中文名）</span>
          </p>
        </div>
        <div class="head-actions">
          <!-- 照片入口：任何登录的 STAFF / ADMIN 都能上传，游客只能看。 -->
          <RouterLink
            v-if="canUpload"
            class="link-button"
            :to="{ name: 'photos', query: { taxon: record.taxon_id, upload: '1' } }"
          >
            上传照片
          </RouterLink>
          <RouterLink
            v-if="photoCount > 0"
            class="link-button secondary"
            :to="{ name: 'photos', query: { taxon: record.taxon_id } }"
          >
            浏览照片（{{ photoCount }}）
          </RouterLink>
          <!-- 调用方从这里加针对这条记录的动作按钮（例如「添加新重要记录」）。
               一起把 close 交出去，按钮自己决定什么时候收起卡片。 -->
          <slot name="actions" :record="record" :close="close" />
          <button class="secondary" type="button" @click="close">关闭</button>
        </div>
      </div>

      <h3>分类阶元</h3>
      <p v-if="error" class="alert error">{{ error }}</p>
      <p v-else-if="loading" class="muted">加载中…</p>
      <div v-else class="rank-list">
        <div v-for="row in rankRows" :key="row.rank" class="rank-row">
          <span class="tag rank-tag">{{ row.label }}</span>
          <span v-if="row.taxon === null" class="muted">—</span>
          <span v-else class="rank-name">
            <span v-if="row.taxon.chinese_name !== null">{{ row.taxon.chinese_name }} </span>
            <i class="muted">{{ row.taxon.scientific_name }}</i>
          </span>
        </div>
      </div>

      <h3>记录信息</h3>
      <div class="info-list">
        <template v-if="rows">
          <div v-for="row in rows" :key="row.label" class="info-row">
            <span class="info-label">{{ row.label }}</span>
            <span :class="row.mono ? 'mono' : null">{{ row.value ?? '—' }}</span>
          </div>
        </template>
        <template v-else>
          <div v-if="listName && record.list_id" class="info-row">
            <span class="info-label">名录</span>
            <span>{{ listName(record.list_id) }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">分布</span>
            <span>{{ record.distribution ?? '—' }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">备注</span>
            <NoteText :note="record.note ?? null" />
          </div>
          <div class="info-row">
            <span class="info-label">来源</span>
            <span>{{ record.source ?? '—' }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">编号</span>
            <span class="mono">{{ record.record_no ?? '—' }}</span>
          </div>
          <div class="info-row">
            <span class="info-label">更新</span>
            <span class="muted">{{
              record.updated_at ? formatTimestamp(record.updated_at) : '—'
            }}</span>
          </div>
        </template>
      </div>
    </template>
  </dialog>
</template>
