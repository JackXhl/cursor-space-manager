<script setup lang="ts">
/** Preferences, plus the read-only build identity and update check. */
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import TruncatedText from '@/components/TruncatedText.vue'
import { useAppStore } from '@/stores/app'
import { useScanStore } from '@/stores/scan'
import { useMigrationStore } from '@/stores/migration'
import type { Settings } from '@/types/domain'
import { formatBytes, formatTimestamp, percentage } from '@/utils/format'
import { buildEditionLabel, platformLabel } from '@/utils/labels'
import { applyLocale } from '@/i18n'

const { t } = useI18n()
const app = useAppStore()
const scan = useScanStore()
const migration = useMigrationStore()

const draft = ref<Settings>({ ...app.settings })
const confirmingReset = ref(false)
const checkingUpdates = ref(false)
const unit = computed(() => app.settings.sizeUnit)
const edition = computed(() => buildEditionLabel(app.build?.buildProfile))
const platformText = computed(() => platformLabel(app.build?.targetOs))
const updatePercent = computed(() => {
  const progress = app.updateProgress
  if (!progress?.total) return 0
  return Math.round(percentage(progress.downloaded, progress.total))
})

watch(
  () => app.settings,
  (next) => {
    draft.value = { ...next }
  },
  { deep: true },
)

watch(
  () => draft.value.language,
  (language) => applyLocale(language),
)

onUnmounted(() => {
  applyLocale(app.settings.language)
})

const dirty = computed(() => JSON.stringify(draft.value) !== JSON.stringify(app.settings))

/** Every scannable root, so exclusions can be chosen from real options. */
const excludableRoots = computed(() =>
  scan.roots.map((root) => ({ id: root.id, label: root.label, path: root.path })),
)

function toggleExclusion(rootId: string, excluded: boolean) {
  draft.value.excludedRootIds = excluded
    ? [...new Set([...draft.value.excludedRootIds, rootId])]
    : draft.value.excludedRootIds.filter((id) => id !== rootId)
}

async function chooseDefaultTarget() {
  const picked = await open({ directory: true, multiple: false, title: t('settings.pickDefault') })
  if (typeof picked === 'string') draft.value.defaultTargetRoot = picked
}

async function save() {
  await app.saveSettings({ ...draft.value })
}

async function reset() {
  confirmingReset.value = false
  await app.resetSettings()
}

async function checkUpdates() {
  checkingUpdates.value = true
  await app.checkForUpdates()
  checkingUpdates.value = false
}

async function reveal(path: string) {
  try {
    await revealItemInDir(path)
  } catch {
    app.notify(t('settings.openFailed'), 'warning')
  }
}
</script>

