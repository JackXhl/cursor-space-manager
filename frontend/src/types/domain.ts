/**
 * Mirrors the Rust types in `src-tauri/src/core/model.rs`.
 * Keep both sides in sync when either changes.
 */

export type InstallKind = 'notFound' | 'perUser' | 'machine' | 'portable' | 'unknown'

export interface RunningProcess {
  pid: number
  name: string
  exePath: string | null
}

export interface AppInstallation {
  found: boolean
  kind: InstallKind
  installDir: string | null
  exePath: string | null
  version: string | null
  running: boolean
  processes: RunningProcess[]
  conflicts: string[]
  notes: string[]
}

export type LinkState =
  | { kind: 'missing' }
  | { kind: 'regular' }
  | { kind: 'junction'; target: string }
  | { kind: 'symlink'; target: string }
  | { kind: 'unknownReparse'; target: string }

export interface DirSize {
  logicalBytes: number
  onDiskBytes: number
  fileCount: number
  dirCount: number
  errorCount: number
}

export type Recommendation = 'recommended' | 'optional' | 'notRecommended'

export interface DataRoot {
  id: string
  label: string
  purpose: string
  path: string
  resolvedPath: string
  linkState: LinkState
  exists: boolean
  recommendation: Recommendation
  reason: string
  size: DirSize | null
  volume: string | null
  scanErrors: string[]
  fromLaunchArgument: boolean
  priorCopy: string | null
}

export type DriveKind = 'fixed' | 'removable' | 'network' | 'cdRom' | 'ramDisk' | 'unknown'

export interface VolumeInfo {
  mount: string
  label: string
  filesystem: string
  totalBytes: number
  freeBytes: number
  kind: DriveKind
  isSystem: boolean
  eligibleTarget: boolean
  ineligibleReason: string | null
}

export interface TargetSuggestion {
  volume: string
  directory: string
  freeBytes: number
  requiredBytes: number
  reason: string
}

export type PriorSiteKind = 'roamingMirror' | 'dotCursor' | 'toolDefault' | 'mixed'

export type PriorDisposition = 'linked' | 'orphanCopy' | 'leftover'

export interface PriorEntry {
  path: string
  label: string
  rootId: string | null
  disposition: PriorDisposition
  size: DirSize | null
  sourcePath: string | null
}

export interface PriorSite {
  path: string
  kind: PriorSiteKind
  volume: string | null
  linkedCount: number
  orphanCount: number
  leftoverCount: number
  extraCount: number
  totalBytes: number
  entries: PriorEntry[]
  evidence: string[]
}

export type ScanPhase =
  | 'starting'
  | 'installation'
  | 'directories'
  | 'sizing'
  | 'volumes'
  | 'done'
  | 'cancelled'
  | 'failed'

export interface ScanProgress {
  scanId: string
  phase: ScanPhase
  message: string
  rootId: string | null
  size: DirSize | null
}

export interface ScanReport {
  scanId: string
  scannedAt: number
  platform: string
  installation: AppInstallation
  roots: DataRoot[]
  volumes: VolumeInfo[]
  totalLogicalBytes: number
  reclaimableBytes: number
  suggestedTarget: TargetSuggestion | null
  priorMigrations: PriorSite[]
  warnings: string[]
  durationMs: number
}

export type MigrationState =
  | 'planReady'
  | 'preflight'
  | 'waitingForCursorExit'
  | 'quiescent'
  | 'copying'
  | 'verifying'
  | 'readyToCutover'
  | 'cutoverIntentRecorded'
  | 'sourceRenamed'
  | 'linkCreated'
  | 'postCheck'
  | 'activeBackupRetained'
  | 'cleanupEligible'
  | 'completed'
  | 'cancelled'
  | 'failedSafe'
  | 'recovering'
  | 'partialCutover'
  | 'manualIntervention'
  | 'rollbackPreparing'
  | 'rolledBack'

export interface PlannedRoot {
  rootId: string
  label: string
  sourcePath: string
  targetPath: string
  backupPath: string
  stagingPath: string
  estimatedBytes: number
  estimatedFiles: number
}

export type IssueSeverity = 'blocker' | 'warning' | 'info'

export interface PreflightIssue {
  code: string
  severity: IssueSeverity
  message: string
  detail: string | null
  rootId: string | null
}

