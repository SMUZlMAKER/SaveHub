<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { formatAgo, formatSize } from '../lib/format'
  import { notice } from '../lib/notice.svelte'
  import type { Game, Snapshot, SnapshotKind } from '../lib/types'
  import DevNote from '../lib/components/DevNote.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  const kindLabel: Record<SnapshotKind, string> = {
    beforeSwitch: 'Перед переключением профиля',
    afterSession: 'После игровой сессии',
    conflict: 'Вторая версия при конфликте',
    beforeRestore: 'Перед откатом',
  }

  let games = $state<Game[]>([])
  let gameId = $state<string | null>(null)
  let snapshots = $state<Snapshot[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  // Откат необратим без нового снапшота, поэтому спрашиваем второй раз
  let confirming = $state<string | null>(null)

  async function load() {
    try {
      snapshots = await api.listSnapshots(gameId)
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  async function restore(snapshot: Snapshot) {
    confirming = null
    try {
      await api.restoreSnapshot(snapshot.id)
      notice.show(`${snapshot.gameName}: откат выполнен. Текущее состояние сохранено снапшотом.`)
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  onMount(() => {
    api.listGames().then((list) => (games = list), notice.error.bind(notice))
    load()
    return subscribe(events.snapshotsChanged, load)
  })
</script>

<h2>Снапшоты и откат</h2>
<p class="muted">Копии сохранений, которые SaveHub делает перед рискованными действиями.</p>

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
    {#each snapshots as snapshot (snapshot.id)}
      <li class="row">
        <div class="grow">
          <div>{kindLabel[snapshot.kind]}</div>
          <div class="muted">
            {snapshot.gameName} · {snapshot.profileName} · {formatAgo(snapshot.createdAt)} · {formatSize(
              snapshot.sizeBytes,
            )}
          </div>
        </div>
        {#if confirming === snapshot.id}
          <button onclick={() => (confirming = null)}>Отмена</button>
          <button class="danger" onclick={() => restore(snapshot)}>Точно откатить</button>
        {:else}
          <button onclick={() => (confirming = snapshot.id)}>Откатить</button>
        {/if}
      </li>
    {:else}
      <li class="row muted">Снапшотов пока нет.</li>
    {/each}
  </ul>
</Loadable>

<DevNote file="src/api/snapshots.rs" commands={['list_snapshots', 'restore_snapshot']} />
