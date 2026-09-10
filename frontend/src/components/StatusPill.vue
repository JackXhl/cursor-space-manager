<script setup lang="ts">
import type { Tone } from '@/utils/labels'

withDefaults(defineProps<{ tone?: Tone; dot?: boolean }>(), { tone: 'neutral', dot: false })
</script>

<template>
  <span class="pill" :class="`pill--${tone}`">
    <span v-if="dot" class="pill__dot" aria-hidden="true" />
    <slot />
  </span>
</template>

<style scoped>
.pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 21px;
  padding: 0 8px;
  border-radius: var(--radius-full);
  border: 1px solid;
  font-size: var(--text-xs);
  font-weight: 540;
  white-space: nowrap;
}

.pill__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.pill--neutral {
  background: var(--neutral-soft);
  border-color: var(--border);
  color: var(--text-secondary);
}

.pill--accent {
  background: var(--accent-soft);
  border-color: var(--accent-border);
  color: var(--accent);
}

.pill--success {
  background: var(--success-soft);
  border-color: var(--success-border);
  color: var(--success);
}

.pill--warning {
  background: var(--warning-soft);
  border-color: var(--warning-border);
  color: var(--warning);
}

.pill--danger {
  background: var(--danger-soft);
  border-color: var(--danger-border);
  color: var(--danger);
}
</style>
