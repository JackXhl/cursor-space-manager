<script setup lang="ts">
import { WIZARD_STEPS, type WizardStep } from '@/stores/migration'
import TruncatedText from './TruncatedText.vue'
import { useI18n } from 'vue-i18n'

const props = defineProps<{ current: WizardStep }>()
const emit = defineEmits<{ navigate: [step: WizardStep] }>()
const { t } = useI18n()

function indexOf(step: WizardStep) {
  return WIZARD_STEPS.findIndex((entry) => entry.id === step)
}

function stateOf(index: number) {
  const currentIndex = indexOf(props.current)
  if (index < currentIndex) return 'done'
  if (index === currentIndex) return 'active'
  return 'todo'
}

/** Only completed steps are navigable; skipping ahead would bypass preflight. */
function canNavigate(index: number) {
  return index < indexOf(props.current)
}
</script>

<template>
  <nav class="steps" :aria-label="t('wizard.aria')">
    <ol class="steps__list">
      <li
        v-for="(entry, index) in WIZARD_STEPS"
        :key="entry.id"
        class="steps__item"
        :data-state="stateOf(index)"
      >
        <button
          class="steps__button"
          :disabled="!canNavigate(index)"
          :aria-current="entry.id === current ? 'step' : undefined"
          @click="emit('navigate', entry.id)"
        >
          <span class="steps__marker" aria-hidden="true">
            <svg v-if="stateOf(index) === 'done'" viewBox="0 0 14 14" width="11" height="11">
              <path
                d="M2 7.4 5.3 10.6 12 3.8"
                fill="none"
                stroke="currentColor"
                stroke-width="2.1"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <template v-else>{{ index + 1 }}</template>
          </span>
          <span class="steps__text">
            <span class="steps__label">{{ t(`wizard.${entry.id}.label`) }}</span>
            <TruncatedText class="steps__hint" variant="text" :lines="2" :text="t(`wizard.${entry.id}.hint`)" />
          </span>
        </button>
        <span v-if="index < WIZARD_STEPS.length - 1" class="steps__connector" aria-hidden="true" />
      </li>
    </ol>
  </nav>
</template>

<style scoped>
.steps__list {
  display: flex;
  align-items: center;
}

.steps__item {
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
}

.steps__button {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2);
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  text-align: left;
  min-width: 0;
}

.steps__button:disabled {
  cursor: default;
}

.steps__button:not(:disabled):hover {
  background: var(--surface-hover);
}

.steps__marker {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 1.5px solid var(--border-strong);
  background: var(--surface);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.steps__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.steps__label {
  font-size: var(--text-sm);
  font-weight: 560;
  color: var(--text-secondary);
}

.steps__hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.steps__connector {
  flex: 1;
  height: 1px;
  min-width: 12px;
  background: var(--border);
}

.steps__item[data-state='active'] .steps__marker {
  border-color: var(--accent);
  background: var(--accent);
  color: #fff;
}

.steps__item[data-state='active'] .steps__label {
  color: var(--text);
}

.steps__item[data-state='done'] .steps__marker {
  border-color: var(--success);
  background: var(--success-soft);
  color: var(--success);
}

.steps__item[data-state='done'] .steps__label {
  color: var(--text);
}
</style>
