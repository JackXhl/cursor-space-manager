<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import StatusPill from '@/components/StatusPill.vue'
import { useAppStore } from '@/stores/app'
import { useScanStore } from '@/stores/scan'
import { useMigrationStore } from '@/stores/migration'
import { formatBytes } from '@/utils/format'

const { t } = useI18n()
const app = useAppStore()
const scan = useScanStore()
const migration = useMigrationStore()
const route = useRoute()

const navigation = computed(() => [
  { to: '/', label: t('nav.migrate'), hint: t('nav.migrateHint') },
  { to: '/history', label: t('nav.history'), hint: t('nav.historyHint') },
  { to: '/recovery', label: t('nav.recovery'), hint: t('nav.recoveryHint') },
  { to: '/settings', label: t('nav.settings'), hint: t('nav.settingsHint') },
])

const pageTitle = computed(() => {
  switch (route.name) {
    case 'history':
      return t('nav.history')
    case 'recovery':
      return t('nav.recovery')
    case 'settings':
      return t('nav.settings')
    default:
      return t('nav.migrate')
  }
})

const recoveryCount = computed(() => app.recovery.length)
const reclaimable = computed(() => scan.report?.reclaimableBytes ?? 0)

watch(
  pageTitle,
  (title) => {
    document.title = `${title} · ${t('brand')}`
  },
  { immediate: true },
)

onMounted(async () => {
  await Promise.all([scan.attach(), migration.attach()])
  await app.initialise()
  // A failed startup replaces the whole view with an error, and the backend it
  // would have to talk to is the thing that just failed. Scanning anyway only
  // produces a second, more confusing error.
  if (app.startupError) return

  await scan.loadCached()
  // The window is already interactive at this point; the scan fills it in.
  if (app.settings.scanOnStart) {
    void scan.start()
  }
})

onUnmounted(() => {
  scan.detach()
  migration.detach()
})
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand" data-tauri-drag-region>
        <span class="brand__mark" aria-hidden="true">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none">
            <ellipse cx="8" cy="7" rx="6" ry="2.4" fill="currentColor" />
            <ellipse cx="8" cy="12" rx="6" ry="2.4" fill="currentColor" opacity="0.75" />
            <ellipse cx="8" cy="17" rx="6" ry="2.4" fill="currentColor" opacity="0.5" />
            <path d="M16 12h5m0 0-2.4-2.6M21 12l-2.4 2.6" stroke="currentColor" stroke-width="1.8"
              stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </span>
        <span class="brand__text">
          <b>{{ t('brand') }}</b>
          <small>v{{ app.build?.version ?? '—' }}</small>
        </span>
      </div>

      <nav class="nav" :aria-label="t('nav.aria')">
        <RouterLink v-for="item in navigation" :key="item.to" :to="item.to" class="nav__item">
          <span class="nav__label">{{ item.label }}</span>
          <span class="nav__hint">{{ item.hint }}</span>
          <span v-if="item.to === '/recovery' && recoveryCount > 0" class="nav__badge">
            {{ recoveryCount }}
          </span>
        </RouterLink>
      </nav>

      <div class="sidebar__footer">
        <div v-if="scan.scanning" class="mini-status">
          <span class="mini-status__dot" aria-hidden="true" />
          {{ t('sidebar.scanning') }}
        </div>
        <div v-else-if="reclaimable > 0" class="mini-status mini-status--idle">
          <i18n-t keypath="sidebar.reclaimable" tag="span">
            <template #size>
              <b>{{ formatBytes(reclaimable, app.settings.sizeUnit) }}</b>
            </template>
          </i18n-t>
        </div>
      </div>
    </aside>

    <div class="content">
      <header class="header" data-tauri-drag-region>
        <h1>{{ pageTitle }}</h1>
        <div class="header__status">
          <StatusPill v-if="migration.running" tone="accent" dot>{{ t('header.migrating') }}</StatusPill>
          <StatusPill v-else-if="app.needsAttention" tone="warning" dot>{{ t('header.needsAttention') }}</StatusPill>
        </div>
      </header>

      <main class="main">
        <div v-if="app.startupError" class="startup-error">
          <b>{{ t('common.initFailed') }}</b>
          <p>{{ app.startupError }}</p>
        </div>
        <RouterView v-else />
      </main>
    </div>

    <!-- Toasts: transient for info, sticky for errors the user must see. -->
    <div class="toasts" role="status" aria-live="polite">
      <div v-for="toast in app.toasts" :key="toast.id" class="toast" :data-tone="toast.tone">
        <span>{{ toast.text }}</span>
        <button class="toast__close" :aria-label="t('common.closeToast')" @click="app.dismiss(toast.id)">×</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  height: 100%;
  background: var(--bg);
}

.sidebar {
  display: flex;
  flex-direction: column;
  width: var(--sidebar-width);
  flex-shrink: 0;
  background: var(--surface-sunken);
  border-right: 1px solid var(--border);
}

.brand {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  height: var(--header-height);
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
}

.brand__mark {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  background: var(--accent);
  color: #fff;
}

.brand__text b {
  display: block;
  font-size: var(--text-sm);
  font-weight: 620;
  line-height: 1.25;
}

.brand__text small {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--space-3);
  flex: 1;
}

.nav__item {
  position: relative;
  display: block;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  text-decoration: none;
  color: var(--text-secondary);
  transition: background var(--duration-fast) var(--ease);
}

.nav__item:hover {
  background: var(--surface-hover);
}

.nav__item.router-link-active {
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text);
}

.nav__label {
  display: block;
  font-size: var(--text-sm);
  font-weight: 560;
}

.nav__hint {
  display: block;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.nav__badge {
  position: absolute;
  top: 50%;
  right: var(--space-3);
  transform: translateY(-50%);
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-full);
  background: var(--warning);
  color: #fff;
  font-size: var(--text-xs);
  font-weight: 620;
}

.sidebar__footer {
  padding: var(--space-3) var(--space-4);
  border-top: 1px solid var(--border);
  min-height: 44px;
}

.mini-status {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--accent);
  font-size: var(--text-xs);
}

.mini-status--idle {
  color: var(--text-tertiary);
}

.mini-status b {
  color: var(--text);
}

.mini-status__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  animation: pulse 1.3s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 0.3;
  }
  50% {
    opacity: 1;
  }
}

.content {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  height: var(--header-height);
  padding: 0 var(--space-5);
  border-bottom: 1px solid var(--border);
  background: var(--surface);
}

.header h1 {
  font-size: var(--text-lg);
}

.main {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-5);
}

.startup-error {
  padding: var(--space-4);
  background: var(--danger-soft);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
}

.startup-error p {
  margin-top: var(--space-1);
  font-size: var(--text-sm);
}

.toasts {
  position: fixed;
  right: var(--space-5);
  bottom: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  max-width: 380px;
  z-index: 50;
}

.toast {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid;
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-lg);
  font-size: var(--text-sm);
  background: var(--surface-raised);
  border-color: var(--border);
}

.toast[data-tone='success'] {
  background: var(--success-soft);
  border-color: var(--success-border);
}

.toast[data-tone='warning'] {
  background: var(--warning-soft);
  border-color: var(--warning-border);
}

.toast[data-tone='danger'] {
  background: var(--danger-soft);
  border-color: var(--danger-border);
}

.toast__close {
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--text-md);
  line-height: 1;
  padding: 0 2px;
}

.toast__close:hover {
  color: var(--text);
}
</style>
