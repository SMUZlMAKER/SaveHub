<script lang="ts">
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

  const toggles: { key: 'syncAfterSession' | 'syncWhileGaming' | 'autoSwitchProfile'; title: string; hint: string }[] = [
    { key: 'syncAfterSession', title: 'Синхронизировать после каждой сессии', hint: 'Когда игра закрывается' },
    { key: 'syncWhileGaming', title: 'Синхронизировать во время игры', hint: 'Может мешать игре, если сохранения большие' },
    {
      key: 'autoSwitchProfile',
      title: 'Автопереключение профиля',
      hint: 'Профиль того, кто последним запускал игру',
    },
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

<Loadable {loading} error={loadError}>
  {#if settings}
    <form onsubmit={save}>
      <h3>GitHub</h3>
      <label class="field">
        <span class="muted">Репозиторий для сохранений</span>
        <input bind:value={settings.repoUrl} placeholder="https://github.com/user/saves" />
      </label>
      <label class="field">
        <span class="muted">Локальная копия репозитория</span>
        <input bind:value={settings.localRepoPath} />
      </label>

      <h3>Синхронизация</h3>
      <ul class="list">
        {#each toggles as toggle (toggle.key)}
          <li class="row">
            <label class="grow toggle">
              <span>
                <span>{toggle.title}</span>
                <span class="muted block">{toggle.hint}</span>
              </span>
              <input type="checkbox" bind:checked={settings[toggle.key]} />
            </label>
          </li>
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

<DevNote file="src/api/settings.rs" commands={['get_settings', 'update_settings', 'set_github_token']} />
