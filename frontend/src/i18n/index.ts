import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import en from './locales/en'

export type AppLocale = 'zh-CN' | 'en'

export const i18n = createI18n({
  legacy: false as const,
  locale: 'zh-CN' as AppLocale,
  fallbackLocale: 'zh-CN' as AppLocale,
  messages: {
    'zh-CN': zhCN,
    en,
  },
})

export function normalizeLocale(value: string | undefined | null): AppLocale {
  const raw = (value ?? '').trim().toLowerCase()
  if (raw === 'en' || raw.startsWith('en-')) return 'en'
  return 'zh-CN'
}

export function applyLocale(value: string | undefined | null): AppLocale {
  const locale = normalizeLocale(value)
  i18n.global.locale.value = locale
  document.documentElement.lang = locale === 'en' ? 'en' : 'zh-CN'
  return locale
}

export function currentLocale(): AppLocale {
  return i18n.global.locale.value
}

export function localeTag(): string {
  return currentLocale() === 'en' ? 'en-US' : 'zh-CN'
}

export function t(key: string, values?: Record<string, unknown>): string {
  return String(i18n.global.t(key, values ?? {}))
}

export function te(key: string): boolean {
  return i18n.global.te(key)
}
