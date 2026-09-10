import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, events, toCommandError } from '@/api'
import { t } from '@/i18n'
import type {
  MigrationPlan,
  MigrationProgress,
  MigrationResult,
  QuiescenceReport,
  TargetProbe,
} from '@/types/domain'
import { formatBytes } from '@/utils/format'
import { useAppStore } from './app'
import { useScanStore } from './scan'

export type WizardStep = 'detect' | 'recommend' | 'confirm' | 'execute' | 'done'

export const WIZARD_STEPS: { id: WizardStep }[] = [
  { id: 'detect' },
  { id: 'recommend' },
  { id: 'confirm' },
  { id: 'execute' },
  { id: 'done' },
]

export const useMigrationStore = defineStore('migration', () => {
  const step = ref<WizardStep>('detect')
  const selectedRootIds = ref<string[]>([])
  const targetRoot = ref('')
  const targetProbe = ref<TargetProbe | null>(null)
  const plan = ref<MigrationPlan | null>(null)
  const planning = ref(false)
  const planError = ref<string | null>(null)

  /** Blocker/warning codes the user has ticked off individually. */
  const acknowledged = ref<string[]>([])
  const quiescence = ref<QuiescenceReport | null>(null)

  const running = ref(false)
  const progress = ref<MigrationProgress | null>(null)
  const result = ref<MigrationResult | null>(null)
  const failure = ref<string | null>(null)
  const cleaningUp = ref(false)

  let unlisten: UnlistenFn[] = []

  const stepIndex = computed(() => WIZARD_STEPS.findIndex((entry) => entry.id === step.value))

  const selectedBytes = computed(() => {
    const scan = useScanStore()
    return selectedRootIds.value.reduce((total, id) => {
      return total + (scan.sizeFor(id)?.logicalBytes ?? 0)
    }, 0)
  })

  const selectedFiles = computed(() => {
    const scan = useScanStore()
    return selectedRootIds.value.reduce((total, id) => {
      return total + (scan.sizeFor(id)?.fileCount ?? 0)
    }, 0)
  })

  const hasBlockers = computed(() => (plan.value?.blockers.length ?? 0) > 0)

  const unacknowledgedWarnings = computed(
    () => plan.value?.warnings.filter((issue) => !acknowledged.value.includes(issue.code)) ?? [],
  )

  const canExecute = computed(
    () =>
      !!plan.value &&
      !hasBlockers.value &&
      unacknowledgedWarnings.value.length === 0 &&
      quiescence.value?.quiet === true &&
      !running.value,
  )

  const percentComplete = computed(() => {
    const current = progress.value
    if (!current || !current.totalBytes) return 0
    return Math.min(100, (current.processedBytes / current.totalBytes) * 100)
  })

  function toggleRoot(rootId: string) {
    selectedRootIds.value = selectedRootIds.value.includes(rootId)
      ? selectedRootIds.value.filter((id) => id !== rootId)
      : [...selectedRootIds.value, rootId]
    // Any change invalidates the preflight that produced the current plan.
    plan.value = null
    acknowledged.value = []
  }

  function selectRecommended() {
    const scan = useScanStore()
    selectedRootIds.value = scan.recommendedRoots.map((root) => root.id)
    plan.value = null
    acknowledged.value = []
  }

  function clearSelection() {
    selectedRootIds.value = []
    plan.value = null
    acknowledged.value = []
  }

  function acknowledge(code: string, value: boolean) {
    acknowledged.value = value
      ? [...new Set([...acknowledged.value, code])]
      : acknowledged.value.filter((entry) => entry !== code)
  }

  async function setTarget(path: string) {
    targetRoot.value = path
    plan.value = null
    acknowledged.value = []
    if (!path) {
      targetProbe.value = null
      return
    }
    try {
      targetProbe.value = await api.probeTargetDirectory(path)
    } catch (error) {
      targetProbe.value = null
      useAppStore().notify(toCommandError(error).message, 'warning')
    }
  }

  async function buildPlan() {
    if (selectedRootIds.value.length === 0 || !targetRoot.value) return
    planning.value = true
    planError.value = null
    try {
      plan.value = await api.createPlan(selectedRootIds.value, targetRoot.value)
      acknowledged.value = []
      await refreshQuiescence()
    } catch (error) {
      plan.value = null
      planError.value = toCommandError(error).message
    } finally {
      planning.value = false
    }
  }

  async function refreshQuiescence() {
    try {
      quiescence.value = await api.checkQuiescence()
    } catch (error) {
      quiescence.value = null
      useAppStore().notify(toCommandError(error).message, 'warning')
    }
  }

  async function attach() {
    if (unlisten.length > 0) return
    unlisten = await Promise.all([
      events.onMigrationProgress((payload) => {
        progress.value = payload
      }),
      events.onMigrationComplete((payload) => {
        running.value = false
        result.value = payload
        step.value = 'done'
        const app = useAppStore()
        if (payload.state === 'activeBackupRetained') {
          app.notify(t('result.doneToast'), 'success')
        } else {
          app.notify(t('result.needsYouToast'), 'warning')
        }
        void app.refreshRecovery()
      }),
      events.onMigrationFailed((payload) => {
        running.value = false
        failure.value = payload.message
        step.value = 'done'
        useAppStore().notify(payload.message, payload.code === 'cancelled' ? 'warning' : 'danger')
      }),
    ])
  }

  function detach() {
    unlisten.forEach((fn) => fn())
    unlisten = []
  }

  async function execute() {
    if (!plan.value || !canExecute.value) return
    running.value = true
    failure.value = null
    result.value = null
    progress.value = null
    step.value = 'execute'
    try {
      await api.startMigration(plan.value.operationId)
    } catch (error) {
      running.value = false
      failure.value = toCommandError(error).message
      // The result step is where failures are explained. Staying on the
      // progress step would leave an empty bar and no way to read the error.
      step.value = 'done'
      useAppStore().notify(failure.value, 'danger')
    }
  }

  async function cancel() {
    if (!running.value) return
    await api.cancelMigration()
  }

  async function cleanupBackups() {
    if (!result.value) return
    cleaningUp.value = true
    const app = useAppStore()
    try {
      const freed = await api.cleanupBackups(result.value.operationId)
      app.notify(t('history.cleanedToast', { size: formatBytes(freed, app.settings.sizeUnit) }), 'success')
      result.value = { ...result.value, backupsRetained: false, pendingCleanupBytes: 0 }
      await app.refreshRecovery()
    } catch (error) {
      app.notify(toCommandError(error).message, 'danger')
    } finally {
      cleaningUp.value = false
    }
  }

  async function undo(operationId: string) {
    const app = useAppStore()
    try {
      const outcome = await api.undoMigration(operationId)
      result.value = outcome
      app.notify(
        outcome.state === 'rolledBack' ? t('result.undoneToast') : t('result.undoPartialToast'),
        outcome.state === 'rolledBack' ? 'success' : 'warning',
      )
      await app.refreshRecovery()
    } catch (error) {
      app.notify(toCommandError(error).message, 'danger')
    }
  }

  function reset() {
    step.value = 'detect'
    selectedRootIds.value = []
    plan.value = null
    acknowledged.value = []
    progress.value = null
    result.value = null
    failure.value = null
    running.value = false
  }

  function goTo(next: WizardStep) {
    step.value = next
  }

  return {
    step,
    stepIndex,
    selectedRootIds,
    targetRoot,
    targetProbe,
    plan,
    planning,
    planError,
    acknowledged,
    quiescence,
    running,
    progress,
    result,
    failure,
    cleaningUp,
    selectedBytes,
    selectedFiles,
    hasBlockers,
    unacknowledgedWarnings,
    canExecute,
    percentComplete,
    toggleRoot,
    selectRecommended,
    clearSelection,
    acknowledge,
    setTarget,
    buildPlan,
    refreshQuiescence,
    attach,
    detach,
    execute,
    cancel,
    cleanupBackups,
    undo,
    reset,
    goTo,
  }
})
