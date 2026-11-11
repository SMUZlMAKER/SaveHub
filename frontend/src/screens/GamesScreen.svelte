<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import type { AppFeatures, Game, GameStatus, SyncStatus } from '../lib/types'
  import FirstRun from '../lib/components/FirstRun.svelte'
  let { features, onsettings }: { features: AppFeatures; onsettings: () => void } = $props()

  import { notice } from '../lib/notice.svelte'
  import GameForm from '../lib/components/GameForm.svelte'

  const statusLabel: Record<GameStatus, { text: string; tone: string }> = {
    found: { text: 'Пути найдены', tone: 'ok' },
    missing: { text: 'Нужны пути', tone: 'warn' },
    running: { text: 'Запущена', tone: 'danger' },
    ignored: { text: 'Игнорируется', tone: '' },
  }
  let games = $state<Game[]>([])
  let catalogStatus = $state<SyncStatus | null>(null)
  let syncing = $state(false)
  let syncError = $state('')
  let syncStartedAfter = 0
  let statusGeneration = 0
  let loading = $state(true)
  let loadError = $state('')
  let savedNote = $state('')
  let editor = $state<{ mode: 'add' | 'edit' | 'resolve'; game?: Game } | null>(null)
  let launching = $state<string | null>(null)
  let shortcutting = $state<string | null>(null)
  async function launch(game: Game) {
    launching = game.id
    try { await api.beginGameLaunch(game.id) } catch (e) { notice.error(e) }
    finally { launching = null }
  }
  async function shortcut(game: Game) {
    shortcutting = game.id
    try { const result = await api.createGameShortcut(game.id); notice.show(`Ярлык создан: ${result.path}`) }
    catch (e) { notice.error(e) }
    finally { shortcutting = null }
  }
  let generation = 0
  async function load() {
    const current = ++generation
    loading = true; loadError = ''
    try {
      const result = await api.listGames()
      if (current === generation) games = result
    } catch (e) { if (current === generation) loadError = describeError(e) }
    finally { if (current === generation) loading = false }
  }
  async function loadStatus() {
    const current = ++statusGeneration
    try {
      const status = await api.getCatalogStatus()
      if (current === statusGeneration) {
        catalogStatus = status
        if (syncError && status.state === 'idle' && (status.lastSyncAt ?? 0) > syncStartedAfter) syncError = ''
      }
    } catch (e) {
      if (current === statusGeneration) catalogStatus = { state: 'error', lastSyncAt: null, error: describeError(e) }
    }
  }
  async function saved(game: Game) {
    // Do not hide a successful mutation behind a failing refresh or a late list response.
    generation++; loading = false; loadError = ''
    games = [...games.filter(g => g.id !== game.id), game].sort((a, b) => a.name.localeCompare(b.name))
    editor = null; savedNote = `Настройки «${game.name}» сохранены на этом ПК.`
    await loadStatus()
  }
  async function sync() {
    syncing = true; syncError = ''; syncStartedAfter = catalogStatus?.lastSyncAt ?? 0
    try { await api.syncCatalog() }
    catch (e) { syncError = describeError(e) }
    finally { await loadStatus(); syncing = false }
  }
  function edit(mode: 'add' | 'edit' | 'resolve', game?: Game) {
    savedNote = ''; editor = { mode, game }
  }
  onMount(() => {
    const stopGames = subscribe(events.gamesChanged, load, load)
    const stopStatus = subscribe(events.catalogStatusChanged, loadStatus, loadStatus)
    return () => { stopGames(); stopStatus(); generation++; statusGeneration++ }
  })
</script>

<div class="games-heading">
  <div><h2>Игры</h2><p class="muted">Выберите игры и укажите, где находятся их сохранения.</p></div>
  <button class="primary" disabled={editor !== null || loading || !!loadError} onclick={() => edit('add')}>Добавить игру</button>
