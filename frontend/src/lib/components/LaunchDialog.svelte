<script lang="ts">
  import LaunchPanel from './LaunchPanel.svelte'
</script>

<div class="launcher">
  <p class="muted brand">SaveHub · запуск игры</p>
  <LaunchPanel compact />
</div>

<style>
  :global(body.launch-dialog #app) { padding: 16px; }
  .brand { margin: 0 0 12px; }
</style>
