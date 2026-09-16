<script setup lang="ts">
/** Step 2: choose what moves and where it goes. */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import RootRow from '@/components/RootRow.vue'
import DiskBar from '@/components/DiskBar.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { useAppStore } from '@/stores/app'
import { useScanStore } from '@/stores/scan'
import { useMigrationStore } from '@/stores/migration'
import { formatBytes, formatCount } from '@/utils/format'

const app = useAppStore()
const scan = useScanStore()
const migration = useMigrationStore()
const { t } = useI18n()

const unit = computed(() => app.settings.sizeUnit)

const selectableGroups = computed(() => [
  { key: 'recommended', title: t('detect.groups.recommended'), hint: t('recommend.recommendedHint'), roots: scan.recommendedRoots },
  { key: 'optional', title: t('detect.groups.optional'), hint: t('recommend.optionalHint'), roots: scan.optionalRoots },
])

const maxBytes = computed(() =>
  Math.max(0, ...scan.roots.map((root) => scan.sizeFor(root.id)?.logicalBytes ?? 0)),
)

const targetVolume = computed(() =>
  scan.volumes.find((volume) => migration.targetRoot.toUpperCase().startsWith(volume.mount.toUpperCase())),
)

/** Mirrors the Rust rule so the UI warns before the plan is even built. */
const requiredBytes = computed(() => {
  const payload = migration.selectedBytes
  return Math.max(payload * 1.15, payload + 2 * 1024 ** 3)
})

const spaceLooksSufficient = computed(() => {
  const volume = targetVolume.value
  if (!volume) return true
  const headroom = Math.max(volume.totalBytes * 0.1, 10 * 1024 ** 3)
  return volume.freeBytes >= requiredBytes.value + headroom
})

const canContinue = computed(
  () => migration.selectedRootIds.length > 0 && !!migration.targetRoot && !migration.planning,
)

async function chooseDirectory() {
  const picked = await open({
    directory: true,
    multiple: false,
    title: t('recommend.pickFolder'),
    defaultPath: migration.targetRoot || undefined,
  })
  if (typeof picked === 'string') {
    await migration.setTarget(picked)
  }
}

function useSuggestion() {
  const suggestion = scan.report?.suggestedTarget
  if (suggestion) void migration.setTarget(suggestion.directory)
}

async function continueToConfirm() {
  await migration.buildPlan()
  if (migration.plan) migration.goTo('confirm')
}
</script>

