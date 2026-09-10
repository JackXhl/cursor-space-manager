<script setup lang="ts">
/**
 * Step 1. Reached automatically on launch: the scan is already running by the
 * time this renders, so the page answers "is Cursor installed, how much is it
 * using, and where could it go" without the user clicking anything.
 */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import SectionCard from '@/components/SectionCard.vue'
import AppButton from '@/components/AppButton.vue'
import StatusPill from '@/components/StatusPill.vue'
import DiskBar from '@/components/DiskBar.vue'
import RootRow from '@/components/RootRow.vue'
import SkeletonBlock from '@/components/SkeletonBlock.vue'
import { useAppStore } from '@/stores/app'
import { useScanStore } from '@/stores/scan'
import { useMigrationStore } from '@/stores/migration'
import { formatBytes, formatCount, formatExactBytes, formatRelativeTime } from '@/utils/format'
import { driveKindLabel, installKindLabel, priorDispositionLabel, priorSiteKindLabel } from '@/utils/labels'
import TruncatedText from '@/components/TruncatedText.vue'

const app = useAppStore()
const scan = useScanStore()
const migration = useMigrationStore()
const { t } = useI18n()

const unit = computed(() => app.settings.sizeUnit)
const installation = computed(() => scan.report?.installation ?? null)
const hasReport = computed(() => scan.report !== null)

const maxBytes = computed(() =>
  Math.max(0, ...scan.roots.map((root) => scan.sizeFor(root.id)?.logicalBytes ?? 0)),
)

const totalFiles = computed(() =>
  scan.roots.reduce((sum, root) => sum + (scan.sizeFor(root.id)?.fileCount ?? 0), 0),
)

/** Roots that exist and are not already links; anything else is informational. */
const actionableRoots = computed(() =>
  scan.roots.filter((root) => root.exists && root.linkState.kind === 'regular'),
)

const groups = computed(() => [
  { key: 'already', title: t('detect.groups.already'), roots: scan.alreadyMigratedRoots },
  { key: 'recommended', title: t('detect.groups.recommended'), roots: scan.recommendedRoots },
  { key: 'optional', title: t('detect.groups.optional'), roots: scan.optionalRoots },
  { key: 'notRecommended', title: t('detect.groups.notRecommended'), roots: scan.notRecommendedRoots },
])

function proceed() {
  migration.selectRecommended()
  const suggested = scan.report?.suggestedTarget?.directory ?? app.settings.defaultTargetRoot
  if (suggested) void migration.setTarget(suggested)
  migration.goTo('recommend')
}

function usePriorSite(path: string) {
  void migration.setTarget(path)
  migration.selectRecommended()
  migration.goTo('recommend')
}
</script>

