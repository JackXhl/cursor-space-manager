<script setup lang="ts">
/**
 * Shows how full a volume is, with the portion Cursor occupies called out
 * separately. That second segment is the whole reason a user opens this tool,
 * so it gets its own colour rather than being folded into "used".
 */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { formatBytes, formatExactBytes, percentage } from '@/utils/format'
import type { SizeUnit } from '@/types/domain'

const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    totalBytes: number
    freeBytes: number
    highlightBytes?: number
    highlightLabel?: string
    unit?: SizeUnit
    compact?: boolean
  }>(),
  { highlightBytes: 0, highlightLabel: '', unit: 'auto', compact: false },
)

const usedLabel = computed(() => props.highlightLabel || t('disk.cursorUsed'))

const usedBytes = computed(() => Math.max(0, props.totalBytes - props.freeBytes))
const highlight = computed(() => Math.min(props.highlightBytes, usedBytes.value))
const otherUsed = computed(() => Math.max(0, usedBytes.value - highlight.value))

const otherPercent = computed(() => percentage(otherUsed.value, props.totalBytes))
const highlightPercent = computed(() => percentage(highlight.value, props.totalBytes))
const usedPercent = computed(() => percentage(usedBytes.value, props.totalBytes))

/** Under 10% free is the point where Windows itself starts misbehaving. */
const pressure = computed(() => {
  const freeRatio = props.totalBytes ? props.freeBytes / props.totalBytes : 1
  if (freeRatio < 0.05) return 'critical'
  if (freeRatio < 0.1) return 'low'
  return 'ok'
})
</script>

<template>
  <div class="disk" :class="{ 'disk--compact': compact }">
    <div
      class="disk__track"
      role="img"
      :aria-label="t('disk.aria', { used: formatBytes(usedBytes, unit), total: formatBytes(totalBytes, unit), label: usedLabel, highlight: formatBytes(highlight, unit) })"
    >
      <div class="disk__segment disk__segment--other" :style="{ width: `${otherPercent}%` }" />
      <div
        v-if="highlightPercent > 0"
        class="disk__segment disk__segment--highlight"
        :style="{ width: `${highlightPercent}%` }"
      />
    </div>

    <div v-if="!compact" class="disk__legend">
      <span class="legend">
        <i class="legend__swatch legend__swatch--highlight" aria-hidden="true" />
        {{ usedLabel }}
        <b :title="formatExactBytes(highlight)">{{ formatBytes(highlight, unit) }}</b>
      </span>
      <span class="legend">
        <i class="legend__swatch legend__swatch--other" aria-hidden="true" />
        {{ t('disk.otherUsed') }}
        <b :title="formatExactBytes(otherUsed)">{{ formatBytes(otherUsed, unit) }}</b>
      </span>
      <span class="legend legend--free" :data-pressure="pressure">
        <i class="legend__swatch legend__swatch--free" aria-hidden="true" />
        {{ t('disk.free') }}
        <b :title="formatExactBytes(freeBytes)">{{ formatBytes(freeBytes, unit) }}</b>
      </span>
      <span class="legend legend--total">{{ t('disk.total', { size: formatBytes(totalBytes, unit), percent: usedPercent.toFixed(0) }) }}</span>
    </div>
  </div>
</template>

<style scoped>
.disk {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.disk__track {
  display: flex;
  height: 10px;
  border-radius: var(--radius-full);
  background: var(--surface-sunken);
  border: 1px solid var(--border);
  overflow: hidden;
}

.disk--compact .disk__track {
  height: 6px;
}

.disk__segment {
  height: 100%;
  transition: width var(--duration-base) var(--ease);
}

.disk__segment--other {
  background: var(--border-strong);
}

.disk__segment--highlight {
  background: var(--accent);
}

.disk__legend {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-4);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.legend {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.legend b {
  color: var(--text);
  font-weight: 580;
  font-variant-numeric: tabular-nums;
}

.legend__swatch {
  width: 8px;
  height: 8px;
  border-radius: 2px;
}

.legend__swatch--highlight {
  background: var(--accent);
}

.legend__swatch--other {
  background: var(--border-strong);
}

.legend__swatch--free {
  background: var(--surface-sunken);
  border: 1px solid var(--border-strong);
}

.legend--free[data-pressure='low'] b {
  color: var(--warning);
}

.legend--free[data-pressure='critical'] b {
  color: var(--danger);
}

.legend--total {
  margin-left: auto;
  color: var(--text-tertiary);
}
</style>
