<script setup lang="ts">
/** Past migrations, with the journal behind each one and an undo path. */
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { api, toCommandError } from '@/api'
import { useAppStore } from '@/stores/app'
import { useMigrationStore } from '@/stores/migration'
import type { HistoryEntry, OperationDetail } from '@/types/domain'
import { formatBytes, formatTimestamp } from '@/utils/format'
import { journalActionLabel, migrationStateLabel, migrationStateTone } from '@/utils/labels'

const app = useAppStore()
const migration = useMigrationStore()
const { t } = useI18n()

const entries = ref<HistoryEntry[]>([])
const loading = ref(true)
const expanded = ref<string | null>(null)
const detail = ref<OperationDetail | null>(null)
const detailLoading = ref(false)
const confirmingUndo = ref<string | null>(null)

const unit = computed(() => app.settings.sizeUnit)

async function load() {
  loading.value = true
  try {
    entries.value = await api.getHistory(50)
  } catch (error) {
    app.notify(toCommandError(error).message, 'danger')
  } finally {
    loading.value = false
  }
}

async function toggle(operationId: string) {
  if (expanded.value === operationId) {
    expanded.value = null
    detail.value = null
    return
  }
  expanded.value = operationId
  detailLoading.value = true
  try {
    const loaded = await api.getOperation(operationId)
    // Expanding another entry while this request was in flight must not
    // display one operation's journal under another operation's header.
    if (expanded.value !== operationId) return
    detail.value = loaded
  } catch (error) {
    if (expanded.value !== operationId) return
    app.notify(toCommandError(error).message, 'danger')
    detail.value = null
  } finally {
    if (expanded.value === operationId) {
      detailLoading.value = false
    }
  }
}

async function undo(operationId: string) {
  confirmingUndo.value = null
  await migration.undo(operationId)
  await load()
}

async function cleanup(operationId: string) {
  try {
    const freed = await api.cleanupBackups(operationId)
    app.notify(t('history.cleanedToast', { size: formatBytes(freed, unit.value) }), 'success')
    await load()
  } catch (error) {
    app.notify(toCommandError(error).message, 'danger')
  }
}

onMounted(load)
</script>

