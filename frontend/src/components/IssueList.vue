<script setup lang="ts">
/**
 * Preflight issues.
 *
 * Warnings are acknowledged one at a time rather than behind a single blanket
 * disclaimer, so a user cannot wave away a risk they never actually read.
 */
import { issueHint } from '@/utils/labels'
import type { PreflightIssue } from '@/types/domain'

defineProps<{
  issues: PreflightIssue[]
  acknowledged?: string[]
  requireAcknowledgement?: boolean
}>()

const emit = defineEmits<{ acknowledge: [code: string, value: boolean] }>()
</script>

<template>
  <ul v-if="issues.length" class="issues">
    <li v-for="issue in issues" :key="issue.code" class="issue" :data-severity="issue.severity">
      <span class="issue__icon" aria-hidden="true">
        <svg viewBox="0 0 16 16" width="14" height="14">
          <path
            v-if="issue.severity === 'blocker'"
            d="M8 1.6 15 14H1L8 1.6Zm0 4.2v4m0 2.1v.1"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
          <path
            v-else
            d="M8 1.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13Zm0 3.3v4m0 2.2v.1"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
          />
        </svg>
      </span>

      <div class="issue__body">
        <p class="issue__message">{{ issue.message }}</p>
        <p v-if="issue.detail" class="issue__detail">{{ issue.detail }}</p>
        <p v-if="issueHint(issue.code)" class="issue__hint">{{ issueHint(issue.code) }}</p>

        <label v-if="requireAcknowledgement && issue.severity === 'warning'" class="issue__ack">
          <input
            type="checkbox"
            :checked="acknowledged?.includes(issue.code)"
            @change="emit('acknowledge', issue.code, ($event.target as HTMLInputElement).checked)"
          />
          {{ $t('confirm.ack') }}
        </label>
      </div>
    </li>
  </ul>
</template>

<style scoped>
.issues {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.issue {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid;
  border-radius: var(--radius-sm);
}

.issue[data-severity='blocker'] {
  background: var(--danger-soft);
  border-color: var(--danger-border);
  color: var(--danger);
}

.issue[data-severity='warning'] {
  background: var(--warning-soft);
  border-color: var(--warning-border);
  color: var(--warning);
}

.issue[data-severity='info'] {
  background: var(--accent-soft);
  border-color: var(--accent-border);
  color: var(--accent);
}

.issue__icon {
  flex-shrink: 0;
  margin-top: 2px;
}

.issue__body {
  min-width: 0;
}

.issue__message {
  color: var(--text);
  font-weight: 540;
  font-size: var(--text-sm);
}

.issue__detail,
.issue__hint {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.issue__ack {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-2);
  color: var(--text);
  font-size: var(--text-sm);
  cursor: pointer;
}

.issue__ack input {
  width: 14px;
  height: 14px;
  accent-color: var(--accent);
}
</style>
