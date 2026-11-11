<script lang="ts">
  import LaunchPanel from './LaunchPanel.svelte'
  import { api, describeError } from '../api'
  let error = $state('')
  async function close() {
    try { await api.dismissLaunchDialog() }
    catch (e) { error = describeError(e) }
  }
</script>

<div class="launcher">
  <p class="muted brand">SaveHub · запуск игры</p>
  <LaunchPanel compact />
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="actions"><button onclick={close}>Закрыть</button></div>
</div>

<style>
  :global(body.launch-dialog #app) { padding: 16px; }
  .brand { margin: 0 0 12px; }
  .actions { margin-top: 16px; }
</style>
