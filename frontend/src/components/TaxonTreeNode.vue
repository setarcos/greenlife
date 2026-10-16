<script setup lang="ts">
import { computed } from 'vue'
import { rankLabel, taxonLabel, type TaxonNode } from '../taxonomy'

/// 递归渲染分类树的一层。默认折叠，展开时才渲染子节点
/// —— 整棵树有 3000+ 个节点，不能一次全画出来。
///
/// 展开状态由外面持有（`expandedIds`）：在右侧详情里点一个后代节点时，左边要把它的
/// 祖先链一起展开，否则树上看不到选中的是谁（见 TaxonomyTreeView 的 revealInTree）。
defineOptions({ name: 'TaxonTreeNode' })

const props = defineProps<{
  node: TaxonNode
  selectedId: string | null
  expandedIds: Set<string>
}>()

const emit = defineEmits<{ select: [TaxonNode]; toggle: [string] }>()

const expanded = computed(() => props.expandedIds.has(props.node.id))
</script>

<template>
  <li>
    <div class="tree-row" :class="{ selected: node.id === selectedId }">
      <button
        v-if="node.children.length > 0"
        class="tree-toggle"
        type="button"
        :aria-expanded="expanded"
        :aria-label="expanded ? '折叠' : '展开'"
        @click="emit('toggle', node.id)"
      >
        {{ expanded ? '▾' : '▸' }}
      </button>
      <span v-else class="tree-toggle muted">·</span>

      <button class="tree-label" type="button" @click="emit('select', node)">
        <span class="tag">{{ rankLabel(node.rank) }}</span>
        <span>{{ taxonLabel(node) }}</span>
        <span v-if="node.children.length > 0" class="muted">（{{ node.children.length }}）</span>
      </button>
    </div>

    <ul v-if="expanded && node.children.length > 0" class="tree-children">
      <TaxonTreeNode
        v-for="child in node.children"
        :key="child.id"
        :node="child"
        :selected-id="selectedId"
        :expanded-ids="expandedIds"
        @select="emit('select', $event)"
        @toggle="emit('toggle', $event)"
      />
    </ul>
  </li>
</template>
