import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { t } from '@/i18n'
import { api, events, toCommandError } from '@/api'
import type { DataRoot, DirSize, ScanPhase, ScanReport, VolumeInfo } from '@/types/domain'
import { formatDuration } from '@/utils/format'
import { useAppStore } from './app'

export const useScanStore = defineStore('scan', () => {
  const report = ref<ScanReport | null>(null)
  const scanning = ref(false)
  const phase = ref<ScanPhase>('starting')
  const statusMessage = ref('')
  const error = ref<string | null>(null)
  /** Sizes streamed in while the scan is still running. */
  const liveSizes = ref<Record<string, DirSize>>({})
  const activeRootId = ref<string | null>(null)
  const lastScanAt = ref<number | null>(null)

  let unlisten: UnlistenFn[] = []

  const roots = computed<DataRoot[]>(() => report.value?.roots ?? [])
  const volumes = computed<VolumeInfo[]>(() => report.value?.volumes ?? [])

  const recommendedRoots = computed(() =>
    roots.value.filter((root) => root.recommendation === 'recommended' && root.exists),
  )
  const optionalRoots = computed(() =>
    roots.value.filter((root) => root.recommendation === 'optional' && root.exists),
  )
  const notRecommendedRoots = computed(() =>
    roots.value.filter(
      (root) =>
        root.recommendation === 'notRecommended' &&
        root.linkState.kind !== 'junction' &&
        root.linkState.kind !== 'symlink',
    ),
  )
  const alreadyMigratedRoots = computed(() =>
    roots.value.filter(
      (root) =>
        root.id !== 'install-dir' &&
        (root.linkState.kind === 'junction' || root.linkState.kind === 'symlink'),
    ),
  )
  const priorMigrations = computed(() => report.value?.priorMigrations ?? [])

  const eligibleTargets = computed(() => volumes.value.filter((volume) => volume.eligibleTarget))

  /**
   * Bytes Cursor occupies on each volume, keyed by mount point.
   *
   * Roots are not necessarily on one disk: a partially migrated install keeps
   * some directories in place and links the rest elsewhere, so each root counts
   * against the volume that actually stores its bytes.
   */
  const bytesByVolume = computed(() => {
    const totals: Record<string, number> = {}
    for (const root of roots.value) {
      if (!root.volume || !root.size) continue
      totals[root.volume] = (totals[root.volume] ?? 0) + root.size.logicalBytes
    }
    return totals
  })

  function volumeUsage(mount: string): number {
    return bytesByVolume.value[mount] ?? 0
  }

  /** Size known so far: the finished measurement, or the streaming one. */
  function sizeFor(rootId: string): DirSize | null {
    const root = roots.value.find((item) => item.id === rootId)
    return root?.size ?? liveSizes.value[rootId] ?? null
  }

  async function attach() {
    if (unlisten.length > 0) return
    unlisten = await Promise.all([
      events.onScanProgress((payload) => {
        phase.value = payload.phase
        statusMessage.value = payload.message
        activeRootId.value = payload.rootId
        if (payload.rootId && payload.size) {
          liveSizes.value = { ...liveSizes.value, [payload.rootId]: payload.size }
        }
      }),
      events.onScanComplete((payload) => {
        report.value = payload
        scanning.value = false
        phase.value = 'done'
        activeRootId.value = null
        lastScanAt.value = payload.scannedAt
        statusMessage.value = t('scan.done', { time: formatDuration(payload.durationMs / 1000) })
      }),
      events.onScanFailed((payload) => {
        scanning.value = false
        activeRootId.value = null
        if (payload.code === 'cancelled') {
          phase.value = 'cancelled'
          statusMessage.value = t('scan.cancelled')
        } else {
          phase.value = 'failed'
          error.value = payload.message
          statusMessage.value = t('scan.failed')
          useAppStore().notify(payload.message, 'danger')
        }
      }),
    ])
  }

  function detach() {
    unlisten.forEach((fn) => fn())
    unlisten = []
  }

  async function loadCached() {
    try {
      const cached = await api.getLatestScan()
      if (cached && !report.value) {
        report.value = cached
        lastScanAt.value = cached.scannedAt
      }
    } catch {
      // A missing snapshot is not worth surfacing; the live scan follows.
    }
  }

  async function start() {
    if (scanning.value) return
    error.value = null
    liveSizes.value = {}
    scanning.value = true
    phase.value = 'starting'
    statusMessage.value = t('scan.preparing')
    try {
      await api.startScan()
    } catch (caught) {
      scanning.value = false
      const commandError = toCommandError(caught)
      error.value = commandError.message
      useAppStore().notify(commandError.message, 'danger')
    }
  }

  async function cancel() {
    if (!scanning.value) return
    await api.cancelScan()
  }

  return {
    report,
    scanning,
    phase,
    statusMessage,
    error,
    liveSizes,
    activeRootId,
    lastScanAt,
    roots,
    volumes,
    recommendedRoots,
    optionalRoots,
    notRecommendedRoots,
    alreadyMigratedRoots,
    priorMigrations,
    eligibleTargets,
    bytesByVolume,
    volumeUsage,
    sizeFor,
    attach,
    detach,
    loadCached,
    start,
    cancel,
  }
})