<template>
  <div class="detect">
    <!-- Live status: the scan is already underway when this page appears. -->
    <SectionCard>
      <template #actions>
        <AppButton v-if="scan.scanning" size="sm" variant="ghost" @click="scan.cancel()">
          {{ t('detect.cancelScan') }}
        </AppButton>
        <AppButton v-else size="sm" @click="scan.start()">{{ t('common.rescan') }}</AppButton>
        <AppButton
          variant="primary"
          size="sm"
          :disabled="!hasReport || actionableRoots.length === 0"
          @click="proceed"
        >
          {{ t('detect.startMigrate') }}
        </AppButton>
      </template>

      <div class="summary">
        <div class="summary__install">
          <div class="summary__row">
            <h2>{{ t('detect.installStatus') }}</h2>
            <StatusPill v-if="!hasReport" tone="neutral">{{ t('detect.detecting') }}</StatusPill>
            <StatusPill v-else-if="installation?.found" tone="success" dot>{{ t('detect.installed') }}</StatusPill>
            <StatusPill v-else tone="warning" dot>{{ t('detect.notFound') }}</StatusPill>
            <StatusPill v-if="installation?.running" tone="warning">{{ t('detect.running') }}</StatusPill>
            <StatusPill v-if="(installation?.conflicts.length ?? 0) > 0" tone="warning">
              {{ t('detect.extraInstalls', { n: installation?.conflicts.length }) }}
            </StatusPill>
          </div>

          <dl v-if="hasReport" class="facts">
            <div class="fact">
              <dt>{{ t('detect.installKind') }}</dt>
              <dd>{{ installKindLabel(installation?.kind ?? 'notFound') }}</dd>
            </div>
            <div class="fact">
              <dt>{{ t('detect.version') }}</dt>
              <dd>{{ installation?.version ?? t('detect.unknown') }}</dd>
            </div>
            <div class="fact fact--wide">
              <dt>{{ t('detect.installDir') }}</dt>
              <dd class="mono">
                <TruncatedText :text="installation?.installDir ?? t('detect.dirMissing')" />
              </dd>
            </div>
          </dl>
          <div v-else class="facts">
            <SkeletonBlock width="180px" />
            <SkeletonBlock width="120px" />
            <SkeletonBlock width="320px" />
          </div>
        </div>

        <div class="summary__metrics">
          <div class="metric">
            <span class="metric__label">{{ t('detect.totalUsage') }}</span>
            <b v-if="hasReport" class="metric__value" :title="formatExactBytes(scan.report?.totalLogicalBytes)">
              {{ formatBytes(scan.report?.totalLogicalBytes, unit) }}
            </b>
            <SkeletonBlock v-else height="24px" width="110px" />
            <span v-if="hasReport" class="metric__sub">{{ t('common.files', { count: formatCount(totalFiles) }) }}</span>
          </div>
          <div class="metric">
            <span class="metric__label">{{ t('detect.reclaimable') }}</span>
            <b v-if="hasReport" class="metric__value metric__value--accent">
              {{ formatBytes(scan.report?.reclaimableBytes, unit) }}
            </b>
            <SkeletonBlock v-else height="24px" width="110px" />
            <span v-if="hasReport" class="metric__sub">{{ t('detect.byRecommended') }}</span>
          </div>
        </div>
      </div>

      <p v-if="scan.scanning" class="progress-note" role="status">
        <span class="progress-note__dot" aria-hidden="true" />
        {{ scan.statusMessage }}
      </p>
      <p v-else-if="scan.lastScanAt" class="progress-note progress-note--idle">
        {{ t('detect.lastScan', { time: formatRelativeTime(scan.lastScanAt) }) }}
      </p>
    </SectionCard>

    <!-- Volumes: where the data lives now and where it could go. -->
    <SectionCard :title="t('detect.volumesTitle')" :subtitle="t('detect.volumesSubtitle')">
      <div v-if="scan.volumes.length === 0" class="stack">
        <SkeletonBlock height="10px" rounded />
        <SkeletonBlock height="10px" rounded />
      </div>
      <ul v-else class="volumes">
        <li v-for="volume in scan.volumes" :key="volume.mount" class="volume">
          <div class="volume__head">
            <span class="volume__mount mono">{{ volume.mount }}</span>
            <span class="volume__label">{{ volume.label || driveKindLabel(volume.kind) }}</span>
            <span v-if="volume.filesystem && volume.filesystem.toUpperCase() !== 'NTFS'" class="volume__fs">
              {{ volume.filesystem }}
            </span>
            <StatusPill v-if="volume.isSystem" tone="accent">{{ t('detect.systemDisk') }}</StatusPill>
            <StatusPill v-else-if="volume.eligibleTarget" tone="success">{{ t('detect.eligible') }}</StatusPill>
            <StatusPill v-else tone="neutral" :title="volume.ineligibleReason ?? ''">
              {{ t('detect.ineligible') }}
            </StatusPill>
          </div>
          <DiskBar
            :total-bytes="volume.totalBytes"
            :free-bytes="volume.freeBytes"
            :highlight-bytes="scan.volumeUsage(volume.mount)"
            :unit="unit"
          />
          <p v-if="volume.ineligibleReason && !volume.isSystem" class="volume__reason">
            {{ volume.ineligibleReason }}
          </p>
        </li>
      </ul>
    </SectionCard>

    <SectionCard
      v-if="scan.priorMigrations.length"
      :title="t('detect.priorsTitle')"
      :subtitle="t('detect.priorsSubtitle', { n: scan.priorMigrations.length })"
    >
      <ul class="priors">
        <li v-for="site in scan.priorMigrations" :key="site.path" class="prior">
          <div class="prior__head">
            <div>
              <p class="prior__path">
                <TruncatedText :text="site.path" />
              </p>
              <p class="prior__kind">{{ priorSiteKindLabel(site.kind) }}</p>
            </div>
            <div class="prior__metrics">
              <b>{{ formatBytes(site.totalBytes, unit) }}</b>
              <StatusPill v-if="site.linkedCount" tone="success">{{ t('detect.linkedCount', { n: site.linkedCount }) }}</StatusPill>
              <StatusPill v-if="site.orphanCount" tone="warning">{{ t('detect.orphanCount', { n: site.orphanCount }) }}</StatusPill>
            </div>
          </div>
          <ul class="prior__evidence">
            <li v-for="item in site.evidence" :key="item">{{ item }}</li>
          </ul>
          <ul class="prior__entries">
            <li v-for="entry in site.entries" :key="entry.path">
              <div class="prior__entry-main">
                <span>{{ entry.label }}</span>
                <TruncatedText class="prior__entry-path" :text="entry.path" />
              </div>
              <StatusPill :tone="entry.disposition === 'linked' ? 'success' : entry.disposition === 'orphanCopy' ? 'warning' : 'neutral'">
                {{ priorDispositionLabel(entry.disposition) }}
              </StatusPill>
              <span class="prior__entry-size">{{ formatBytes(entry.size?.logicalBytes, unit) }}</span>
            </li>
          </ul>
          <div class="prior__actions">
            <AppButton size="sm" variant="ghost" @click="usePriorSite(site.path)">
              {{ t('detect.reuseSite') }}
            </AppButton>
          </div>
        </li>
      </ul>
    </SectionCard>

    <!-- Directory detail, grouped by recommendation. -->
    <SectionCard
      v-for="group in groups"
      v-show="group.roots.length > 0 || (!hasReport && group.key === 'recommended')"
      :key="group.key"
      :title="group.title"
      :subtitle="t('common.directories', { count: group.roots.length })"
      flush
    >
      <div v-if="!hasReport && group.key === 'recommended'" class="stack stack--padded">
        <SkeletonBlock height="18px" />
        <SkeletonBlock height="18px" width="70%" />
        <SkeletonBlock height="18px" width="85%" />
      </div>
      <p v-else-if="group.roots.length === 0" class="empty">{{ t('detect.emptyGroup') }}</p>
      <RootRow
        v-for="root in group.roots"
        :key="root.id"
        :root="root"
        :size="scan.sizeFor(root.id)"
        :unit="unit"
        :max-bytes="maxBytes"
        :measuring="scan.scanning"
      />
    </SectionCard>

    <div v-if="scan.report?.warnings.length" class="warnings">
      <p v-for="warning in scan.report.warnings" :key="warning">{{ warning }}</p>
    </div>
  </div>
