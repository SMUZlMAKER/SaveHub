<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { Game, GameStatus } from '../lib/types'
  import DevNote from '../lib/components/DevNote.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  const statusLabel: Record<GameStatus, { text: string; tone: string }> = {
    found: { text: 'найдено', tone: 'ok' },
    missing: { text: 'нужен путь', tone: 'warn' },
    running: { text: 'запущена', tone: 'danger' },
  }

  let games = $state<Game[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)

  // id игры, для которой открыт ввод пути
  let editingPathFor = $state<string | null>(null)
  let pathDraft = $state('')

  let newName = $state('')
  let newPath = $state('')

  async function load() {
    try {
      games = await api.listGames()
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  async function savePath(game: Game) {
    try {
      await api.setGameSavePath(game.id, pathDraft.trim())
      editingPathFor = null
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  async function addGame(event: SubmitEvent) {
    event.preventDefault()
    try {
      await api.addGame(newName.trim(), newPath.trim())
      newName = ''
      newPath = ''
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  onMount(() => {
    load()
    return subscribe(events.gamesChanged, load)
  })
</script>

<h2>Игры</h2>
<p class="muted">Игры, сохранения которых переключаются и синхронизируются.</p>

<Loadable {loading} error={loadError}>
  <ul class="list">
    {#each games as game (game.id)}
      <li class="row">
        <span class="cover">{game.name.slice(0, 2).toUpperCase()}</span>
        <div class="grow">
          <div>{game.name}</div>
          {#if editingPathFor === game.id}
            <div class="inline-form">
              <input bind:value={pathDraft} placeholder="Папка с сохранениями" />
              <button class="primary" onclick={() => savePath(game)} disabled={!pathDraft.trim()}>
                Сохранить
              </button>
              <button onclick={() => (editingPathFor = null)}>Отмена</button>
            </div>
          {:else}
            <div class="muted path">{game.savePath ?? 'папка сохранений не найдена'}</div>
          {/if}
        </div>
        {#if game.status === 'missing' && editingPathFor !== game.id}
          <button
            onclick={() => {
              editingPathFor = game.id
              pathDraft = ''
            }}>Указать путь</button
          >
        {/if}
        <span class="badge {statusLabel[game.status].tone}">{statusLabel[game.status].text}</span>
      </li>
    {:else}
      <li class="row muted">Пока ни одной игры.</li>
    {/each}
  </ul>

  <h3>Добавить игру вручную</h3>
  <!-- TODO: выбор папки через диалог (tauri-plugin-dialog) вместо ввода пути руками -->
  <form class="inline-form" onsubmit={addGame}>
    <input bind:value={newName} placeholder="Название" />
    <input class="grow" bind:value={newPath} placeholder="Папка с сохранениями" />
    <button class="primary" disabled={!newName.trim() || !newPath.trim()}>Добавить</button>
  </form>
</Loadable>

<DevNote file="src/api/games.rs" commands={['list_games', 'add_game', 'set_game_save_path']} />
