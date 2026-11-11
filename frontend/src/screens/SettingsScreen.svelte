<script lang="ts">
  import AppUpdates from '../lib/components/AppUpdates.svelte'
  import { onMount } from 'svelte'
  import { api, describeError } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { Settings } from '../lib/types'
  import DevNote from '../lib/components/DevNote.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  let settings = $state<Settings | null>(null)
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let token = $state('')

  const syncToggles: { key: 'syncAfterSession' | 'syncWhileGaming'; title: string; hint: string }[] = [
    { key: 'syncAfterSession', title: 'Отправлять сохранения в GitHub после выхода из игры', hint: 'Изменённые сохранения текущего профиля на этом ПК → репозиторий сохранений' },
    { key: 'syncWhileGaming', title: 'Отправлять сохранения в GitHub во время игры', hint: 'Изменённые сохранения текущего профиля на этом ПК → репозиторий. Запись файлов игрой может мешать отправке.' },
  ]
  const launchToggles: { key: 'askProfileBeforeLaunch' | 'autoSwitchProfile'; title: string; hint: string }[] = [
    { key: 'askProfileBeforeLaunch', title: 'Выбирать профиль перед запуском игры', hint: 'Показывать диалог выбора, если профилей больше одного' },
    { key: 'autoSwitchProfile', title: 'Выбирать профиль последнего игрока', hint: 'Запланированное автоматическое переключение сохранений; сейчас недоступно' },
  ]

  async function load() {
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
    if (!settings) return
    try {
      await api.updateSettings(settings)
      notice.show('Настройки сохранены')
    } catch (e) {
      notice.error(e)
    }
  }

  async function saveToken() {
    try {
      await api.setGithubToken(token.trim())
      token = ''
      notice.show('Токен сохранён в системном хранилище')
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  onMount(load)
</script>

<h2>Настройки</h2>

<AppUpdates />

<Loadable {loading} error={loadError}>
  {#if settings}
    <form onsubmit={save}>
      <h3>Хранилище сохранений в GitHub</h3>
      <p class="muted">Репозиторий для игровых сохранений и профилей. Обновления самого SaveHub настраиваются отдельно выше.</p>
      <label class="field">
        <span class="muted">Репозиторий для сохранений</span>
        <input bind:value={settings.repoUrl} placeholder="https://github.com/user/saves" />
      </label>
      <label class="field">
        <span class="muted">Локальная копия репозитория</span>
        <input bind:value={settings.localRepoPath} />
      </label>

      <h3>Передача сохранений между ПК</h3>
      <p class="muted">Автоматическая отправка и получение сохранений ещё не реализованы. Эти параметры сохраняются для будущего подключения синхронизации.</p>
      <ul class="list settings-list">
        {#each syncToggles as toggle (toggle.key)}
          <li class="row"><label class="grow toggle"><span><span>{toggle.title}</span><span class="muted block">{toggle.hint}</span></span><input type="checkbox" bind:checked={settings[toggle.key]} disabled /></label></li>
        {/each}
      </ul>
      <p class="muted">Проверка и получение более свежих сохранений из GitHub перед запуском игры пока недоступны. Отдельной работающей настройки для этого ещё нет.</p>
      <h3>Профиль при запуске игры</h3>
      <ul class="list settings-list">
        {#each launchToggles as toggle (toggle.key)}
          <li class="row"><label class="grow toggle"><span><span>{toggle.title}</span><span class="muted block">{toggle.hint}</span></span><input type="checkbox" bind:checked={settings[toggle.key]} disabled={toggle.key === 'autoSwitchProfile'} /></label></li>
        {/each}
      </ul>

      <div class="actions">
        <button class="primary">Сохранить настройки</button>
      </div>
    </form>

    <h3>Токен GitHub</h3>
    <p class="muted">
      {settings.hasGithubToken ? 'Токен сохранён. Новый заменит старый.' : 'Токен не задан — синхронизация не работает.'}
    </p>
    <div class="inline-form">
      <input class="grow" type="password" bind:value={token} placeholder="ghp_…" autocomplete="off" />
      <button onclick={saveToken} disabled={!token.trim()}>Сохранить токен</button>
    </div>
  {/if}
</Loadable>

<DevNote file="src/api/settings.rs" commands={['get_settings', 'update_settings', 'set_github_token']}
  notes={['RepoWorker, снапшоты и переключение сохранений пока не подключены; запись настройки не означает выполнения синхронизации.']} />