</template>

<style scoped>
.detect {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.summary {
  display: flex;
  gap: var(--space-6);
  align-items: flex-start;
}

.summary__install {
  flex: 1;
  min-width: 0;
}

.summary__row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.facts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2) var(--space-5);
  margin-top: var(--space-3);
}

.fact {
  min-width: 0;
}

.fact--wide {
  flex-basis: 100%;
}

.fact dt {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.fact dd {
  margin: 0;
  font-size: var(--text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.summary__metrics {
  display: flex;
  gap: var(--space-5);
  flex-shrink: 0;
}

.metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 120px;
}

.metric__label {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.metric__value {
  font-size: var(--text-xl);
  font-weight: 640;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
}

.metric__value--accent {
  color: var(--accent);
}

.metric__sub {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.progress-note {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-4);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.progress-note--idle {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.progress-note__dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
  animation: pulse 1.3s ease-in-out infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 0.35;
  }
  50% {
    opacity: 1;
  }
}

.volumes {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.volume__head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.volume__mount {
  font-weight: 620;
}

.volume__label {
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.volume__fs {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.volume__reason {
  margin-top: var(--space-1);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.stack {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.stack--padded {
  padding: var(--space-4);
}

.empty {
  padding: var(--space-5) var(--space-4);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  text-align: center;
}

.warnings {
  padding: var(--space-3) var(--space-4);
  background: var(--warning-soft);
  border: 1px solid var(--warning-border);
  border-radius: var(--radius-md);
  color: var(--text);
  font-size: var(--text-sm);
}

.priors {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.prior__head {
  display: flex;
  justify-content: space-between;
  gap: var(--space-4);
  align-items: flex-start;
}

.prior__path {
  font-weight: 600;
  min-width: 0;
}

.prior__kind {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.prior__metrics {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
  align-items: center;
  flex-shrink: 0;
}

.prior__metrics b {
  font-variant-numeric: tabular-nums;
}

.prior__evidence {
  margin-top: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.prior__evidence li {
  list-style: disc;
  margin-left: 1.1em;
}

.prior__entries {
  margin-top: var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
}

.prior__entries li {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 8px var(--space-3);
  border-bottom: 1px solid var(--border);
  font-size: var(--text-sm);
  min-width: 0;
}

.prior__entry-main {
  flex: 1;
  min-width: 0;
}

.prior__entry-path {
  margin-top: 1px;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.prior__entry-size {
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.prior__actions {
  margin-top: var(--space-3);
}
</style>
