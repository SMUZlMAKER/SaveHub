<script lang="ts">
  import { onMount } from 'svelte'
  import { api, subscribe } from '../api'
  import type { ManualSaveStatus } from '../types'
  let status = $state<ManualSaveStatus | null>(null)
  onMount(() => {
    let generation = 0
    const stop = subscribe<ManualSaveStatus>('manual-save-status', value => { generation++; status = value }, () => {
      const initial = generation
      api.getManualSaveStatus().then(value => { if (initial === generation) status = value }).catch(console.error)
    })
    return stop
  })
</script>
<div class="toast" class:failed={status && !status.success} role="status">
  <strong>SaveHub · {status?.success ? 'Снимок готов' : 'Не удалось создать снимок'}</strong>
  <p>{status?.message ?? 'Проверяем результат…'}</p>
</div>
<style>
  :global(body.save-toast) { margin: 0; pointer-events: none; overflow: hidden; }
  :global(body.save-toast #app) { padding: 0; min-width: 0; }
  .toast { box-sizing: border-box; height: 130px; padding: 16px; border: 3px solid #16a34a; background: var(--surface, #fff); overflow: hidden; }
  .failed { border-color: #dc2626; }
  p { margin: 8px 0 0; font-size: 13px; }
</style>
