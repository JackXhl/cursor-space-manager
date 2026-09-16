<script setup lang="ts">
withDefaults(defineProps<{ title?: string; subtitle?: string; flush?: boolean }>(), {
  title: '',
  subtitle: '',
  flush: false,
})
</script>

<template>
  <section class="card">
    <header v-if="title || $slots.actions" class="card__header">
      <div class="card__titles">
        <h3 v-if="title">{{ title }}</h3>
        <p v-if="subtitle" class="card__subtitle">{{ subtitle }}</p>
      </div>
      <div v-if="$slots.actions" class="card__actions">
        <slot name="actions" />
      </div>
    </header>
    <div class="card__body" :class="{ 'card__body--flush': flush }">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
}

.card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4);
  border-bottom: 1px solid var(--border);
}

.card__subtitle {
  margin-top: 2px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.card__actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

.card__body {
  padding: var(--space-4);
}

.card__body--flush {
  padding: 0;
}
</style>
