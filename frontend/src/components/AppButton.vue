<script setup lang="ts">
withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
    size?: 'sm' | 'md'
    disabled?: boolean
    loading?: boolean
    type?: 'button' | 'submit'
  }>(),
  { variant: 'secondary', size: 'md', disabled: false, loading: false, type: 'button' },
)
</script>

<template>
  <button
    :type="type"
    class="button"
    :class="[`button--${variant}`, `button--${size}`, { 'button--loading': loading }]"
    :disabled="disabled || loading"
    :aria-busy="loading"
  >
    <span v-if="loading" class="spinner" aria-hidden="true" />
    <slot />
  </button>
</template>

<style scoped>
.button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  font-weight: 520;
  white-space: nowrap;
  transition:
    background var(--duration-fast) var(--ease),
    border-color var(--duration-fast) var(--ease),
    color var(--duration-fast) var(--ease);
}

.button--md {
  height: 34px;
  padding: 0 var(--space-4);
  font-size: var(--text-base);
}

.button--sm {
  height: 28px;
  padding: 0 var(--space-3);
  font-size: var(--text-sm);
}

.button--primary {
  background: var(--accent);
  color: #fff;
}

.button--primary:hover:not(:disabled) {
  background: var(--accent-hover);
}

.button--primary:active:not(:disabled) {
  background: var(--accent-active);
}

.button--secondary {
  background: var(--surface);
  border-color: var(--border-strong);
  color: var(--text);
}

.button--secondary:hover:not(:disabled) {
  background: var(--surface-hover);
}

.button--ghost {
  background: transparent;
  color: var(--text-secondary);
}

.button--ghost:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text);
}

.button--danger {
  background: var(--danger);
  color: #fff;
}

.button--danger:hover:not(:disabled) {
  filter: brightness(1.07);
}

.button:disabled {
  opacity: 0.45;
}

.spinner {
  width: 13px;
  height: 13px;
  border: 2px solid currentColor;
  border-right-color: transparent;
  border-radius: 50%;
  animation: spin 700ms linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
