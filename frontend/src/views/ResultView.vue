<script setup lang="ts">
/**
 * Step 5: what actually happened.
 *
 * The headline number is deliberately *not* "space freed", because a successful
 * migration frees almost nothing until the retained backups are removed. The
 * page shows both figures side by side so the difference is unmistakable.
 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { api, toCommandError } from '@/api'
import { useAppStore } from '@/stores/app'
import { useMigrationStore } from '@/stores/migration'
import { useScanStore } from '@/stores/scan'
import { formatBytes, formatCount } from '@/utils/format'
import { migrationStateLabel, migrationStateTone } from '@/utils/labels'

const app = useAppStore()
const migration = useMigrationStore()
const scan = useScanStore()
const { t } = useI18n()

const unit = computed(() => app.settings.sizeUnit)
const result = computed(() => migration.result)
const exporting = ref(false)
const confirmingCleanup = ref(false)

const succeededRoots = computed(
  () => result.value?.roots.filter((root) => root.state === 'activeBackupRetained') ?? [],
)
const problemRoots = computed(
  () => result.value?.roots.filter((root) => root.state !== 'activeBackupRetained') ?? [],
)

async function exportDiagnostics() {
  exporting.value = true
  try {
    const report = await api.exportDiagnostics(result.value?.operationId)
    await writeText(report)
    app.notify(t('result.copied'), 'success')
  } catch (error) {
    app.notify(toCommandError(error).message, 'danger')
  } finally {
    exporting.value = false
  }
}

async function runCleanup() {
  confirmingCleanup.value = false
  await migration.cleanupBackups()
  await scan.start()
}

function startOver() {
  migration.reset()
  void scan.start()
}
</script>

<template>
  <div v-if="result" class="result">
    <SectionCard>
      <template #actions>
        <AppButton size="sm" variant="ghost" :loading="exporting" @click="exportDiagnostics">
          {{ t('result.copyReport') }}
        </AppButton>
        <AppButton size="sm" @click="startOver">{{ t('common.rescan') }}</AppButton>
      </template>

      <div class="headline">
        <StatusPill :tone="migrationStateTone(result.state)" dot>
          {{ migrationStateLabel(result.state) }}
        </StatusPill>
        <h2>
          {{
            result.state === 'activeBackupRetained'
              ? t('result.successTitle')
              : t('result.partialTitle')
          }}
        </h2>
      </div>

      <div class="numbers">
        <div class="number">
          <span class="number__label">{{ t('result.migrated') }}</span>
          <b class="number__value">{{ formatBytes(result.roots.reduce((s, r) => s + r.bytes, 0), unit) }}</b>
          <span class="number__sub">{{ t('result.dirCount', { n: succeededRoots.length }) }}</span>
        </div>
        <div class="number">
          <span class="number__label">{{ t('result.freedNow') }}</span>
          <b class="number__value">{{ formatBytes(Math.max(0, result.freedBytes), unit) }}</b>
          <span class="number__sub">{{ t('result.backupsStill') }}</span>
        </div>
        <div class="number number--accent">
          <span class="number__label">{{ t('result.pendingFree') }}</span>
          <b class="number__value">{{ formatBytes(result.pendingCleanupBytes, unit) }}</b>
          <span class="number__sub">{{ t('result.needConfirm') }}</span>
        </div>
      </div>

      <ul v-if="result.messages.length" class="messages">
        <li v-for="message in result.messages" :key="message">{{ message }}</li>
      </ul>
    </SectionCard>

    <SectionCard v-if="result.backupsRetained" :title="t('result.nextTitle')">
      <ol class="checklist">
        <li>{{ t('result.check1') }}</li>
        <li>{{ t('result.check2') }}</li>
        <li>{{ t('result.check3') }}</li>
        <li>{{ t('result.check4') }}</li>
      </ol>

      <div class="cleanup">
        <template v-if="!confirmingCleanup">
          <AppButton variant="secondary" @click="confirmingCleanup = true">
            {{ t('result.cleanupCta') }}
          </AppButton>
          <span class="cleanup__hint">{{ t('result.cleanupHint') }}</span>
        </template>
        <template v-else>
          <p class="cleanup__warning">
            {{ t('result.cleanupWarning', { size: formatBytes(result.pendingCleanupBytes, unit) }) }}
          </p>
          <div class="cleanup__actions">
            <AppButton variant="ghost" @click="confirmingCleanup = false">{{ t('common.cancel') }}</AppButton>
            <AppButton variant="danger" :loading="migration.cleaningUp" @click="runCleanup">
              {{ t('result.confirmDelete') }}
            </AppButton>
          </div>
        </template>
      </div>
    </SectionCard>

    <SectionCard :title="t('result.perRootTitle')" flush>
      <table class="table">
        <thead>
          <tr>
            <th scope="col">{{ t('result.colDir') }}</th>
            <th scope="col">{{ t('result.colState') }}</th>
            <th scope="col">{{ t('result.colVerify') }}</th>
            <th scope="col">{{ t('result.colLink') }}</th>
            <th scope="col" class="table__number">{{ t('result.colSize') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="root in result.roots" :key="root.rootId">
            <td>
              <div class="cell-label">{{ root.label }}</div>
              <TruncatedText class="cell-path" :text="root.targetPath" />
              <div v-if="root.error" class="cell-error">{{ root.error }}</div>
            </td>
            <td>
              <StatusPill :tone="migrationStateTone(root.state)">
                {{ migrationStateLabel(root.state) }}
              </StatusPill>
            </td>
            <td>{{ root.verified ? t('result.verified') : t('result.notVerified') }}</td>
            <td>{{ root.linkOk ? t('result.linked') : t('result.notLinked') }}</td>
            <td class="table__number">
              {{ formatBytes(root.bytes, unit) }}
              <span class="cell-files">{{ t('common.files', { count: formatCount(root.files) }) }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </SectionCard>

    <SectionCard v-if="problemRoots.length" :title="t('result.needsYou')">
      <ul class="problems">
        <li v-for="root in problemRoots" :key="root.rootId">
          <b>{{ root.label }}</b>
          <span>{{ root.error ?? migrationStateLabel(root.state) }}</span>
          <TruncatedText :text="t('result.backupAt', { path: root.backupPath ?? t('result.noBackup') })" />
        </li>
      </ul>
      <AppButton variant="secondary" @click="migration.undo(result.operationId)">
        {{ t('result.undo') }}
      </AppButton>
    </SectionCard>
  </div>

  <SectionCard v-else-if="migration.failure" :title="t('result.failedTitle')">
    <p class="failure">{{ migration.failure }}</p>
    <p class="failure__note">{{ t('result.failedNote') }}</p>
    <div class="failure__actions">
      <AppButton @click="migration.goTo('recommend')">{{ t('confirm.goBack') }}</AppButton>
      <AppButton variant="ghost" @click="startOver">{{ t('common.rescan') }}</AppButton>
    </div>
  </SectionCard>

  <SectionCard v-else :title="t('result.emptyTitle')">
    <p class="empty">{{ t('result.empty') }}</p>
  </SectionCard>
</template>

<style scoped>
.result {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.headline {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  flex-wrap: wrap;
}

.numbers {
  display: flex;
  gap: var(--space-6);
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border);
}

.number {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.number__label {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.number__value {
  font-size: var(--text-xl);
  font-weight: 640;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
}

.number--accent .number__value {
  color: var(--accent);
}

.number__sub {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.messages {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  margin-top: var(--space-4);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.checklist {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  margin-left: var(--space-5);
  list-style: decimal;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.cleanup {
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border);
}

.cleanup__hint {
  display: block;
  margin-top: var(--space-2);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.cleanup__warning {
  padding: var(--space-3);
  background: var(--danger-soft);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.cleanup__actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-sm);
}

.table th,
.table td {
  padding: var(--space-3) var(--space-4);
  text-align: left;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}

.table thead th {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-weight: 540;
}

.table tbody tr:last-child td {
  border-bottom: none;
}

.table__number {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.cell-label {
  font-weight: 540;
}

.cell-path {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.cell-error {
  margin-top: 2px;
  color: var(--danger);
  font-size: var(--text-xs);
}

.cell-files {
  display: block;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.problems {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-bottom: var(--space-4);
}

.problems li {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--space-3);
  background: var(--danger-soft);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.problems .mono {
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.failure {
  color: var(--danger);
  font-size: var(--text-sm);
}

.failure__note {
  margin-top: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.failure__actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-4);
}

.empty {
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}
</style>
