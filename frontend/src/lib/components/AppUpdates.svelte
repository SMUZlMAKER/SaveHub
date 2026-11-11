<script lang="ts">
  import DevNote from './DevNote.svelte'
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

<h3>Запуск SaveHub</h3>
{#if error}<p role="alert">{error}</p>{/if}
{#if preferences}
  <ul class="list settings-list">
    <li class="row"><label class="grow toggle"><span>Запускать SaveHub при входе в Windows</span><input type="checkbox" bind:checked={preferences.launchAtLogin} disabled={saving} /></label></li>
  </ul>
  <h3>Обновления SaveHub</h3>
  <p class="muted">Новые версии самого приложения. Эти проверки не синхронизируют сохранения игр.</p>
  <ul class="list settings-list">
    <li class="row"><label class="grow toggle"><span>Проверять обновления SaveHub при обычном запуске приложения</span><input type="checkbox" bind:checked={preferences.checkOnStartup} disabled={saving} /></label></li>
    <li class="row"><label class="grow toggle"><span>Проверять обновления SaveHub каждые 6 часов</span><input type="checkbox" bind:checked={preferences.checkPeriodically} disabled={saving} /></label></li>
  </ul>
  <ul class="list settings-list">
    <li class="row"><label class="grow toggle"><span><span>Проверять обновления SaveHub перед запуском игры</span><span class="muted block">По умолчанию выключено. Проверка может задержать запуск; установка выполняется отдельно.</span></span><input type="checkbox" bind:checked={preferences.checkBeforeGameLaunch} disabled={saving} /></label></li>
  </ul>
  <div class="actions">
    <button class="primary" onclick={save} disabled={saving}>{saving ? 'Сохранение…' : 'Сохранить параметры запуска'}</button>
  </div>
{/if}
{#if status}
  <p class="muted">Установлена версия {status.currentVersion}. Канал: стабильные релизы GitHub.</p>
  {#if !status.configured}
    <p>Автоматические обновления недоступны в этой сборке.</p>
  {/if}
  <p role="status">
    {#if status.phase === 'checking'}Проверяем обновления…
    {:else if status.phase === 'installing'}Загружаем и устанавливаем обновление…
    {:else if status.phase === 'restartRequired'}Обновление установлено. Перезапустите SaveHub, когда сохранения будут синхронизированы.
    {:else if status.phase === 'upToDate'}Установлена последняя версия.
    {:else if status.phase === 'error'}{status.error}
    {:else if status.version}Доступна версия {status.version}.
    {/if}
  </p>
  {#if status.lastCheckedAt}<p class="muted">Последняя проверка: {new Date(status.lastCheckedAt).toLocaleString('ru-RU')}</p>{/if}
  {#if status.notes}<details><summary>Что нового</summary><p style="white-space: pre-wrap">{status.notes}</p></details>{/if}
  <div class="actions">
    <button onclick={check} disabled={working || !status.configured}>Проверить обновления</button>
    {#if status.phase === 'restartRequired'}
      <button class="primary" onclick={install} disabled={working}>Перезапустить</button>
    {:else if status.version}<button class="primary" onclick={() => { confirming = true }} disabled={working}>Установить {status.version}</button>{/if}
  </div>
  {#if confirming}
    <p>SaveHub закроется для установки обновления и запустится снова. Продолжить?</p>
    <div class="actions"><button class="primary" onclick={install} disabled={working}>Установить и перезапустить</button><button onclick={() => { confirming = false }}>Отмена</button></div>
  {/if}
{/if}

<DevNote file="src/app/updates.rs" commands={['get_app_preferences', 'save_app_preferences', 'check_for_updates']}
  notes={status && !status.configured ? ['Не настроен публичный ключ подписи обновлений. Проверки пропускаются, запуск игры не блокируется.'] : []} />
