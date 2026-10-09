<script setup lang="ts">
import { ref } from 'vue'
import { rankLabel, taxonLabel, type TaxonNode } from '../taxonomy'

/// 递归渲染分类树的一层。默认折叠，展开时才渲染子节点
/// —— 整棵树有 3000+ 个节点，不能一次全画出来。
defineOptions({ name: 'TaxonTreeNode' })

defineProps<{
  node: TaxonNode
  selectedId: string | null
}>()

const emit = defineEmits<{ select: [TaxonNode] }>()

const expanded = ref(false)
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
        @click="expanded = !expanded"
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
        @select="emit('select', $event)"
      />
    </ul>
  </li>
</template>
