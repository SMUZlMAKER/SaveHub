<script lang="ts">
  import ManualSaveSettings from '../lib/components/ManualSaveSettings.svelte'
  import AppUpdates from '../lib/components/AppUpdates.svelte'
  import { onMount, tick } from 'svelte'
  import { api, describeError } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { AppFeatures, Settings } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  let { features, target }: { features: AppFeatures; target?: 'repository' | 'token' } = $props()
  let saving = $state(false)
  let savingToken = $state(false)
  let saveError = $state('')
  let tokenError = $state('')
  let connectError = $state('')
  let connecting = $state(false)
  let pickingRepo = $state(false)

  let settings = $state<Settings | null>(null)
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let token = $state('')

  const syncToggles: { key: 'syncAfterSession' | 'syncWhileGaming'; title: string; hint: string }[] = [
    { key: 'syncAfterSession', title: 'Автоматически отправлять сохранения, когда игры закрыты', hint: 'После фиксации контрольной точки изменения текущего профиля отправляются в репозиторий' },
    { key: 'syncWhileGaming', title: 'Отправлять готовые коммиты во время игры', hint: 'Пока игра запущена, SaveHub не фиксирует меняющиеся файлы. Можно отправить более ранние контрольные точки; изменения текущей игры попадут после её закрытия.' },
  ]
  const launchToggles: { key: 'askProfileBeforeLaunch'; title: string; hint: string }[] = [
    { key: 'askProfileBeforeLaunch', title: 'Выбирать профиль перед запуском игры', hint: 'Показывать диалог выбора, если профилей больше одного' },
  ]

  async function load() {
    loading = true
    try {
      settings = await api.getSettings()
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault()
    if (!settings || saving || savingToken) return
    saving = true; saveError = ''
    try {
      await api.updateSettings(settings)
      notice.show('Настройки сохранены. Подключение к GitHub ещё не проверено.')
    } catch (e) {
      saveError = describeError(e)
    } finally { saving = false }
  }

  async function saveToken(event: SubmitEvent) {
    event.preventDefault()
    if (!settings || !token.trim() || saving || savingToken) return
    savingToken = true; tokenError = ''
    try {
      await api.setGithubToken(token.trim())
      token = ''
      notice.show('Токен сохранён в системном хранилище')
      settings.hasGithubToken = true
    } catch (e) {
      tokenError = describeError(e)
    } finally { savingToken = false }
  }

  async function pickRepo() {
    pickingRepo = true
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ title: 'Выберите локальный репозиторий сохранений', multiple: false, directory: true })
      if (typeof selected === 'string') settings!.localRepoPath = selected
    } catch (e) {
      connectError = describeError(e)
    } finally { pickingRepo = false }
  }

  async function connectRepository() {
    if (connecting || saving || savingToken || !settings) return
    connecting = true
    connectError = ''
    try {
      await api.updateSettings(settings)
      await api.connectRepository()
      notice.show('Репозиторий сохранений подключён')
    } catch (e) {
      connectError = describeError(e)
    } finally { connecting = false }
  }

  onMount(load)
  $effect(() => {
    if (!loading && settings && target) {
      const id = target
      tick().then(() => {
        const input = document.getElementById(`settings-${id}`)
        input?.scrollIntoView({ block: 'center', behavior: 'smooth' })
        input?.focus({ preventScroll: true })
      })
    }
  })
</script>

<h2>Настройки</h2>

<AppUpdates />
<ManualSaveSettings />

