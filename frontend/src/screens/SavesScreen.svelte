<script lang="ts">
  import type { AppFeatures } from '../lib/types'
  let { features }: { features: AppFeatures } = $props()

  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { formatAgo, formatSize } from '../lib/format'
  import { notice } from '../lib/notice.svelte'
  import type { Game, SaveHistoryEntry, SaveKind } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  const kindLabel: Record<SaveKind, string> = {
    beforeSwitch: 'Перед переключением профиля',
    afterSession: 'После игровой сессии',
    conflict: 'Вторая версия при конфликте',
    beforeRestore: 'Перед откатом',
  }

  let games = $state<Game[]>([])
  let gameId = $state<string | null>(null)
  let saves = $state<SaveHistoryEntry[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  // Откат необратим без нового сохранения, поэтому спрашиваем второй раз
  let confirming = $state<string | null>(null)

  async function load() {
    try {
      saves = await api.listSaveHistory(gameId)
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  async function restore(save: SaveHistoryEntry) {
    confirming = null
    try {
      await api.restoreSave(save.id)
      notice.show(`${save.gameName}: откат выполнен. Создана резервная копия предыдущего состояния.`)
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  onMount(() => {
    if (!features.saveHistory) return
    api.listGames().then((list) => (games = list), notice.error.bind(notice))
    load()
    return subscribe(events.savesChanged, load)
  })
</script>

<h2>Сохранения и откат</h2>
<p class="muted">Копии сохранений, которые SaveHub делает перед рискованными действиями.</p>

{#if !features.saveHistory}<p class="availability-note">История сохранений и восстановление пока недоступны в этой версии. Автоматические копии ещё не создаются.</p>{/if}
{#if features.saveHistory}
<label class="field">
  <span class="muted">Игра</span>
  <select bind:value={gameId} onchange={load}>
    <option value={null}>Все игры</option>
    {#each games as game (game.id)}
      <option value={game.id}>{game.name}</option>
    {/each}
  </select>
</label>

<Loadable {loading} error={loadError}>
  <ul class="list">
    {#each saves as save (save.id)}
      <li class="row">
        <div class="grow">
          <div>{kindLabel[save.kind]}</div>
          <div class="muted">
            {save.gameName} · {save.profileName} · {formatAgo(save.createdAt)} · {formatSize(
              save.sizeBytes,
            )}
          </div>
        </div>
        {#if confirming === save.id}
          <button onclick={() => (confirming = null)}>Отмена</button>
          <button class="danger" onclick={() => restore(save)}>Точно откатить</button>
        {:else}
          <button onclick={() => (confirming = save.id)}>Откатить</button>
        {/if}
      </li>
    {:else}
      <li class="row muted">Сохранений пока нет.</li>
    {/each}
  </ul>
</Loadable>
{/if}