<template>
  <div class="settings">
    <div class="settings__body">
    <SectionCard :title="t('settings.appearance')">
      <div class="fields">
        <label class="field">
          <span class="field__label">{{ t('settings.theme') }}</span>
          <select v-model="draft.theme" class="field__control">
            <option value="system">{{ t('settings.themeSystem') }}</option>
            <option value="light">{{ t('settings.themeLight') }}</option>
            <option value="dark">{{ t('settings.themeDark') }}</option>
          </select>
        </label>

        <label class="field">
          <span class="field__label">{{ t('settings.language') }}</span>
          <select v-model="draft.language" class="field__control">
            <option value="zh-CN">简体中文</option>
            <option value="en">English</option>
          </select>
        </label>

        <label class="field">
          <span class="field__label">{{ t('settings.sizeUnit') }}</span>
          <select v-model="draft.sizeUnit" class="field__control">
            <option value="auto">{{ t('settings.unitAuto') }}</option>
            <option value="megabytes">{{ t('settings.unitMb') }}</option>
            <option value="gigabytes">{{ t('settings.unitGb') }}</option>
          </select>
        </label>
      </div>

      <label class="toggle">
        <input v-model="draft.reduceMotion" type="checkbox" />
        <span>
          <b>{{ t('settings.reduceMotion') }}</b>
          <small>{{ t('settings.reduceMotionHint') }}</small>
        </span>
      </label>
    </SectionCard>

    <SectionCard :title="t('settings.scan')">
      <label class="toggle">
        <input v-model="draft.scanOnStart" type="checkbox" />
        <span>
          <b>{{ t('settings.scanOnStart') }}</b>
          <small>{{ t('settings.scanOnStartHint') }}</small>
        </span>
      </label>

      <label class="field field--wide">
        <span class="field__label">{{ t('settings.defaultTarget') }}</span>
        <div class="field__row">
          <input
            v-model="draft.defaultTargetRoot"
            class="field__control mono"
            type="text"
            :placeholder="t('settings.unset')"
            spellcheck="false"
          />
          <AppButton size="sm" @click="chooseDefaultTarget">{{ t('common.browse') }}</AppButton>
        </div>
      </label>

      <fieldset v-if="excludableRoots.length" class="exclusions">
        <legend>{{ t('settings.excludeLegend') }}</legend>
        <p class="exclusions__hint">{{ t('settings.excludeHint') }}</p>
        <div class="exclusions__grid">
          <label v-for="root in excludableRoots" :key="root.id" class="exclusion">
            <input
              type="checkbox"
              :checked="draft.excludedRootIds.includes(root.id)"
              @change="toggleExclusion(root.id, ($event.target as HTMLInputElement).checked)"
            />
            <span>
              <b>{{ root.label }}</b>
              <TruncatedText class="exclusion__path" :text="root.path" />
            </span>
          </label>
        </div>
      </fieldset>
    </SectionCard>

    <SectionCard :title="t('settings.safety')">
      <label class="toggle">
        <input v-model="draft.retainBackups" type="checkbox" />
        <span>
          <b>{{ t('settings.retainBackups') }}</b>
          <small>{{ t('settings.retainBackupsHint') }}</small>
        </span>
      </label>
      <p v-if="!draft.retainBackups" class="danger-note">
        {{ t('settings.retainOff') }}
      </p>
    </SectionCard>

    <SectionCard :title="t('settings.updates')">
      <template #actions>
        <AppButton size="sm" :loading="checkingUpdates" :disabled="app.installingUpdate" @click="checkUpdates">{{ t('settings.checkNow') }}</AppButton>
      </template>

      <label class="toggle">
        <input v-model="draft.checkUpdatesAutomatically" type="checkbox" />
        <span>
          <b>{{ t('settings.autoCheck') }}</b>
          <small>{{ t('settings.autoCheckHint') }}</small>
        </span>
      </label>

      <label class="field field--compact">
        <span class="field__label">{{ t('settings.channel') }}</span>
        <select v-model="draft.updateChannel" class="field__control">
          <option value="stable">{{ t('settings.stable') }}</option>
          <option value="preview">{{ t('settings.preview') }}</option>
        </select>
      </label>

      <div v-if="app.updateStatus" class="update">
        <StatusPill :tone="app.updateStatus.updateAvailable ? 'accent' : 'success'" dot>
          {{ app.updateStatus.updateAvailable ? t('settings.updateAvailable') : t('settings.upToDate') }}
        </StatusPill>
        <span>{{ app.updateStatus.message }}</span>
        <span class="update__time">{{ t('settings.checkedAt', { time: formatTimestamp(app.updateStatus.checkedAt) }) }}</span>
      </div>

      <div v-if="app.updateStatus?.updateAvailable" class="release">
        <dl class="release__meta">
          <div><dt>{{ t('settings.releaseVersion') }}</dt><dd>{{ app.updateStatus.latestVersion ?? '—' }}</dd></div>
          <div><dt>{{ t('settings.published') }}</dt><dd>{{ app.updateStatus.publishedAt ?? '—' }}</dd></div>
          <div>
            <dt>{{ t('settings.downloadSize') }}</dt>
            <dd>{{ app.updateStatus.downloadBytes ? formatBytes(app.updateStatus.downloadBytes, unit) : t('common.notProvided') }}</dd>
          </div>
          <div>
            <dt>{{ t('settings.afterInstall') }}</dt>
            <dd>{{ app.updateStatus.requiresRestart ? t('settings.needsRestart') : t('settings.noRestart') }}</dd>
          </div>
        </dl>
        <p v-if="app.updateStatus.releaseNotes" class="release__notes">
          {{ app.updateStatus.releaseNotes }}
        </p>
        <p class="release__note">{{ t('settings.updateNote') }}</p>
        <p v-if="migration.running" class="release__note">{{ t('settings.installBusy') }}</p>
        <div v-if="app.installingUpdate" class="update-progress">
          <div
            class="update-progress__track"
            role="progressbar"
            :aria-valuenow="updatePercent"
            aria-valuemin="0"
            aria-valuemax="100"
          >
            <div class="update-progress__bar" :style="{ width: `${app.updateProgress?.phase === 'install' ? 100 : updatePercent}%` }" />
          </div>
          <span>
            {{
              app.updateProgress?.phase === 'install'
                ? t('settings.installApplying')
                : t('settings.installProgress', { percent: updatePercent })
            }}
          </span>
        </div>
        <AppButton
          :loading="app.installingUpdate"
          :disabled="migration.running || app.build?.buildProfile === 'debug'"
          @click="app.installUpdate()"
        >
          {{ t('settings.installNow') }}
        </AppButton>
      </div>
    </SectionCard>

    <SectionCard :title="t('settings.about')">
      <div class="about-ident">
        <p class="about-ident__name">{{ app.build?.name ?? t('appName') }}</p>
        <p class="about-ident__meta">
          {{ t('settings.version', { version: app.build?.version ?? '—' }) }}
          <template v-if="edition"> · {{ edition }}</template>
          <template v-if="platformText"> · {{ platformText }}</template>
        </p>
      </div>
      <p class="about__note">{{ t('settings.aboutNote') }}</p>

      <div class="paths">
        <div class="path">
          <span class="path__label">{{ t('settings.journal') }}</span>
          <TruncatedText class="path__value" :text="app.journalPath" />
          <AppButton size="sm" variant="ghost" @click="reveal(app.journalPath)">{{ t('settings.openFolder') }}</AppButton>
        </div>
        <div class="path">
          <span class="path__label">{{ t('settings.settingsFile') }}</span>
          <TruncatedText class="path__value" :text="app.settingsPath" />
          <AppButton size="sm" variant="ghost" @click="reveal(app.settingsPath)">{{ t('settings.openFolder') }}</AppButton>
        </div>
      </div>

      <p class="about__privacy">{{ t('settings.privacy') }}</p>
    </SectionCard>
    </div>

    <div class="actions">
      <template v-if="confirmingReset">
        <span class="actions__confirm">{{ t('settings.resetConfirm') }}</span>
        <AppButton variant="ghost" @click="confirmingReset = false">{{ t('common.cancel') }}</AppButton>
        <AppButton variant="danger" @click="reset">{{ t('common.confirm') }}</AppButton>
      </template>
      <template v-else>
        <AppButton variant="ghost" @click="confirmingReset = true">{{ t('settings.reset') }}</AppButton>
        <AppButton variant="primary" :disabled="!dirty" @click="save">{{ t('settings.save') }}</AppButton>
      </template>
    </div>
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  gap: var(--space-4);
}