export interface MigrationPlan {
  operationId: string
  createdAt: number
  targetRoot: string
  targetVolume: string
  sourceVolume: string
  roots: PlannedRoot[]
  totalBytes: number
  totalFiles: number
  requiredTargetBytes: number
  blockers: PreflightIssue[]
  warnings: PreflightIssue[]
}

export interface MigrationProgress {
  operationId: string
  state: MigrationState
  rootId: string | null
  message: string
  processedBytes: number
  totalBytes: number
  processedFiles: number
  totalFiles: number
  bytesPerSecond: number
  etaSeconds: number | null
  cancellable: boolean
}

export interface RootOutcome {
  rootId: string
  label: string
  sourcePath: string
  targetPath: string
  backupPath: string | null
  state: MigrationState
  bytes: number
  files: number
  verified: boolean
  linkOk: boolean
  error: string | null
}

export interface MigrationResult {
  operationId: string
  state: MigrationState
  startedAt: number
  finishedAt: number
  roots: RootOutcome[]
  sourceFreeBefore: number
  sourceFreeAfter: number
  freedBytes: number
  pendingCleanupBytes: number
  backupsRetained: boolean
  messages: string[]
}

export interface HistoryEntry {
  operationId: string
  createdAt: number
  updatedAt: number
  state: MigrationState
  targetRoot: string
  rootCount: number
  totalBytes: number
  backupsRetained: boolean
  error: string | null
}

export interface TargetProbe {
  path: string
  writable: boolean
  volume: string | null
  message: string | null
}

export type RootDisposition =
  | 'untouched'
  | 'stagedOnly'
  | 'migratedWithBackup'
  | 'migratedNoBackup'
  | 'sourceRenamedOnly'
  | 'unexpectedLink'
  | 'dataMissing'

export interface RootDiagnosis {
  rootId: string
  label: string
  disposition: RootDisposition
  sourcePath: string
  targetPath: string
  backupPath: string
  sourceState: LinkState
  targetExists: boolean
  backupExists: boolean
  explanation: string
  suggestedAction: string
  safeToAutomate: boolean
}

export interface RecoveryReport {
  operationId: string
  recordedState: MigrationState
  resolvedState: MigrationState
  roots: RootDiagnosis[]
  danglingActions: string[]
  summary: string
  requiresUserDecision: boolean
}

export interface JournalEvent {
  id: number
  operationId: string
  rootId: string | null
  at: number
  kind: 'intent' | 'result'
  action: string
  detail: string | null
  ok: boolean | null
}

export interface OperationDetail {
  plan: MigrationPlan
  state: MigrationState
  roots: RootOutcome[]
  events: JournalEvent[]
}

export interface QuiescenceReport {
  quiet: boolean
  processes: RunningProcess[]
  message: string
}

export type ThemeMode = 'system' | 'light' | 'dark'
export type SizeUnit = 'auto' | 'megabytes' | 'gigabytes'
export type UpdateChannel = 'stable' | 'preview'

export interface Settings {
  schemaVersion: number
  theme: ThemeMode
  language: string
  scanOnStart: boolean
  sizeUnit: SizeUnit
  defaultTargetRoot: string | null
  excludedRootIds: string[]
  checkUpdatesAutomatically: boolean
  updateChannel: UpdateChannel
  retainBackups: boolean
  reduceMotion: boolean
}

export interface BuildInfo {
  name: string
  version: string
  buildProfile: string
  targetOs: string
  targetArch: string
  license: string
  schemaVersion: number
}

export interface StartupState {
  build: BuildInfo
  settings: Settings
  settingsMessage: string | null
  platform: string
  journalPath: string
  settingsPath: string
  recovery: RecoveryReport[]
  pendingCleanupOperations: string[]
}

export interface UpdateStatus {
  currentVersion: string
  channel: string
  updateAvailable: boolean
  latestVersion: string | null
  releaseNotes: string | null
  publishedAt: string | null
  downloadBytes: number | null
  requiresRestart: boolean
  checkedAt: number
  message: string
}

export interface UpdateProgress {
  downloaded: number
  total: number | null
  phase: 'download' | 'install'
}

export interface CommandError {
  code: string
  message: string
}