{#if loadError}<button onclick={load}>Повторить загрузку настроек</button>{/if}
<Loadable {loading} error={loadError}>
  {#if settings}
    <form onsubmit={save} aria-busy={saving}>
      <fieldset disabled={saving || savingToken}>
      <h3>Хранилище сохранений в GitHub</h3>
      <p class="muted">Репозиторий для игровых сохранений и профилей. Обновления самого SaveHub настраиваются отдельно выше.</p>
      <label class="field">
        <span class="muted">Репозиторий для сохранений</span>
        <input bind:value={settings.repoUrl} placeholder="https://github.com/user/saves" />
      </label>
      <label class="field">
        <span class="muted">Локальная копия репозитория</span>
        <div class="inline-form"><input id="settings-repository" class="grow" bind:value={settings.localRepoPath} /><button type="button" onclick={pickRepo} disabled={pickingRepo || saving || savingToken}>{pickingRepo ? 'Выбор…' : 'Обзор…'}</button></div>
      </label>

      <p class="muted">Укажите абсолютный путь к существующей папке. Для GitHub пустая папка используется для клона, существующие файлы не заменяются. Без GitHub SaveHub автоматически создаст Git-репозиторий в выбранной папке, если его там ещё нет.</p>
      {#if features.sync && settings.repoUrl?.trim() && settings.localRepoPath?.trim()}
        <div class="actions"><button type="button" onclick={connectRepository} disabled={connecting || saving || savingToken || !settings.hasGithubToken}>{connecting ? 'Подключаем…' : 'Подключить репозиторий GitHub'}</button></div>
        {#if !settings.hasGithubToken}<p class="muted">Сначала сохраните токен GitHub ниже.</p>{/if}
      {/if}
      {#if connectError}<p class="error" role="alert">{connectError}</p>{/if}

      <h3>Список игр</h3>
      <label class="field">
        <span class="muted">Отдельный репозиторий списка игр</span>
        <input bind:value={settings.catalogRepoUrl} placeholder="https://github.com/user/game-catalog" />
      </label>
      <p class="muted">Создайте пустой репозиторий без README. Список обновляется автоматически; без подключения он хранится на этом ПК.</p>

      <h3>Передача сохранений между ПК</h3>
      <p class="muted">SaveHub автоматически отправляет контрольные точки и получает изменения из GitHub. Полученные сохранения применяются к профилю перед запуском игры; синхронизация сама по себе не перезаписывает файлы работающей игры.</p>
      <p class="muted">Кнопка «Синхронизировать» выполняет полный обмен для всех профилей: сохраняет локальные изменения, получает и объединяет изменения из GitHub, затем отправляет результат. Она недоступна, пока игра запущена; при конфликте сначала выберите, какие версии оставить.</p>
      <ul class="list settings-list">
        {#each syncToggles as toggle (toggle.key)}
          <li class="row"><label class="grow toggle"><span><span>{toggle.title}</span><span class="muted block">{toggle.hint}</span></span><input type="checkbox" bind:checked={settings[toggle.key]} disabled={!features.sync} /></label></li>
        {/each}
      </ul>
      <p class="muted">При запуске игры через SaveHub, пока она ещё остановлена, локальные изменения фиксируются, затем приложение получает и применяет удалённые изменения. Если данные изменились на обоих ПК, сначала разрешите конфликт.</p>
      <h3>Профиль при запуске игры</h3>
      <ul class="list settings-list">
        {#each launchToggles as toggle (toggle.key)}
          <li class="row"><label class="grow toggle"><span><span>{toggle.title}</span><span class="muted block">{toggle.hint}</span></span><input type="checkbox" bind:checked={settings[toggle.key]} /></label></li>
        {/each}
      </ul>

      <div class="actions">
        <button class="primary">{saving ? 'Сохраняем…' : 'Сохранить настройки'}</button>
      </div>
      </fieldset>
      {#if saveError}<p class="error" role="alert">{saveError} Введённые значения сохранены в форме — попробуйте ещё раз.</p>{/if}
    </form>

    <h3>Токен GitHub</h3>
    <p class="muted">
      {settings.hasGithubToken ? 'Токен сохранён. Новый заменит старый.' : 'Токен ещё не сохранён.'}
    </p>
    <form class="inline-form" onsubmit={saveToken} aria-busy={savingToken}>
      <input id="settings-token" class="grow" aria-label="Токен GitHub" disabled={saving || savingToken} type="password" bind:value={token} placeholder={settings.hasGithubToken ? '******' : 'ghp_…'} autocomplete="off" />
      <button disabled={!token.trim() || saving || savingToken}>{savingToken ? 'Сохраняем…' : 'Сохранить токен'}</button>
    </form>
    {#if tokenError}<p class="error" role="alert">{tokenError}</p>{/if}
    <p class="muted">{settings.repoUrl?.trim() ? 'Сохраните параметры репозитория и нажмите «Подключить репозиторий GitHub», чтобы проверить доступ и клонировать локальную копию.' : 'Адрес GitHub необязателен: без него профили и сохранения работают только локально.'}</p>
  {/if}
</Loadable>


<style>
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
</style>
