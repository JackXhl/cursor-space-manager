import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'wizard',
    component: () => import('@/views/WizardView.vue'),
    meta: { titleKey: 'nav.migrate' },
  },
  {
    path: '/history',
    name: 'history',
    component: () => import('@/views/HistoryView.vue'),
    meta: { titleKey: 'nav.history' },
  },
  {
    path: '/recovery',
    name: 'recovery',
    component: () => import('@/views/RecoveryView.vue'),
    meta: { titleKey: 'nav.recovery' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('@/views/SettingsView.vue'),
    meta: { titleKey: 'nav.settings' },
  },
  { path: '/:pathMatch(.*)*', redirect: '/' },
]

export const router = createRouter({
  // A packaged app is served from a custom protocol, where hash routing avoids
  // any dependency on server-side rewrites.
  history: createWebHashHistory(),
  routes,
})

/// The webview keeps its profile between launches and can restore the hash from
/// the previous session. A maintenance tool should always open on the wizard.
export async function startRouterAtWizard() {
  await router.isReady()
  if (router.currentRoute.value.name !== 'wizard') {
    await router.replace('/')
  }
}