<template>
  <div class="recommend">
    <SectionCard :title="t('recommend.pickTitle')">
      <template #actions>
        <AppButton size="sm" variant="ghost" @click="migration.clearSelection()">{{ t('recommend.clearAll') }}</AppButton>
        <AppButton size="sm" @click="migration.selectRecommended()">{{ t('recommend.onlyRecommended') }}</AppButton>
      </template>

      <div class="groups">
        <div v-for="group in selectableGroups" :key="group.key" class="group">
          <div class="group__head">
            <h4>{{ group.title }}</h4>
            <span class="group__hint">{{ group.hint }}</span>
          </div>
          <div class="group__body">
            <p v-if="group.roots.length === 0" class="empty">{{ t('recommend.emptyGroup') }}</p>
            <RootRow
              v-for="root in group.roots"
              :key="root.id"
              :root="root"
              :size="scan.sizeFor(root.id)"
              :unit="unit"
              :max-bytes="maxBytes"
              selectable
              :selected="migration.selectedRootIds.includes(root.id)"
              @toggle="migration.toggleRoot"
            />
          </div>
        </div>
      </div>

      <details v-if="scan.notRecommendedRoots.length" class="excluded">
        <summary>{{ t('recommend.excluded', { n: scan.notRecommendedRoots.length }) }}</summary>
        <RootRow
          v-for="root in scan.notRecommendedRoots"
          :key="root.id"
          :root="root"
          :size="scan.sizeFor(root.id)"
          :unit="unit"
          disabled
        />
      </details>
    </SectionCard>

    <SectionCard :title="t('recommend.targetTitle')" :subtitle="t('recommend.targetSubtitle')">
      <template #actions>
        <AppButton
          v-if="scan.report?.suggestedTarget"
          size="sm"
          variant="ghost"
          @click="useSuggestion"
        >
          {{ t('recommend.useSuggested') }}
        </AppButton>
        <AppButton size="sm" @click="chooseDirectory">{{ t('common.browse') }}</AppButton>
      </template>

      <label class="field">
        <span class="field__label">{{ t('recommend.targetLabel') }}</span>
        <input
          class="field__input mono"
          type="text"
          :value="migration.targetRoot"
          :placeholder="t('recommend.placeholder')"
          spellcheck="false"
          @change="migration.setTarget(($event.target as HTMLInputElement).value)"
        />
      </label>

      <p v-if="scan.report?.suggestedTarget" class="suggestion">
        <span>{{ t('recommend.suggested') }}</span>
        <TruncatedText class="suggestion__path" :text="scan.report.suggestedTarget.directory" />
        <span>—— {{ scan.report.suggestedTarget.reason }}</span>
      </p>

      <div v-if="migration.targetProbe" class="probe" :data-ok="migration.targetProbe.writable">
        <StatusPill :tone="migration.targetProbe.writable ? 'success' : 'danger'" dot>
          {{ migration.targetProbe.writable ? t('recommend.writable') : t('recommend.notWritable') }}
        </StatusPill>
        <span v-if="migration.targetProbe.message">{{ migration.targetProbe.message }}</span>
        <span v-else>{{ t('recommend.writeOk') }}</span>
      </div>

      <div v-if="targetVolume" class="target-volume">
        <DiskBar
          :total-bytes="targetVolume.totalBytes"
          :free-bytes="targetVolume.freeBytes"
          :highlight-bytes="migration.selectedBytes"
          :highlight-label="t('recommend.writingThis')"
          :unit="unit"
        />
        <p v-if="!spaceLooksSufficient" class="target-volume__warning">
          {{ t('recommend.spaceWarning', { size: formatBytes(requiredBytes, unit) }) }}
        </p>
      </div>
    </SectionCard>

    <div class="footer">
      <div class="footer__summary">
        <span>
          {{ t('recommend.selected', { n: migration.selectedRootIds.length, size: formatBytes(migration.selectedBytes, unit), files: formatCount(migration.selectedFiles) }) }}
        </span>
        <span v-if="migration.planError" class="footer__error">{{ migration.planError }}</span>
      </div>
      <div class="footer__actions">
        <AppButton variant="ghost" @click="migration.goTo('detect')">{{ t('common.back') }}</AppButton>
        <AppButton
          variant="primary"
          :disabled="!canContinue"
          :loading="migration.planning"
          @click="continueToConfirm"
        >
          {{ t('recommend.continue') }}
        </AppButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.recommend {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.groups {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.group__head {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.group__head h4 {
  margin: 0;
  font-size: var(--text-sm);
  font-weight: 620;
}

.group__hint {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.group__body {
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.excluded {
  margin-top: var(--space-4);
  border-top: 1px solid var(--border);
  padding-top: var(--space-3);
}

.excluded summary {
  cursor: pointer;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.field__label {
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.field__input {
  height: 34px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-sunken);
  color: var(--text);
  user-select: text;
}

.field__input:focus {
  border-color: var(--accent);
}

.suggestion {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 0 6px;
  margin-top: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.suggestion__path {
  flex: 1 1 220px;
  min-width: 0;
}

.probe {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-3);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.target-volume {
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border);
}

.target-volume__warning {
  margin-top: var(--space-2);
  color: var(--warning);
  font-size: var(--text-sm);
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

.footer__summary {
  display: flex;
  flex-direction: column;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.footer__summary b {
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.footer__error {
  color: var(--danger);
  font-size: var(--text-xs);
}

.footer__actions {
  display: flex;
  gap: var(--space-2);
}

.empty {
  padding: var(--space-4);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  text-align: center;
}
</style>
