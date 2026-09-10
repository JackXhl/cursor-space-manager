<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import StatusPill from './StatusPill.vue'
import TruncatedText from './TruncatedText.vue'
import { formatBytes, formatCount, formatExactBytes, percentage } from '@/utils/format'
import { linkStateLabel, linkStateTone, recommendationLabel, recommendationTone } from '@/utils/labels'
import type { DataRoot, DirSize, SizeUnit } from '@/types/domain'

const props = withDefaults(
  defineProps<{
    root: DataRoot
    size: DirSize | null
    unit?: SizeUnit
    selectable?: boolean
    selected?: boolean
    disabled?: boolean
    /** Largest directory in the group, used to scale the inline bar. */
    maxBytes?: number
    measuring?: boolean
  }>(),
  { unit: 'auto', selectable: false, selected: false, disabled: false, maxBytes: 0, measuring: false },
)

const emit = defineEmits<{ toggle: [rootId: string] }>()
const { t } = useI18n()

const bytes = computed(() => props.size?.logicalBytes ?? 0)
const barWidth = computed(() => (props.maxBytes > 0 ? percentage(bytes.value, props.maxBytes) : 0))
const hasErrors = computed(() => props.root.scanErrors.length > 0 || (props.size?.errorCount ?? 0) > 0)

function onToggle() {
  if (props.selectable && !props.disabled) emit('toggle', props.root.id)
}
</script>

<template>
  <component
    :is="selectable ? 'label' : 'div'"
    class="row"
    :class="{ 'row--selectable': selectable, 'row--selected': selected, 'row--disabled': disabled }"
  >
    <input
      v-if="selectable"
      class="row__checkbox"
      type="checkbox"
      :checked="selected"
      :disabled="disabled"
      @change="onToggle"
    />

    <div class="row__main">
      <div class="row__heading">
        <span class="row__label">{{ root.label }}</span>
        <StatusPill :tone="recommendationTone[root.recommendation]">
          {{ recommendationLabel(root.recommendation) }}
        </StatusPill>
        <StatusPill v-if="root.linkState.kind !== 'regular'" :tone="linkStateTone(root.linkState)">
          {{ linkStateLabel(root.linkState) }}
        </StatusPill>
        <StatusPill v-if="root.fromLaunchArgument" tone="accent">{{ t('root.specified') }}</StatusPill>
      </div>

      <TruncatedText class="row__path" :text="root.path" />
      <TruncatedText
        v-if="root.resolvedPath && root.resolvedPath !== root.path"
        class="row__path row__path--resolved"
        :text="root.resolvedPath"
      />
      <p class="row__reason">{{ root.reason }}</p>

      <p v-if="hasErrors" class="row__error">
        {{ root.scanErrors.join('; ') || t('root.unread', { n: size?.errorCount }) }}
      </p>
    </div>

    <div class="row__metrics">
      <div class="row__size">
        <span v-if="measuring && !size" class="row__measuring">{{ t('root.measuring') }}</span>
        <template v-else>
          <b :title="formatExactBytes(bytes)">{{ formatBytes(bytes, unit) }}</b>
          <span class="row__files">{{ t('common.files', { count: formatCount(size?.fileCount) }) }}</span>
        </template>
      </div>
      <div v-if="maxBytes > 0" class="row__bar" aria-hidden="true">
        <div class="row__bar-fill" :style="{ width: `${barWidth}%` }" />
      </div>
    </div>
  </component>
</template>

<style scoped>
.row {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border);
}

.row:last-child {
  border-bottom: none;
}

.row--selectable {
  cursor: pointer;
}

.row--selectable:hover {
  background: var(--surface-hover);
}

.row--selected {
  background: var(--accent-soft);
}

.row--disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.row__checkbox {
  margin: 3px 0 0;
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  flex-shrink: 0;
}

.row__main {
  flex: 1;
  min-width: 0;
}

.row__heading {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.row__label {
  font-weight: 580;
}

.row__path {
  margin-top: 2px;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.row__path--resolved {
  color: var(--accent);
}

.row__reason {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.row__error {
  margin-top: var(--space-1);
  color: var(--warning);
  font-size: var(--text-xs);
}

.row__metrics {
  width: 168px;
  flex-shrink: 0;
  text-align: right;
}

.row__size b {
  font-size: var(--text-md);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.row__files {
  display: block;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
}

.row__measuring {
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.row__bar {
  margin-top: var(--space-2);
  height: 4px;
  border-radius: var(--radius-full);
  background: var(--surface-sunken);
  overflow: hidden;
}

.row__bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: var(--radius-full);
  transition: width var(--duration-base) var(--ease);
}
</style>
