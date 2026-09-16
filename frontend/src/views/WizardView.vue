<script setup lang="ts">
/** Hosts the five wizard steps and keeps the stepper in sync. */
import { computed } from 'vue'
import WizardSteps from '@/components/WizardSteps.vue'
import DetectView from './DetectView.vue'
import RecommendView from './RecommendView.vue'
import ConfirmView from './ConfirmView.vue'
import ExecuteView from './ExecuteView.vue'
import ResultView from './ResultView.vue'
import { useMigrationStore, type WizardStep } from '@/stores/migration'

const migration = useMigrationStore()

const stepComponent = computed(() => {
  switch (migration.step) {
    case 'detect':
      return DetectView
    case 'recommend':
      return RecommendView
    case 'confirm':
      return ConfirmView
    case 'execute':
      return ExecuteView
    case 'done':
      return ResultView
  }
})

function navigate(step: WizardStep) {
  // Never step backwards out of a run that is still touching the filesystem.
  if (migration.running) return
  migration.goTo(step)
}
</script>

<template>
  <div class="wizard">
    <WizardSteps
      class="wizard__steps"
      :current="migration.step"
      @navigate="navigate"
    />
    <component :is="stepComponent" />
  </div>
</template>

<style scoped>
.wizard {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.wizard__steps {
  padding: var(--space-2) var(--space-3);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}
</style>
