<!-- Подсказка для разработчиков: какие ручки вызывает экран. Видна только в `tauri dev`. -->
<script lang="ts">
  let { commands, file }: { commands: string[]; file: string } = $props()
</script>

{#if import.meta.env.DEV}
  <aside class="dev-note">
    <strong>Для бэка</strong> · {file}: {#each commands as command, i (command)}{i > 0 ? ', ' : ''}<code
        >{command}</code
      >{/each}
  </aside>
{/if}

<style>
  .dev-note {
    margin-top: 24px;
    padding: 8px 12px;
    border: 1px dashed var(--border-strong);
    border-radius: 8px;
    font-size: 12px;
    color: var(--muted);
  }

  code {
    font-size: 12px;
  }
</style>
