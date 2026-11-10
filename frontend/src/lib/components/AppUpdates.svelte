<script lang="ts">
  import { onMount } from 'svelte'
  import { api, events, subscribe, describeError } from '../api'
  import { notice } from '../notice.svelte'
  import type { AppPreferences, UpdateStatus } from '../types'

  let preferences = $state<AppPreferences | null>(null)
  let status = $state<UpdateStatus | null>(null)
  let error = $state<string | null>(null)
  let busy = $state(false)
  let saving = $state(false)
  let confirming = $state(false)
  const working = $derived(busy || status?.phase === 'checking' || status?.phase === 'installing')

  onMount(() => {
    const stop = subscribe<UpdateStatus>(events.updateStatusChanged, (value) => { status = value })
    Promise.all([api.getAppPreferences(), api.getUpdateStatus()])
      .then(([p, s]) => { preferences = p; status = s })
      .catch((e) => { error = describeError(e) })
    return stop
  })
  async function save() {
    if (!preferences) return
    saving = true
    try { await api.saveAppPreferences(preferences); notice.show('Настройки запуска и обновлений сохранены') }
    catch (e) { notice.error(e) }
    finally { saving = false }
  }
  async function check() {
    busy = true
    try { status = await api.checkForUpdates() }
    catch (e) { notice.error(e) }
    finally { busy = false }
  }
  async function install() {
    busy = true
    confirming = false
    try { await api.installUpdate() }
    catch (e) { notice.error(e) }
    finally { busy = false }
  }
</script>

<h3>Запуск и обновления</h3>
{#if error}<p role="alert">{error}</p>{/if}
{#if preferences}
  <ul class="list">
    <li class="row"><label class="grow toggle"><span>Запускать SaveHub при входе в Windows</span><input type="checkbox" bind:checked={preferences.launchAtLogin} disabled={saving} /></label></li>
    <li class="row"><label class="grow toggle"><span>Проверять обновления при запуске</span><input type="checkbox" bind:checked={preferences.checkOnStartup} disabled={saving} /></label></li>
    <li class="row"><label class="grow toggle"><span>Проверять обновления каждые 6 часов</span><input type="checkbox" bind:checked={preferences.checkPeriodically} disabled={saving} /></label></li>
  </ul>
  <button onclick={save} disabled={saving}>{saving ? 'Сохранение…' : 'Сохранить параметры запуска'}</button>
{/if}
{#if status}
  <p class="muted">Установлена версия {status.currentVersion}. Канал: стабильные релизы GitHub.</p>
  {#if !status.configured}
    <p>В этой сборке обновления недоступны: ключ проверки подписи ещё не настроен.</p>
  {/if}
  <p role="status">
    {#if status.phase === 'checking'}Проверяем обновления…
    {:else if status.phase === 'installing'}Загружаем и устанавливаем обновление…
    {:else if status.phase === 'upToDate'}Установлена последняя версия.
    {:else if status.phase === 'error'}{status.error}
    {:else if status.version}Доступна версия {status.version}.
    {/if}
  </p>
  {#if status.lastCheckedAt}<p class="muted">Последняя проверка: {new Date(status.lastCheckedAt).toLocaleString('ru-RU')}</p>{/if}
  {#if status.notes}<details><summary>Что нового</summary><p style="white-space: pre-wrap">{status.notes}</p></details>{/if}
  <div class="actions">
    <button onclick={check} disabled={working || !status.configured}>Проверить обновления</button>
    {#if status.version}<button class="primary" onclick={() => { confirming = true }} disabled={working}>Установить {status.version}</button>{/if}
  </div>
  {#if confirming}
    <p>SaveHub закроется для установки обновления и запустится снова. Продолжить?</p>
    <div class="actions"><button class="primary" onclick={install} disabled={working}>Установить и перезапустить</button><button onclick={() => { confirming = false }}>Отмена</button></div>
  {/if}
{/if}
