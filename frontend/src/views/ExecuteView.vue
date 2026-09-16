<script setup lang="ts">
/**
 * Step 4: live progress.
 *
 * The important honesty here is the cancel button: it disappears the moment
 * the cutover begins, because from that point cancelling would leave the
 * filesystem half-changed.
 */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { useAppStore } from '@/stores/app'
import { useMigrationStore } from '@/stores/migration'
import { formatBytes, formatCount, formatDuration, formatSpeed } from '@/utils/format'
import { migrationStateLabel, migrationStateTone } from '@/utils/labels'

const app = useAppStore()
const migration = useMigrationStore()
const { t } = useI18n()

const unit = computed(() => app.settings.sizeUnit)
const progress = computed(() => migration.progress)
const plan = computed(() => migration.plan)

const currentRootLabel = computed(() => {
  const rootId = progress.value?.rootId
  if (!rootId) return null
  return plan.value?.roots.find((root) => root.rootId === rootId)?.label ?? rootId
})

/** Per-directory state, so a long run shows what is done and what is queued. */
const rootStates = computed(() => {
  const roots = plan.value?.roots ?? []
  const activeId = progress.value?.rootId
  const activeIndex = roots.findIndex((root) => root.rootId === activeId)
  return roots.map((root, index) => ({
    ...root,
    status:
      activeIndex === -1
        ? 'pending'
        : index < activeIndex
          ? 'done'
          : index === activeIndex
            ? 'active'
            : 'pending',
  }))
})
</script>

<template>
  <div class="execute">
    <SectionCard>
      <template #actions>
        <AppButton
          v-if="progress?.cancellable && migration.running"
          size="sm"
          variant="ghost"
          @click="migration.cancel()"
        >
          {{ t('execute.cancel') }}
        </AppButton>
      </template>

      <div class="headline">
        <div>
          <div class="headline__row">
            <h2>{{ progress?.message ?? t('execute.preparing') }}</h2>
            <StatusPill v-if="progress" :tone="migrationStateTone(progress.state)" dot>
              {{ migrationStateLabel(progress.state) }}
            </StatusPill>
          </div>
          <p v-if="currentRootLabel" class="headline__sub">{{ t('execute.currentDir', { name: currentRootLabel }) }}</p>
        </div>
        <span class="headline__percent">{{ migration.percentComplete.toFixed(0) }}%</span>
      </div>

      <div
        class="bar"
        role="progressbar"
        :aria-valuenow="Math.round(migration.percentComplete)"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <div class="bar__fill" :style="{ width: `${migration.percentComplete}%` }" />
      </div>

      <dl class="stats">
        <div class="stat">
          <dt>{{ t('execute.processed') }}</dt>
          <dd>
            {{ formatBytes(progress?.processedBytes ?? 0, unit) }} /
            {{ formatBytes(progress?.totalBytes ?? 0, unit) }}
          </dd>
        </div>
        <div class="stat">
          <dt>{{ t('execute.files') }}</dt>
          <dd>
            {{ formatCount(progress?.processedFiles ?? 0) }} /
            {{ formatCount(progress?.totalFiles ?? 0) }}
          </dd>
        </div>
        <div class="stat">
          <dt>{{ t('execute.speed') }}</dt>
          <dd>{{ formatSpeed(progress?.bytesPerSecond ?? 0) }}</dd>
        </div>
        <div class="stat">
          <dt>{{ t('execute.eta') }}</dt>
          <dd>{{ formatDuration(progress?.etaSeconds) }}</dd>
        </div>
      </dl>

      <p class="cancel-note">
        <template v-if="progress?.cancellable">
          {{ t('execute.cancelSafe') }}
        </template>
        <template v-else-if="migration.running">
          {{ t('execute.cancelLocked') }}
        </template>
      </p>
    </SectionCard>

    <SectionCard :title="t('execute.rootsTitle')" flush>
      <ul class="roots">
        <li v-for="root in rootStates" :key="root.rootId" class="root" :data-status="root.status">
          <span class="root__marker" aria-hidden="true">
            <svg v-if="root.status === 'done'" viewBox="0 0 14 14" width="10" height="10">
              <path
                d="M2 7.4 5.3 10.6 12 3.8"
                fill="none"
                stroke="currentColor"
                stroke-width="2.2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            <span v-else-if="root.status === 'active'" class="root__spinner" />
          </span>
          <span class="root__label">{{ root.label }}</span>
          <TruncatedText class="root__path" :text="root.targetPath" />
          <span class="root__size">{{ formatBytes(root.estimatedBytes, unit) }}</span>
        </li>
      </ul>
    </SectionCard>
  </div>
</template>

<style scoped>
.execute {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.headline {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  margin-bottom: var(--space-3);
}

.headline__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.headline__sub {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.headline__percent {
  font-size: var(--text-xl);
  font-weight: 640;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}

.bar {
  height: 8px;
  border-radius: var(--radius-full);
  background: var(--surface-sunken);
  border: 1px solid var(--border);
  overflow: hidden;
}

.bar__fill {
  height: 100%;
  background: var(--accent);
  transition: width var(--duration-base) var(--ease);
}

.stats {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-5);
  margin-top: var(--space-4);
}

.stat dt {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.stat dd {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 560;
  font-variant-numeric: tabular-nums;
}

.cancel-note {
  margin-top: var(--space-4);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  min-height: 1.2em;
}

.roots {
  display: flex;
  flex-direction: column;
}

.root {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border);
  font-size: var(--text-sm);
}

.root:last-child {
  border-bottom: none;
}

.root__marker {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 1.5px solid var(--border-strong);
  color: var(--text-tertiary);
}

.root[data-status='done'] .root__marker {
  border-color: var(--success);
  background: var(--success-soft);
  color: var(--success);
}

.root[data-status='active'] .root__marker {
  border-color: var(--accent);
}

.root[data-status='pending'] {
  color: var(--text-tertiary);
}

.root__spinner {
  width: 8px;
  height: 8px;
  border: 2px solid var(--accent);
  border-right-color: transparent;
  border-radius: 50%;
  animation: spin 700ms linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.root__label {
  font-weight: 540;
  width: 150px;
  flex-shrink: 0;
}

.root__path {
  flex: 1;
  min-width: 0;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.root__size {
  font-variant-numeric: tabular-nums;
  color: var(--text-secondary);
}
</style>
