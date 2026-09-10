import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { applyLocale, t } from '@/i18n'
import { api, events, toCommandError } from '@/api'
import type { BuildInfo, RecoveryReport, Settings, ThemeMode, UpdateProgress, UpdateStatus } from '@/types/domain'

const DEFAULT_SETTINGS: Settings = {
  schemaVersion: 1,
  theme: 'system',
  language: 'zh-CN',
  scanOnStart: true,
  sizeUnit: 'auto',
  defaultTargetRoot: null,
  excludedRootIds: [],
  checkUpdatesAutomatically: true,
  updateChannel: 'stable',
  retainBackups: true,
  reduceMotion: false,
}

/** Application shell: settings, theme, build identity, startup recovery. */
export const useAppStore = defineStore('app', () => {
  const ready = ref(false)
  const settings = ref<Settings>({ ...DEFAULT_SETTINGS })
  const build = ref<BuildInfo | null>(null)
  const platform = ref('')
  const journalPath = ref('')
  const settingsPath = ref('')
  const settingsMessage = ref<string | null>(null)
  const recovery = ref<RecoveryReport[]>([])
  const pendingCleanupOperations = ref<string[]>([])
  const updateStatus = ref<UpdateStatus | null>(null)
  const installingUpdate = ref(false)
  const updateProgress = ref<UpdateProgress | null>(null)
  const startupError = ref<string | null>(null)
  const toasts = ref<{ id: number; tone: 'info' | 'success' | 'warning' | 'danger'; text: string }[]>([])

  let nextToastId = 1

  const isWindows = computed(() => platform.value === 'windows')
  const needsAttention = computed(() =>
    recovery.value.some((report) => report.requiresUserDecision),
  )

  const TOAST_DISMISS_MS = 5000

  function notify(text: string, tone: 'info' | 'success' | 'warning' | 'danger' = 'info') {
    const id = nextToastId++
    toasts.value.push({ id, tone, text })
    // Errors stay until dismissed; everything else clears after a short beat.
    if (tone !== 'danger') {
      window.setTimeout(() => dismiss(id), TOAST_DISMISS_MS)
    }
  }

  function dismiss(id: number) {
    toasts.value = toasts.value.filter((toast) => toast.id !== id)
  }

  function applyTheme(mode: ThemeMode) {
    const root = document.documentElement
    if (mode === 'system') {
      root.removeAttribute('data-theme')
    } else {
      root.setAttribute('data-theme', mode)
    }
  }

  function applyMotionPreference(reduce: boolean) {
    document.documentElement.setAttribute('data-reduce-motion', String(reduce))
  }

  watch(
    () => settings.value.theme,
    (mode) => applyTheme(mode),
    { immediate: false },
  )
  watch(
    () => settings.value.language,
    (language) => applyLocale(language),
    { immediate: false },
  )
  watch(
    () => settings.value.reduceMotion,
    (reduce) => applyMotionPreference(reduce),
    { immediate: false },
  )

  async function initialise() {
    try {
      const state = await api.getStartupState()
      settings.value = state.settings
      build.value = state.build
      platform.value = state.platform
      journalPath.value = state.journalPath
      settingsPath.value = state.settingsPath
      settingsMessage.value = state.settingsMessage
      recovery.value = state.recovery
      pendingCleanupOperations.value = state.pendingCleanupOperations

      applyTheme(state.settings.theme)
      applyMotionPreference(state.settings.reduceMotion)
      applyLocale(state.settings.language)

      if (state.settingsMessage) {
        notify(state.settingsMessage, 'warning')
      }
      if (state.recovery.length > 0) {
        notify(t('toast.unfinished', { n: state.recovery.length }), 'warning')
      }
      if (state.settings.checkUpdatesAutomatically) {
        // Deliberately not awaited: an update check must never delay the scan.
        void checkForUpdates()
      }
    } catch (error) {
      startupError.value = toCommandError(error).message
    } finally {
      ready.value = true
    }
  }

  async function saveSettings(next: Settings) {
    try {
      settings.value = await api.saveSettings(next)
      applyTheme(settings.value.theme)
      applyMotionPreference(settings.value.reduceMotion)
      applyLocale(settings.value.language)
      notify(t('settings.saved'), 'success')
    } catch (error) {
      notify(toCommandError(error).message, 'danger')
    }
  }

  async function resetSettings() {
    try {
      settings.value = await api.resetSettings()
      applyTheme(settings.value.theme)
      applyMotionPreference(settings.value.reduceMotion)
      applyLocale(settings.value.language)
      notify(t('settings.resetDone'), 'success')
    } catch (error) {
      notify(toCommandError(error).message, 'danger')
    }
  }

  async function checkForUpdates() {
    try {
      updateStatus.value = await api.checkForUpdates()
    } catch (error) {
      updateStatus.value = null
      notify(toCommandError(error).message, 'warning')
    }
  }

  async function installUpdate() {
    if (installingUpdate.value) return
    installingUpdate.value = true
    updateProgress.value = { downloaded: 0, total: null, phase: 'download' }
    const stop = await events.onUpdateProgress((payload) => {
      updateProgress.value = payload
    })
    try {
      await api.installUpdate()
      notify(t('settings.installRestarting'), 'success')
    } catch (error) {
      notify(toCommandError(error).message, 'danger')
      installingUpdate.value = false
      updateProgress.value = null
    } finally {
      stop()
    }
  }

  async function refreshRecovery() {
    try {
      recovery.value = await api.getRecoveryReports()
    } catch (error) {
      notify(toCommandError(error).message, 'warning')
    }
  }

  return {
    ready,
    settings,
    build,
    platform,
    journalPath,
    settingsPath,
    settingsMessage,
    recovery,
    pendingCleanupOperations,
    updateStatus,
    installingUpdate,
    updateProgress,
    startupError,
    toasts,
    isWindows,
    needsAttention,
    notify,
    dismiss,
    initialise,
    saveSettings,
    resetSettings,
    checkForUpdates,
    installUpdate,
    refreshRecovery,
  }
})
