<script setup lang="ts">
/**
 * Step 3: the last point at which nothing has been touched.
 *
 * This page has to earn the user's consent honestly, which means saying what
 * will happen to each directory, that space is not freed until backups are
 * cleaned up, and that Cursor must be closed by them rather than by us.
 */
import { computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import IssueList from '@/components/IssueList.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { useAppStore } from '@/stores/app'
import { useMigrationStore } from '@/stores/migration'
import { formatBytes, formatCount } from '@/utils/format'

const app = useAppStore()
const migration = useMigrationStore()
const { t } = useI18n()

const unit = computed(() => app.settings.sizeUnit)
const plan = computed(() => migration.plan)

let pollTimer: number | undefined

onMounted(() => {
  void migration.refreshQuiescence()
  // Users close Cursor while looking at this page, so the state must update
  // without them having to click anything.
  pollTimer = window.setInterval(() => void migration.refreshQuiescence(), 2500)
})

onUnmounted(() => {
  if (pollTimer) window.clearInterval(pollTimer)
})
</script>

<template>
  <div v-if="plan" class="confirm">
    <SectionCard :title="t('confirm.stepsTitle')" :subtitle="t('confirm.stepsSubtitle')">
      <ol class="steps">
        <li>{{ t('confirm.step1') }}</li>
        <li>{{ t('confirm.step2') }}</li>
        <li>{{ t('confirm.step3') }}</li>
        <li>{{ t('confirm.step4') }}</li>
        <li>{{ t('confirm.step5') }}</li>
      </ol>

      <div class="callout">
        <StatusPill tone="warning" dot>{{ t('confirm.note') }}</StatusPill>
        <p>{{ t('confirm.spaceNote', { size: formatBytes(plan.totalBytes, unit) }) }}</p>
      </div>
    </SectionCard>

    <SectionCard :title="t('confirm.listTitle')" flush>
      <table class="table">
        <thead>
          <tr>
            <th scope="col">{{ t('confirm.colDir') }}</th>
            <th scope="col">{{ t('confirm.colFrom') }}</th>
            <th scope="col">{{ t('confirm.colTo') }}</th>
            <th scope="col" class="table__number">{{ t('confirm.colSize') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="root in plan.roots" :key="root.rootId">
            <td>{{ root.label }}</td>
            <td><TruncatedText :text="root.sourcePath" /></td>
            <td><TruncatedText :text="root.targetPath" /></td>
            <td class="table__number">{{ formatBytes(root.estimatedBytes, unit) }}</td>
          </tr>
        </tbody>
        <tfoot>
          <tr>
            <td colspan="3">{{ t('confirm.totalFiles', { count: formatCount(plan.totalFiles) }) }}</td>
            <td class="table__number">{{ formatBytes(plan.totalBytes, unit) }}</td>
          </tr>
        </tfoot>
      </table>
    </SectionCard>

    <SectionCard v-if="plan.blockers.length" :title="t('confirm.blockersTitle')">
      <IssueList :issues="plan.blockers" />
    </SectionCard>

    <SectionCard
      v-if="plan.warnings.length"
      :title="t('confirm.warningsTitle')"
      :subtitle="t('confirm.warningsSubtitle')"
    >
      <IssueList
        :issues="plan.warnings"
        :acknowledged="migration.acknowledged"
        require-acknowledgement
        @acknowledge="migration.acknowledge"
      />
    </SectionCard>

    <SectionCard :title="t('confirm.quitTitle')">
      <div class="quiescence" :data-quiet="migration.quiescence?.quiet">
        <StatusPill :tone="migration.quiescence?.quiet ? 'success' : 'warning'" dot>
          {{ migration.quiescence?.quiet ? t('confirm.quiet') : t('confirm.stillRunning') }}
        </StatusPill>
        <p>{{ migration.quiescence?.message ?? t('confirm.checking') }}</p>
      </div>

      <ul v-if="migration.quiescence && !migration.quiescence.quiet" class="processes">
        <li v-for="process in migration.quiescence.processes" :key="process.pid">
          <span>{{ process.name }}</span>
        </li>
      </ul>

      <p class="quiescence__note">
        {{ t('confirm.quitNote') }}
      </p>
    </SectionCard>

    <div class="footer">
      <div class="footer__status">
        <span v-if="migration.hasBlockers" class="footer__blocked">
          {{ t('confirm.blockersLeft', { n: plan.blockers.length }) }}
        </span>
        <span v-else-if="migration.unacknowledgedWarnings.length" class="footer__blocked">
          {{ t('confirm.warningsLeft', { n: migration.unacknowledgedWarnings.length }) }}
        </span>
        <span v-else-if="!migration.quiescence?.quiet" class="footer__blocked">
          {{ t('confirm.quitFirst') }}
        </span>
        <span v-else class="footer__ready">{{ t('confirm.ready') }}</span>
      </div>
      <div class="footer__actions">
        <AppButton variant="ghost" @click="migration.goTo('recommend')">{{ t('common.back') }}</AppButton>
        <AppButton variant="primary" :disabled="!migration.canExecute" @click="migration.execute()">
          {{ t('confirm.start') }}
        </AppButton>
      </div>
    </div>
  </div>

  <SectionCard v-else :title="t('confirm.noPlan')">
    <p class="empty">{{ t('confirm.empty') }}</p>
    <AppButton @click="migration.goTo('recommend')">{{ t('confirm.goBack') }}</AppButton>
  </SectionCard>
</template>

<style scoped>
.confirm {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.steps {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  counter-reset: step;
  list-style: none;
}

.steps li {
  position: relative;
  padding-left: var(--space-6);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  counter-increment: step;
}

.steps li::before {
  content: counter(step);
  position: absolute;
  left: 0;
  top: 1px;
  display: grid;
  place-items: center;
  width: 19px;
  height: 19px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 620;
}

.steps b {
  color: var(--text);
}

.callout {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  margin-top: var(--space-4);
  padding: var(--space-3);
  background: var(--warning-soft);
  border: 1px solid var(--warning-border);
  border-radius: var(--radius-sm);
}

.callout p {
  font-size: var(--text-sm);
  color: var(--text);
}

.table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-sm);
  table-layout: fixed;
}

.table th,
.table td {
  padding: var(--space-2) var(--space-4);
  text-align: left;
  border-bottom: 1px solid var(--border);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.table thead th {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-weight: 540;
}

.table tfoot td {
  border-bottom: none;
  color: var(--text-secondary);
  font-weight: 560;
}

.table__number {
  text-align: right;
  font-variant-numeric: tabular-nums;
  width: 110px;
}

.quiescence {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.quiescence p {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.quiescence__note {
  margin-top: var(--space-3);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.6;
}

.processes {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.processes li {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 2px var(--space-2);
  background: var(--surface-sunken);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
}

.footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-3) var(--space-4);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}

.footer__blocked {
  color: var(--warning);
  font-size: var(--text-sm);
}

.footer__ready {
  color: var(--success);
  font-size: var(--text-sm);
}

.footer__actions {
  display: flex;
  gap: var(--space-2);
}

.empty {
  color: var(--text-secondary);
  font-size: var(--text-sm);
  margin-bottom: var(--space-3);
}
</style>
