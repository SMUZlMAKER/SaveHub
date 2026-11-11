<script lang="ts">
  // TODO(frontend, отдельный PR): явно показывать выполненные шаги онбординга.
  // TODO(frontend, отдельный PR): скрывать онбординг, когда все шаги выполнены.
  import { onMount } from 'svelte'
  import { api, describeError } from '../api'
  import type { AppFeatures, Settings } from '../types'

  let { features, gamesCount, gamesLoading, gamesError, onsettings, onadd, adding }:
    { features: AppFeatures; gamesCount: number; gamesLoading: boolean; gamesError: string;
      onsettings: () => void; onadd: () => void; adding: boolean } = $props()
  let settings = $state<Settings | null>(null)
  let loading = $state(true)
  let error = $state('')
  const repositorySet = $derived(!!settings?.repoUrl?.trim() && !!settings?.localRepoPath?.trim())
  const hasGames = $derived(!gamesLoading && !gamesError && gamesCount > 0)
  const complete = $derived(repositorySet && settings?.hasGithubToken && hasGames)

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
      <li><div class="grow"><strong>Репозиторий сохранений</strong><span class="muted block">{repositorySet ? 'Адрес и локальная папка указаны' : 'Укажите адрес GitHub и папку на этом компьютере'}</span></div><button onclick={onsettings}>{repositorySet ? 'Изменить' : 'Настроить'}</button></li>
      <li><div class="grow"><strong>Доступ к GitHub</strong><span class="muted block">{settings?.hasGithubToken ? 'Токен сохранён; доступ к репозиторию ещё не подтверждён' : 'Сохраните токен в настройках'}</span></div><button onclick={onsettings}>{settings?.hasGithubToken ? 'Настройки' : 'Указать токен'}</button></li>
      <li><div class="grow"><strong>Первая игра</strong><span class="muted block">{gamesLoading ? 'Проверяем список игр…' : gamesError ? 'Список игр не удалось загрузить' : hasGames ? 'Игра добавлена в список' : 'Выберите exe и папку сохранений'}</span></div><button disabled={gamesLoading || !!gamesError || adding} onclick={onadd}>{hasGames ? 'Добавить ещё' : 'Добавить игру'}</button></li>
    </ol>
    <p class="muted">{features.sync ? 'Заполнение параметров не подтверждает подключение. Результат синхронизации появится вверху окна.' : 'Синхронизация пока недоступна в этой версии. Можно подготовить настройки и выбрать игры.'}</p>
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
