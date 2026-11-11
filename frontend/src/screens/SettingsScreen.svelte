<script lang="ts">
  import AppUpdates from '../lib/components/AppUpdates.svelte'
  import { onMount, onDestroy } from 'svelte'
  import { api, describeError } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { Settings } from '../lib/types'
  import DevNote from '../lib/components/DevNote.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  let settings = $state<Settings | null>(null)
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let token = $state('')
  let tokenVisible = $state(false)
  let tokenBusy = $state(false)
  let tokenFromStore = $state(false)
  let destroyed = false

  onDestroy(() => { destroyed = true; token = '' })

  async function toggleTokenVisibility() {
    if (tokenVisible) {
      tokenVisible = false
      if (tokenFromStore) {
        token = ''
        tokenFromStore = false
      }
      return
    }
    tokenBusy = true
    try {
      if (!token && settings?.hasGithubToken) {
        const stored = await api.getGithubToken()
        if (destroyed) return
        if (stored === null) {
          settings.hasGithubToken = false
          notice.show('Сохранённый токен не найден')
          return
        }
        token = stored
        tokenFromStore = true
      }
      tokenVisible = true
    } catch (e) {
      if (!destroyed) notice.error(e)
    } finally {
      tokenBusy = false
    }
  }

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
    tokenBusy = true
    try {
      await api.setGithubToken(token.trim())
      token = ''
      tokenVisible = false
      tokenFromStore = false
      notice.show('Токен сохранён в системном хранилище')
      await load()
    } catch (e) {
      notice.error(e)
    } finally {
      tokenBusy = false
    }
  }

  onMount(load)
</script>

<h2>Настройки</h2>

<AppUpdates />

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
      <ul class="list settings-list">
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
      <input class="grow" type={tokenVisible ? 'text' : 'password'} bind:value={token}
        oninput={() => { tokenFromStore = false }} disabled={tokenBusy}
        aria-label="Токен GitHub" placeholder={settings.hasGithubToken ? '••••••••' : 'ghp_…'}
        autocomplete="off" spellcheck={false} />
      <button type="button" onclick={toggleTokenVisibility} disabled={tokenBusy}
        aria-label={tokenVisible ? 'Скрыть токен' : 'Показать токен'}
        title={tokenVisible ? 'Скрыть токен' : 'Показать токен'} aria-pressed={tokenVisible}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
          <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" />
          <circle cx="12" cy="12" r="3" />
          {#if tokenVisible}<path d="m3 3 18 18" />{/if}
        </svg>
      </button>
      <button onclick={saveToken} disabled={tokenBusy || tokenFromStore || !token.trim()}>Сохранить токен</button>
    </div>
  {/if}
</Loadable>

<DevNote file="src/api/settings.rs" commands={['get_settings', 'update_settings', 'get_github_token', 'set_github_token']} />
