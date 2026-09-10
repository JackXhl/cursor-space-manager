import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  CommandError,
  HistoryEntry,
  MigrationPlan,
  MigrationProgress,
  MigrationResult,
  OperationDetail,
  QuiescenceReport,
  RecoveryReport,
  ScanProgress,
  ScanReport,
  Settings,
  StartupState,
  TargetProbe,
  UpdateProgress,
  UpdateStatus,
  VolumeInfo,
} from '@/types/domain'

export const EVENTS = {
  scanProgress: 'scan://progress',
  scanComplete: 'scan://complete',
  scanFailed: 'scan://failed',
  migrationProgress: 'migration://progress',
  migrationComplete: 'migration://complete',
  migrationFailed: 'migration://failed',
  updateProgress: 'update://progress',
} as const

/** Rust returns `{ code, message }`; anything else came from the bridge itself. */
export function toCommandError(error: unknown): CommandError {
  if (error && typeof error === 'object' && 'code' in error && 'message' in error) {
    return error as CommandError
  }
  return { code: 'unknown', message: String(error) }
}

export const api = {
  getStartupState: () => invoke<StartupState>('get_startup_state'),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<Settings>('save_settings', { settings }),
  resetSettings: () => invoke<Settings>('reset_settings'),

  startScan: () => invoke<string>('start_scan'),
  cancelScan: () => invoke<void>('cancel_scan'),
  getLatestScan: () => invoke<ScanReport | null>('get_latest_scan'),
  listVolumes: () => invoke<VolumeInfo[]>('list_volumes'),
  probeTargetDirectory: (path: string) => invoke<TargetProbe>('probe_target_directory', { path }),
  checkQuiescence: () => invoke<QuiescenceReport>('check_quiescence'),

  createPlan: (rootIds: string[], targetRoot: string) =>
    invoke<MigrationPlan>('create_plan', { request: { rootIds, targetRoot } }),
  getActivePlan: () => invoke<MigrationPlan | null>('get_active_plan'),

  startMigration: (operationId: string) => invoke<void>('start_migration', { operationId }),
  cancelMigration: () => invoke<void>('cancel_migration'),
  cleanupBackups: (operationId: string) => invoke<number>('cleanup_backups', { operationId }),
  undoMigration: (operationId: string) =>
    invoke<MigrationResult>('undo_migration', { operationId }),

  getHistory: (limit = 50) => invoke<HistoryEntry[]>('get_history', { limit }),
  getOperation: (operationId: string) => invoke<OperationDetail>('get_operation', { operationId }),
  getRecoveryReports: () => invoke<RecoveryReport[]>('get_recovery_reports'),
  diagnoseOperation: (operationId: string) =>
    invoke<RecoveryReport>('diagnose_operation', { operationId }),
  exportDiagnostics: (operationId?: string) =>
    invoke<string>('export_diagnostics', { operationId: operationId ?? null }),

  checkForUpdates: () => invoke<UpdateStatus>('check_for_updates'),
  installUpdate: () => invoke<void>('install_update'),
}

export const events = {
  onScanProgress: (handler: (payload: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>(EVENTS.scanProgress, (event) => handler(event.payload)),
  onScanComplete: (handler: (payload: ScanReport) => void): Promise<UnlistenFn> =>
    listen<ScanReport>(EVENTS.scanComplete, (event) => handler(event.payload)),
  onScanFailed: (handler: (payload: CommandError) => void): Promise<UnlistenFn> =>
    listen<CommandError>(EVENTS.scanFailed, (event) => handler(event.payload)),
  onMigrationProgress: (handler: (payload: MigrationProgress) => void): Promise<UnlistenFn> =>
    listen<MigrationProgress>(EVENTS.migrationProgress, (event) => handler(event.payload)),
  onMigrationComplete: (handler: (payload: MigrationResult) => void): Promise<UnlistenFn> =>
    listen<MigrationResult>(EVENTS.migrationComplete, (event) => handler(event.payload)),
  onMigrationFailed: (handler: (payload: CommandError) => void): Promise<UnlistenFn> =>
    listen<CommandError>(EVENTS.migrationFailed, (event) => handler(event.payload)),
  onUpdateProgress: (handler: (payload: UpdateProgress) => void): Promise<UnlistenFn> =>
    listen<UpdateProgress>(EVENTS.updateProgress, (event) => handler(event.payload)),
}
