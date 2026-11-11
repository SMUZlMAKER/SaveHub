<script lang="ts">
  import AppUpdates from '../lib/components/AppUpdates.svelte'
  import { onMount } from 'svelte'
  import { api, describeError } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { AppFeatures, Settings } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  let { features }: { features: AppFeatures } = $props()
  let saving = $state(false)
  let savingToken = $state(false)
  let saveError = $state('')
  let tokenError = $state('')

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

  onMount(load)
</script>

<h2>Настройки</h2>

<AppUpdates />

{#if loadError}<button onclick={load}>Повторить загрузку настроек</button>{/if}
<Loadable {loading} error={loadError}>
  {#if settings}
    <form onsubmit={save} aria-busy={saving}>
      <fieldset disabled={saving || savingToken}>
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
      {#if !features.sync}<p class="availability-note">Синхронизация пока недоступна. Адрес репозитория и токен можно сохранить заранее.</p>{/if}
      {#if !features.profileSwitch}<p class="muted">Автопереключение профиля пока недоступно.</p>{/if}
      <ul class="list settings-list">
        {#each toggles as toggle (toggle.key)}
          <li class="row">
            <label class="grow toggle">
              <span>
                <span>{toggle.title}</span>
                <span class="muted block">{toggle.hint}</span>
              </span>
              <input type="checkbox" disabled={toggle.key === 'autoSwitchProfile' ? !features.profileSwitch : !features.sync} bind:checked={settings[toggle.key]} />
            </label>
          </li>
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
      <input class="grow" aria-label="Токен GitHub" disabled={saving || savingToken} type="password" bind:value={token} placeholder="ghp_…" autocomplete="off" />
      <button disabled={!token.trim() || saving || savingToken}>{savingToken ? 'Сохраняем…' : 'Сохранить токен'}</button>
    </form>
    {#if tokenError}<p class="error" role="alert">{tokenError}</p>{/if}
    <p class="muted">Сохранение токена не подтверждает доступ к репозиторию.</p>
  {/if}
</Loadable>


<style>
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
</style>