<template>
  <SectionCard :title="t('history.title')" :subtitle="t('history.subtitle')" flush>
    <template #actions>
      <AppButton size="sm" variant="ghost" @click="load">{{ t('common.refresh') }}</AppButton>
    </template>

    <p v-if="loading" class="empty">{{ t('common.loading') }}</p>
    <p v-else-if="entries.length === 0" class="empty">{{ t('history.empty') }}</p>

    <ul v-else class="entries">
      <li v-for="entry in entries" :key="entry.operationId" class="entry">
        <button class="entry__head" @click="toggle(entry.operationId)">
          <span class="entry__time">{{ formatTimestamp(entry.createdAt) }}</span>
          <StatusPill :tone="migrationStateTone(entry.state)">
            {{ migrationStateLabel(entry.state) }}
          </StatusPill>
          <TruncatedText class="entry__target" :text="entry.targetRoot" />
          <span class="entry__meta">
            {{ t('history.meta', { n: entry.rootCount, size: formatBytes(entry.totalBytes, unit) }) }}
          </span>
          <StatusPill v-if="entry.backupsRetained" tone="warning">{{ t('history.backupsKept') }}</StatusPill>
          <span class="entry__chevron" aria-hidden="true">
            {{ expanded === entry.operationId ? t('history.collapse') : t('history.expand') }}
          </span>
        </button>

        <div v-if="expanded === entry.operationId" class="entry__body">
          <p v-if="entry.error" class="entry__error">{{ entry.error }}</p>
          <p v-if="detailLoading" class="empty">{{ t('history.loadingDetail') }}</p>

          <template v-else-if="detail">
            <table class="table">
              <thead>
                <tr>
                  <th scope="col">{{ t('history.colDir') }}</th>
                  <th scope="col">{{ t('history.colState') }}</th>
                  <th scope="col">{{ t('history.colTarget') }}</th>
                  <th scope="col">{{ t('history.colBackup') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="root in detail.roots" :key="root.rootId">
                  <td>{{ root.label }}</td>
                  <td>
                    <StatusPill :tone="migrationStateTone(root.state)">
                      {{ migrationStateLabel(root.state) }}
                    </StatusPill>
                  </td>
                  <td><TruncatedText :text="root.targetPath" /></td>
                  <td><TruncatedText :text="root.backupPath ?? t('history.cleaned')" /></td>
                </tr>
              </tbody>
            </table>

            <details class="journal">
              <summary>{{ t('history.journal', { n: detail.events.length }) }}</summary>
              <ol class="journal__list">
                <li v-for="event in detail.events" :key="event.id" :data-kind="event.kind">
                  <span class="journal__time">{{ formatTimestamp(event.at) }}</span>
                  <span class="journal__kind">{{ event.kind === 'intent' ? t('history.intent') : t('history.outcome') }}</span>
                  <span class="journal__action">{{ journalActionLabel(event.action) }}</span>
                  <span v-if="event.ok !== null" class="journal__ok" :data-ok="event.ok">
                    {{ event.ok ? t('history.ok') : t('history.fail') }}
                  </span>
                </li>
              </ol>
            </details>

            <div class="entry__actions">
              <AppButton
                v-if="entry.backupsRetained"
                size="sm"
                @click="cleanup(entry.operationId)"
              >
                {{ t('history.cleanup') }}
              </AppButton>

              <template v-if="confirmingUndo === entry.operationId">
                <span class="entry__confirm">
                  {{ t('history.undoConfirm') }}
                </span>
                <AppButton size="sm" variant="ghost" @click="confirmingUndo = null">{{ t('common.cancel') }}</AppButton>
                <AppButton size="sm" variant="danger" @click="undo(entry.operationId)">
                  {{ t('history.confirmUndo') }}
                </AppButton>
              </template>
              <AppButton
                v-else
                size="sm"
                variant="ghost"
                @click="confirmingUndo = entry.operationId"
              >
                {{ t('history.undo') }}
              </AppButton>
            </div>
          </template>
        </div>
      </li>
    </ul>
  </SectionCard>
</template>

<style scoped>
.entries {
  display: flex;
  flex-direction: column;
}

.entry {
  border-bottom: 1px solid var(--border);
}

.entry:last-child {
  border-bottom: none;
}

.entry__head {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  padding: var(--space-3) var(--space-4);
  border: none;
  background: transparent;
  text-align: left;
  font-size: var(--text-sm);
}

.entry__head:hover {
  background: var(--surface-hover);
}

.entry__time {
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.entry__target {
  flex: 1;
  min-width: 0;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.entry__meta {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  flex-shrink: 0;
}

.entry__chevron {
  color: var(--accent);
  font-size: var(--text-xs);
  flex-shrink: 0;
}

.entry__body {
  padding: 0 var(--space-4) var(--space-4);
}

.entry__error {
  padding: var(--space-2) var(--space-3);
  margin-bottom: var(--space-3);
  background: var(--danger-soft);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-sm);
  table-layout: fixed;
}

.table th,
.table td {
  padding: var(--space-2);
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

.journal {
  margin-top: var(--space-3);
}

.journal summary {
  cursor: pointer;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.journal__list {
  margin-top: var(--space-2);
  max-height: 260px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
}

.journal__list li {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-1) var(--space-3);
  border-bottom: 1px solid var(--border);
  font-size: var(--text-xs);
}

.journal__list li:last-child {
  border-bottom: none;
}

.journal__list li[data-kind='intent'] {
  color: var(--text-tertiary);
}

.journal__time {
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.journal__kind {
  width: 32px;
  flex-shrink: 0;
}

.journal__action {
  flex: 1;
  min-width: 0;
}

.journal__root {
  color: var(--text-tertiary);
}

.journal__ok[data-ok='true'] {
  color: var(--success);
}

.journal__ok[data-ok='false'] {
  color: var(--danger);
}

.entry__actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-4);
  flex-wrap: wrap;
}

.entry__confirm {
  color: var(--danger);
  font-size: var(--text-sm);
}

.empty {
  padding: var(--space-6);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  text-align: center;
}
</style>
