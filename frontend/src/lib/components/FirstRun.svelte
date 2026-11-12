<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError } from '../api'
  import type { AppFeatures, Settings } from '../types'

  let { features, gamesCount, gamesLoading, gamesError, onsettings, onadd, adding }:
    { features: AppFeatures; gamesCount: number; gamesLoading: boolean; gamesError: string;
      onsettings: () => void; onadd: () => void; adding: boolean } = $props()
  let settings = $state<Settings | null>(null)
  let loading = $state(true)
  let error = $state('')
  const repositorySet = $derived(!!settings?.localRepoPath?.trim())
  const remoteConfigured = $derived(!!settings?.repoUrl?.trim())
  const hasGames = $derived(!gamesLoading && !gamesError && gamesCount > 0)
  const complete = $derived(repositorySet && (!remoteConfigured || settings?.hasGithubToken) && hasGames)

  async function load() {
    loading = true; error = ''
    try { settings = await api.getSettings() }
    catch (e) { error = describeError(e) }
    finally { loading = false }
  }
  onMount(load)
</script>

<section class="card first-run" aria-labelledby="first-run-title">
  <h3 id="first-run-title">{complete ? 'Параметры заполнены' : 'Начните с настройки'}</h3>
  {#if loading}
    <p class="muted" role="status">Проверяем настройки…</p>
  {:else if error}
    <p class="error" role="alert">Не удалось прочитать настройки: {error}</p>
    <button onclick={load}>Повторить</button>
  {:else}
    <ol>
      <li><div class="grow"><strong>Репозиторий сохранений</strong><span class="muted block">{repositorySet ? 'Локальный Git-репозиторий указан' : 'Для GitHub укажите папку для клона; без GitHub выберите существующий локальный репозиторий'}</span></div><button onclick={onsettings}>{repositorySet ? 'Изменить' : 'Настроить'}</button></li>
      {#if features.sync && remoteConfigured}<li><div class="grow"><strong>Доступ к GitHub</strong><span class="muted block">{settings?.hasGithubToken ? 'Токен сохранён; доступ к репозиторию ещё не подтверждён' : 'Сохраните токен в настройках'}</span></div><button onclick={onsettings}>{settings?.hasGithubToken ? 'Настройки' : 'Указать токен'}</button></li>{/if}
      <li><div class="grow"><strong>Первая игра</strong><span class="muted block">{gamesLoading ? 'Проверяем список игр…' : gamesError ? 'Список игр не удалось загрузить' : hasGames ? 'Игра добавлена в список' : 'Выберите exe и папку сохранений'}</span></div><button disabled={gamesLoading || !!gamesError || adding} onclick={onadd}>{hasGames ? 'Добавить ещё' : 'Добавить игру'}</button></li>
    </ol>
    <p class="muted">После настройки репозитория подключите его и проверьте состояние синхронизации вверху окна.</p>
  {/if}
</section>

<style>
  .first-run { margin: 16px 0; padding: 16px; }
  h3 { margin: 0 0 12px; }
  ol { padding: 0; margin: 0 0 12px; list-style: none; counter-reset: step; }
  li { display: flex; gap: 12px; align-items: center; padding: 12px 0; counter-increment: step; }
  li::before { content: counter(step); display: grid; place-items: center; flex-shrink: 0; width: 26px; height: 26px; border-radius: 50%; background: var(--accent-soft); color: var(--accent); }
  li + li { border-top: 1px solid var(--border); }
  p { margin-bottom: 0; }
  @media (max-width: 560px) { li { flex-wrap: wrap; } li .grow { flex-basis: calc(100% - 42px); } li button { margin-left: 38px; } }
</style>
