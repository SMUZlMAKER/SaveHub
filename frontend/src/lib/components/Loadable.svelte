<!-- Состояния загрузки и ошибки, общие для экранов. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    loading,
    error,
    children,
  }: { loading: boolean; error: string | null; children: Snippet } = $props()
</script>

{#if loading}
  <p class="muted">Загружаем…</p>
{:else if error}
  <p class="error">{error}</p>
{:else}
  {@render children()}
{/if}
