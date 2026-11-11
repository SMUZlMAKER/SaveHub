<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../api'
  import type { LaunchStatus } from '../types'
  let status = $state<LaunchStatus | null>(null)
  let profileId = $state('')
  let busy = $state(false)
  let error = $state('')
  let generation = 0
  function accept(next: LaunchStatus) {
    if (status?.requestId !== next.requestId) { profileId = next.activeProfileId ?? ''; error = '' }
    else if (!profileId) profileId = next.activeProfileId ?? ''
    status = next
  }
  onMount(() => {
    const initial = generation
    const stop = subscribe<LaunchStatus>(events.launchStatusChanged, (next) => { generation++; accept(next) })
    api.getLaunchStatus().then(next => { if (initial === generation) accept(next) }).catch(e => error = describeError(e))
    return stop
  })
  async function confirm() {
    if (!status?.requestId || !profileId || busy) return
    busy = true; error = ''
    try { accept(await api.confirmGameLaunch(status.requestId, profileId)) }
    catch (e) { error = describeError(e) }
    finally { busy = false }
  }
  async function cancel() {
    if (!status?.requestId || busy) return
    busy = true; error = ''
    try { await api.cancelGameLaunch(status.requestId) }
    catch (e) { error = describeError(e) }
    finally { busy = false }
  }
</script>

{#if status && status.phase !== 'idle'}
  <section class="card launch-panel" aria-label="Запуск игры" aria-live="polite">
    <h3>{status.gameName ?? 'Запуск через SaveHub'}</h3>
    {#if status.phase === 'preparing'}<p role="status">Подготавливаем запуск и проверяем обновления SaveHub…</p>
    {:else if status.phase === 'awaitingProfile'}
      <label class="field"><span>Профиль для запуска</span>
        <select bind:value={profileId} disabled={busy}>
          {#each status.profiles as profile (profile.id)}
            <option value={profile.id}>{profile.name}{profile.isActive ? ' (текущий)' : ''}</option>
          {/each}
        </select>
      </label>
      <p class="muted">Переключение сохранений между профилями пока недоступно. Запуск возможен с текущим профилем.</p>
      <div class="actions"><button class="primary" onclick={confirm} disabled={busy || !profileId}>{busy ? 'Запуск…' : 'Запустить игру'}</button><button onclick={cancel} disabled={busy}>Отмена</button></div>
    {:else if status.phase === 'running'}<p role="status">Игра запущена{status.pid ? ` · PID ${status.pid}` : ''}. {status.monitorError ? 'Состояние процесса временно неизвестно.' : 'SaveHub следит за процессом.'}</p>
    {:else if status.phase === 'stopped'}<p role="status">Процесс игры завершён.</p>
    {:else if status.phase === 'cancelled'}<p role="status">Запуск отменён.</p>{/if}
    {#if status.warnings.length}<ul>{#each status.warnings as warning}<li class="muted">{warning}</li>{/each}</ul>{/if}
    {#if status.monitorError}<p class="error" role="alert">{status.monitorError}</p>{/if}
    {#if error || status.error}<p class="error" role="alert">{error || status.error}</p>{/if}
  </section>
{:else if error}<p class="error" role="alert">{error}</p>{/if}

<style>
  .launch-panel { margin: 16px 24px; padding: 16px; }
  h3 { margin-top: 0; }
  li + li { margin-top: 6px; }
</style>