</div>
{#if !editor}<FirstRun {features} gamesCount={games.length} gamesLoading={loading} gamesError={loadError} {onsettings} onadd={() => edit('add')} adding={editor !== null} />{/if}
<div class="setup-note" aria-live="polite">
  {#if syncError}<p class="error" role="alert">{syncError}</p>
  {:else if catalogStatus?.error}<p class="error">{catalogStatus.error}</p>
  {:else if catalogStatus?.state === 'notConfigured'}<p>Список сохранён на этом ПК. Для обмена между ПК подключите репозиторий списка игр в настройках.</p>
  {:else if catalogStatus?.state === 'syncing'}<p>Обновляем список игр…</p>
  {:else if catalogStatus?.lastSyncAt}<p>Список игр синхронизирован: {new Date(catalogStatus.lastSyncAt).toLocaleString()}.</p>{/if}
  <button onclick={sync} disabled={syncing || !!editor}>{syncing ? 'Обновляем…' : 'Синхронизировать список'}</button>
</div>
{#if savedNote}<p class="success-note" role="status">{savedNote}</p>{/if}
{#if editor}
  {#key `${editor.mode}:${editor.game?.id ?? 'new'}`}
    <GameForm mode={editor.mode} game={editor.game} onsaved={saved} oncancel={() => editor = null} />
  {/key}
{/if}
{#if loading}
  <p class="muted" role="status">Загружаем игры…</p>
{:else if loadError}
  <div class="card load-error" role="alert"><p class="error">{loadError}</p><button onclick={load}>Повторить</button></div>
{:else if games.length}
  <ul class="list">
    {#each games as game (game.id)}
      <li class="row game-row">
        <span class="cover" aria-hidden="true">{game.name.slice(0, 2).toUpperCase()}</span>
        <div class="grow">
          <div class="game-title"><strong>{game.name}</strong><span class="badge {statusLabel[game.status].tone}">{statusLabel[game.status].text}</span></div>
          <div class="muted game-path">Запуск: {game.exePath || 'не указан'}</div>
          <div class="muted game-path">Сохранения: {game.savePath || 'не указаны'}</div>
          {#if game.status === 'missing'}
            <div class="problem">{game.exeMissing && game.savePathMissing ? 'На этом ПК не найдены файл игры и папка сохранений.' : game.exeMissing ? 'На этом ПК не найден файл запуска игры.' : 'На этом ПК не найдена папка сохранений.'}</div>
          {/if}
          {#if game.pathScope === 'local'}<div class="muted">Пути заданы только для этого ПК</div>{/if}
          {#if !game.processStateKnown}<div class="muted">Изменение настроек недоступно: состояние запуска игры ещё не проверено</div>{/if}
          {#if game.status === 'running'}<div class="muted">Закройте игру, чтобы изменить её настройки.</div>{/if}
        </div>
        <div class="game-actions">
          <button class="primary" disabled={!!editor || !!launching || game.status !== 'found'} onclick={() => launch(game)}>{launching === game.id ? 'Подготовка…' : 'Запустить'}</button>
          <button disabled={!!editor || !!shortcutting || game.status !== 'found'} onclick={() => shortcut(game)}>{shortcutting === game.id ? 'Создание…' : 'Ярлык на рабочий стол'}</button>
          <button disabled={!!editor || !game.processStateKnown || game.status === 'running'} onclick={() => edit('edit', game)}>Настроить</button>
          <button disabled={!!editor || !game.processStateKnown || game.status === 'running'} onclick={() => edit('resolve', game)}>{game.status === 'ignored' ? 'Подключить на этом ПК' : 'Пути на этом ПК'}</button>
        </div>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .games-heading { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
  .setup-note { padding: 10px 12px; border-radius: 6px; background: var(--accent-soft); font-size: 13px; }
  .success-note { color: var(--ok); }
  .load-error { padding: 16px; }
  .game-title { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 4px; align-items: center; }
  .game-path { overflow-wrap: anywhere; }
  .problem { color: var(--warn); font-size: 13px; margin-top: 4px; }
  .game-actions { display: flex; flex-direction: column; gap: 6px; }
  .game-row { align-items: start; padding: 16px; }
  @media (max-width: 650px) {
    .game-row { flex-wrap: wrap; }
    .game-row .grow { min-width: 200px; }
    .game-actions { width: 100%; flex-direction: row; flex-wrap: wrap; }
  }
</style>
