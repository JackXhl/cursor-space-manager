import type { SizeUnit } from '@/types/domain'
import { localeTag, t } from '@/i18n'

const KB = 1024
const MB = KB * 1024
const GB = MB * 1024
const TB = GB * 1024

/**
 * Formats a byte count for display.
 *
 * `unit` forces a fixed scale so a list of directories can be compared at a
 * glance; `auto` picks the most readable one per value.
 */
export function formatBytes(bytes: number | null | undefined, unit: SizeUnit = 'auto'): string {
  if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return '—'
  if (bytes < 0) return '—'

  if (unit === 'megabytes') return `${(bytes / MB).toFixed(1)} MB`
  if (unit === 'gigabytes') return `${(bytes / GB).toFixed(2)} GB`

  if (bytes < KB) return `${bytes} B`
  if (bytes < MB) return `${(bytes / KB).toFixed(1)} KB`
  if (bytes < GB) return `${(bytes / MB).toFixed(1)} MB`
  if (bytes < TB) return `${(bytes / GB).toFixed(2)} GB`
  return `${(bytes / TB).toFixed(2)} TB`
}

/** Exact byte count, for the tooltip behind every rounded figure. */
export function formatExactBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return '—'
  return `${bytes.toLocaleString(localeTag())} ${t('common.bytes')}`
}

export function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—'
  return value.toLocaleString(localeTag())
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!bytesPerSecond) return '—'
  return `${formatBytes(bytesPerSecond)}/s`
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return '—'
  if (seconds < 60) return t('common.seconds', { n: Math.round(seconds) })
  if (seconds < 3600) {
    const minutes = Math.floor(seconds / 60)
    return t('common.minutesSeconds', { m: minutes, s: Math.round(seconds % 60) })
  }
  const hours = Math.floor(seconds / 3600)
  return t('common.hoursMinutes', { h: hours, m: Math.round((seconds % 3600) / 60) })
}

export function formatTimestamp(millis: number | null | undefined): string {
  if (!millis) return '—'
  return new Date(millis).toLocaleString(localeTag(), {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatRelativeTime(millis: number | null | undefined): string {
  if (!millis) return '—'
  const delta = Date.now() - millis
  if (delta < 60_000) return t('common.justNow')
  if (delta < 3_600_000) return t('common.minutesAgo', { n: Math.floor(delta / 60_000) })
  if (delta < 86_400_000) return t('common.hoursAgo', { n: Math.floor(delta / 3_600_000) })
  return formatTimestamp(millis)
}

/**
 * Shortens a long path from the middle, so both the volume and the leaf stay
 * visible — those are the two parts that identify a directory.
 */
export function truncatePath(path: string, max = 56): string {
  if (path.length <= max) return path
  const separator = path.includes('\\') ? '\\' : '/'
  const parts = path.split(separator)
  if (parts.length <= 2) return `${path.slice(0, max - 3)}...`
  const head = parts[0]
  const tail = parts.slice(-2).join(separator)
  return `${head}${separator}...${separator}${tail}`
}

export function percentage(value: number, total: number): number {
  if (!total) return 0
  return Math.min(100, Math.max(0, (value / total) * 100))
}
