<script setup lang="ts">
/**
 * Single-line (or 2-line) text that ellipsizes in place.
 *
 * Native `title` is too slow and does not wrap a 200-character path. The
 * tooltip is teleported to `body` so it is not clipped by `overflow: hidden`
 * on cards and tables, and it only appears when the text is actually truncated.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = withDefaults(
  defineProps<{
    text: string | null | undefined
    /** `path` uses a monospace face in the tooltip. */
    variant?: 'path' | 'text'
    lines?: 1 | 2
    selectable?: boolean
  }>(),
  { variant: 'path', lines: 1, selectable: true },
)

const text = computed(() => props.text ?? '')
const host = ref<HTMLElement | null>(null)
const open = ref(false)
const overflowed = ref(false)
const tipStyle = ref<Record<string, string>>({})

let showTimer = 0
let hideTimer = 0
let observer: ResizeObserver | null = null

function measure() {
  const el = host.value
  if (!el) {
    overflowed.value = false
    return
  }
  overflowed.value =
    props.lines > 1
      ? el.scrollHeight > el.clientHeight + 1
      : el.scrollWidth > el.clientWidth + 1
}

function place() {
  const el = host.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const maxWidth = Math.min(520, window.innerWidth - 24)
  let left = rect.left
  if (left + maxWidth > window.innerWidth - 12) {
    left = Math.max(12, window.innerWidth - maxWidth - 12)
  }
  const spaceBelow = window.innerHeight - rect.bottom
  const above = spaceBelow < 132 && rect.top > spaceBelow
  tipStyle.value = {
    left: `${left}px`,
    maxWidth: `${maxWidth}px`,
    ...(above
      ? { bottom: `${window.innerHeight - rect.top + 6}px`, top: 'auto' }
      : { top: `${rect.bottom + 6}px`, bottom: 'auto' }),
  }
}

function show() {
  if (!overflowed.value || !text.value) return
  window.clearTimeout(hideTimer)
  window.clearTimeout(showTimer)
  showTimer = window.setTimeout(() => {
    place()
    open.value = true
  }, 120)
}

function hide() {
  window.clearTimeout(showTimer)
  hideTimer = window.setTimeout(() => {
    open.value = false
  }, 80)
}

function onWindowChange() {
  if (open.value) place()
  measure()
}

onMounted(() => {
  void nextTick(measure)
  observer = new ResizeObserver(() => measure())
  if (host.value) observer.observe(host.value)
  window.addEventListener('resize', onWindowChange)
  window.addEventListener('scroll', onWindowChange, true)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  window.clearTimeout(showTimer)
  window.clearTimeout(hideTimer)
  window.removeEventListener('resize', onWindowChange)
  window.removeEventListener('scroll', onWindowChange, true)
})

watch(text, () => void nextTick(measure))
</script>

<template>
  <span
    class="truncated"
    :class="{
      'truncated--selectable': selectable,
      'truncated--ready': overflowed,
    }"
    :tabindex="overflowed ? 0 : undefined"
    :aria-label="overflowed ? text : undefined"
    @mouseenter="show"
    @mouseleave="hide"
    @focus="show"
    @blur="hide"
  >
    <span
      ref="host"
      class="truncated__text"
      :class="{ 'truncated__text--2': lines > 1 }"
    >{{ text || '—' }}</span>
    <Teleport to="body">
      <span
        v-if="open && overflowed"
        class="truncated__tip"
        :class="{ 'truncated__tip--mono': variant === 'path' }"
        role="tooltip"
        :style="tipStyle"
        @mouseenter="show"
        @mouseleave="hide"
      >{{ text }}</span>
    </Teleport>
  </span>
</template>

<style scoped>
.truncated {
  display: block;
  min-width: 0;
  max-width: 100%;
}

.truncated--ready {
  cursor: default;
}

.truncated--selectable .truncated__text {
  user-select: text;
}

.truncated__text {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.truncated__text--2 {
  white-space: normal;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.truncated:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
  border-radius: 2px;
}

.truncated__tip {
  position: fixed;
  z-index: 80;
  padding: 8px 10px;
  background: var(--surface-raised);
  color: var(--text);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  font-size: var(--text-xs);
  line-height: 1.45;
  overflow-wrap: anywhere;
  word-break: break-all;
  user-select: text;
  pointer-events: auto;
}

.truncated__tip--mono {
  font-family: var(--font-mono);
}
</style>