.settings__body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.settings__body > * {
  flex-shrink: 0;
}

.fields {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--space-4);
}

.fields > .field {
  min-width: 0;
}

.fields > .field .field__label {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  min-width: 0;
}

.fields > .field .field__control {
  width: 100%;
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-width: 180px;
}

.field--wide {
  margin-top: var(--space-4);
}

/* Standalone fields would otherwise stretch across the whole card, which reads
   as a much heavier control than the equivalent one in a row. */
.field--compact {
  margin-top: var(--space-4);
  max-width: 240px;
}

.field__label {
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.field__hint {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-weight: 400;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field__row {
  display: flex;
  gap: var(--space-2);
}

.field__control {
  height: 32px;
  padding: 0 var(--space-2);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface-sunken);
  color: var(--text);
  flex: 1;
}

.field__control:focus {
  border-color: var(--accent);
}

.toggle {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  margin-top: var(--space-4);
  cursor: pointer;
}

.toggle:first-child {
  margin-top: 0;
}

.toggle input {
  margin-top: 3px;
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  flex-shrink: 0;
}

.toggle b {
  display: block;
  font-weight: 540;
  font-size: var(--text-sm);
}

.toggle small {
  display: block;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.danger-note {
  margin-top: var(--space-3);
  padding: var(--space-2) var(--space-3);
  background: var(--danger-soft);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.exclusions {
  margin: var(--space-4) 0 0;
  padding: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
}

.exclusions legend {
  padding: 0 var(--space-1);
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.exclusions__hint {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  margin-bottom: var(--space-2);
}

.exclusions__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: var(--space-2);
}

.exclusion {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  cursor: pointer;
  min-width: 0;
}

.exclusion input {
  margin-top: 3px;
  accent-color: var(--accent);
  flex-shrink: 0;
}

/* Without this the text column keeps its full intrinsic width and the long
   paths overlap the neighbouring grid cell instead of truncating. */
.exclusion > span {
  min-width: 0;
}

.exclusion b {
  display: block;
  font-size: var(--text-sm);
  font-weight: 520;
}

.exclusion__path {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.release {
  margin-top: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface-sunken);
}

.release__meta {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: var(--space-3);
  margin: 0;
}

.release__meta dt {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.release__meta dd {
  margin: 2px 0 0;
  font-size: var(--text-sm);
}

.release__notes {
  margin: var(--space-3) 0 0;
  white-space: pre-wrap;
  font-size: var(--text-sm);
}

.release__note {
  margin: var(--space-2) 0 0;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.update-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin: var(--space-3) 0;
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.update-progress__track {
  height: 6px;
  border-radius: 999px;
  background: var(--border);
  overflow: hidden;
}

.update-progress__bar {
  height: 100%;
  border-radius: 999px;
  background: var(--accent);
  max-width: 100%;
}

.update {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-top: var(--space-4);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.update__time {
  margin-left: auto;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.about-ident__name {
  font-size: var(--text-md);
  font-weight: 600;
}

.about-ident__meta {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.about__note,
.about__privacy {
  margin-top: var(--space-3);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  line-height: 1.6;
}

.paths {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border);
}

.path {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
}

.path__label {
  width: 112px;
  flex-shrink: 0;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.path__value {
  flex: 1;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-shrink: 0;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
}

.actions__confirm {
  margin-right: auto;
  color: var(--danger);
  font-size: var(--text-sm);
}
</style>
