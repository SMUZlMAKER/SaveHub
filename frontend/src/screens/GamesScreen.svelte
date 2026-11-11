<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import type { AppFeatures, Game, GameStatus } from '../lib/types'
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
  let loading = $state(true)
  let loadError = $state('')
  let savedNote = $state('')
  let editor = $state<{ mode: 'add' | 'edit'; game?: Game } | null>(null)
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
  function saved(game: Game) {
    // Do not hide a successful mutation behind a failing refresh or a late list response.
    generation++; loading = false; loadError = ''
    games = [...games.filter(g => g.id !== game.id), game].sort((a, b) => a.name.localeCompare(b.name))
    editor = null; savedNote = features.persistentGames ? `Настройки «${game.name}» сохранены.` : `Настройки «${game.name}» применены для текущего запуска приложения.`
  }
  function edit(mode: 'add' | 'edit', game?: Game) {
    savedNote = ''; editor = { mode, game }
  }
  onMount(() => { load(); return subscribe(events.gamesChanged, load) })
</script>

<div class="games-heading">
  <div><h2>Игры</h2><p class="muted">Выберите игры и укажите, где находятся их сохранения.</p></div>
  <button class="primary" disabled={editor !== null || loading || !!loadError} onclick={() => edit('add')}>Добавить игру</button>
</div>
{#if !editor}<FirstRun {features} gamesCount={games.length} gamesLoading={loading} gamesError={loadError} {onsettings} onadd={() => edit('add')} adding={editor !== null} />{/if}
{#if !features.persistentGames}<p class="setup-note">В этой версии список игр и локальные пути сохраняются только до закрытия приложения. Файлы игр и сохранений не изменяются.</p>{/if}
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
          {#if !game.processStateKnown}<div class="muted">Состояние запуска игры ещё не проверено</div>{/if}
          {#if game.status === 'running'}<div class="muted">Закройте игру, чтобы изменить её настройки.</div>{/if}
        </div>
        <div class="game-actions">
          <button class="primary" disabled={!!editor || !!launching || game.status !== 'found'} onclick={() => launch(game)}>{launching === game.id ? 'Подготовка…' : 'Запустить'}</button>
          <button disabled={!!editor || !!shortcutting || game.status !== 'found'} onclick={() => shortcut(game)}>{shortcutting === game.id ? 'Создание…' : 'Ярлык на рабочий стол'}</button>
          <button disabled={!!editor || game.status === 'running'} onclick={() => edit('edit', game)}>Настроить</button>
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
