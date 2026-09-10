import { t, te } from '@/i18n'
import type {
  DriveKind,
  InstallKind,
  LinkState,
  MigrationState,
  PriorDisposition,
  PriorSiteKind,
  Recommendation,
  RootDisposition,
} from '@/types/domain'

export type Tone = 'neutral' | 'accent' | 'success' | 'warning' | 'danger'

export function installKindLabel(kind: InstallKind): string {
  return t(`label.installKind.${kind}`)
}

export function recommendationLabel(kind: Recommendation): string {
  return t(`label.recommendation.${kind}`)
}

export function priorSiteKindLabel(kind: PriorSiteKind): string {
  return t(`label.priorKind.${kind}`)
}

export function priorDispositionLabel(kind: PriorDisposition): string {
  return t(`label.priorDisposition.${kind}`)
}

export const recommendationTone: Record<Recommendation, Tone> = {
  recommended: 'success',
  optional: 'accent',
  notRecommended: 'neutral',
}

export function driveKindLabel(kind: DriveKind): string {
  return t(`label.driveKind.${kind}`)
}

export function linkStateLabel(state: LinkState): string {
  switch (state.kind) {
    case 'missing':
      return t('label.linkState.missing')
    case 'regular':
      return t('label.linkState.regular')
    case 'junction':
    case 'symlink':
      return t('label.linkState.linked')
    case 'unknownReparse':
      return t('label.linkState.unknownReparse')
  }
}

export function linkStateTone(state: LinkState): Tone {
  switch (state.kind) {
    case 'regular':
      return 'neutral'
    case 'junction':
    case 'symlink':
      return 'accent'
    case 'unknownReparse':
      return 'warning'
    case 'missing':
      return 'neutral'
  }
}

export function migrationStateLabel(state: MigrationState): string {
  return t(`label.migrationState.${state}`)
}

export function migrationStateTone(state: MigrationState): Tone {
  switch (state) {
    case 'completed':
    case 'activeBackupRetained':
    case 'cleanupEligible':
      return 'success'
    case 'failedSafe':
    case 'cancelled':
    case 'rolledBack':
      return 'neutral'
    case 'partialCutover':
    case 'manualIntervention':
      return 'danger'
    case 'recovering':
    case 'rollbackPreparing':
      return 'warning'
    default:
      return 'accent'
  }
}

export function dispositionLabel(disposition: RootDisposition): string {
  return t(`label.disposition.${disposition}`)
}

export function dispositionTone(disposition: RootDisposition): Tone {
  switch (disposition) {
    case 'untouched':
      return 'neutral'
    case 'stagedOnly':
      return 'accent'
    case 'migratedWithBackup':
    case 'migratedNoBackup':
      return 'success'
    case 'sourceRenamedOnly':
    case 'unexpectedLink':
      return 'warning'
    case 'dataMissing':
      return 'danger'
  }
}

export function issueHint(code: string): string {
  const key = `issueHint.${code}`
  return te(key) ? t(key) : ''
}

export function platformLabel(os?: string | null): string {
  switch ((os ?? '').toLowerCase()) {
    case 'windows':
      return 'Windows'
    case 'macos':
    case 'darwin':
      return 'macOS'
    case 'linux':
      return 'Linux'
    default:
      return os?.trim() || ''
  }
}

export function buildEditionLabel(profile?: string | null): string | null {
  return profile === 'debug' ? t('settings.devBuild') : null
}

export function journalActionLabel(action: string): string {
  const key = `label.journal.${action}`
  return te(key) ? t(key) : action
}
