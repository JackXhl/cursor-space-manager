<script setup lang="ts">
/**
 * Interrupted migrations found at startup.
 *
 * The tool states what it found on disk and what it would suggest, but does not
 * act: an automatic guess about which copy of the data is authoritative is
 * exactly how a recovery turns into data loss.
 */
import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { useAppStore } from '@/stores/app'
import { useMigrationStore } from '@/stores/migration'
import { dispositionLabel, dispositionTone, journalActionLabel, migrationStateLabel, migrationStateTone } from '@/utils/labels'

const app = useAppStore()
const migration = useMigrationStore()
const { t } = useI18n()

const reports = computed(() => app.recovery)

onMounted(() => void app.refreshRecovery())
</script>

<template>
  <div class="recovery">
    <SectionCard v-if="reports.length === 0" :title="t('recovery.emptyTitle')">
      <p class="empty">{{ t('recovery.empty') }}</p>
      <AppButton size="sm" variant="ghost" @click="app.refreshRecovery()">{{ t('recovery.recheck') }}</AppButton>
    </SectionCard>

    <SectionCard
      v-for="report in reports"
      :key="report.operationId"
      :title="report.summary"
      :subtitle="t('recovery.subtitle', { recorded: migrationStateLabel(report.recordedState), resolved: migrationStateLabel(report.resolvedState) })"
    >
      <template #actions>
        <StatusPill :tone="migrationStateTone(report.resolvedState)" dot>
          {{ report.requiresUserDecision ? t('recovery.needsYou') : t('recovery.canAuto') }}
        </StatusPill>
      </template>

      <div v-if="report.danglingActions.length" class="dangling">
        <b>{{ t('recovery.stoppedAt') }}</b>
        <span v-for="action in report.danglingActions" :key="action">{{ journalActionLabel(action) }}</span>
      </div>

      <ul class="diagnoses">
        <li v-for="root in report.roots" :key="root.rootId" class="diagnosis">
          <div class="diagnosis__head">
            <span class="diagnosis__label">{{ root.label }}</span>
            <StatusPill :tone="dispositionTone(root.disposition)">
              {{ dispositionLabel(root.disposition) }}
            </StatusPill>
            <StatusPill v-if="!root.safeToAutomate" tone="warning">{{ t('recovery.decide') }}</StatusPill>
          </div>

          <p class="diagnosis__explanation">{{ root.explanation }}</p>
          <p class="diagnosis__action">{{ t('recovery.suggestion', { text: root.suggestedAction }) }}</p>

          <dl class="diagnosis__paths">
            <div>
              <dt>{{ t('recovery.source') }}</dt>
              <dd>
                <TruncatedText :text="root.sourcePath" />
              </dd>
            </div>
            <div>
              <dt>{{ t('recovery.targetCopy') }}</dt>
              <dd>
                <TruncatedText :text="root.targetPath" />
                <span class="flag" :data-present="root.targetExists">
                  {{ root.targetExists ? t('recovery.present') : t('recovery.missing') }}
                </span>
              </dd>
            </div>
            <div>
              <dt>{{ t('recovery.backup') }}</dt>
              <dd>
                <TruncatedText :text="root.backupPath" />
                <span class="flag" :data-present="root.backupExists">
                  {{ root.backupExists ? t('recovery.present') : t('recovery.missing') }}
                </span>
              </dd>
            </div>
          </dl>
        </li>
      </ul>

      <div class="actions">
        <AppButton size="sm" variant="secondary" @click="migration.undo(report.operationId)">
          {{ t('recovery.restore') }}
        </AppButton>
        <span class="actions__hint">{{ t('recovery.restoreHint') }}</span>
      </div>
    </SectionCard>
  </div>
</template>

<style scoped>
.recovery {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.dangling {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-bottom: var(--space-4);
  padding: var(--space-2) var(--space-3);
  background: var(--warning-soft);
  border: 1px solid var(--warning-border);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
}

.diagnoses {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.diagnosis {
  padding: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface-sunken);
}

.diagnosis__head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.diagnosis__label {
  font-weight: 580;
}

.diagnosis__explanation {
  margin-top: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text);
}

.diagnosis__action {
  margin-top: 2px;
  font-size: var(--text-sm);
  color: var(--accent);
}

.diagnosis__paths {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  margin-top: var(--space-3);
  padding-top: var(--space-2);
  border-top: 1px solid var(--border);
}

.diagnosis__paths > div {
  display: flex;
  gap: var(--space-3);
  min-width: 0;
}

.diagnosis__paths dt {
  width: 72px;
  flex-shrink: 0;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.diagnosis__paths dd {
  margin: 0;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.diagnosis__paths dd :deep(.truncated) {
  flex: 1;
}

.flag {
  margin-left: var(--space-2);
  font-size: var(--text-xs);
}

.flag[data-present='true'] {
  color: var(--success);
}

.flag[data-present='false'] {
  color: var(--text-tertiary);
}

.actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-top: var(--space-4);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
  flex-wrap: wrap;
}

.actions__hint {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  flex: 1;
  min-width: 200px;
}

.empty {
  color: var(--text-secondary);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
}
</style>
