<script lang="ts">
  import type { AppFeatures } from '../lib/types'
  let { features }: { features: AppFeatures } = $props()

  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { formatAgo, formatSize } from '../lib/format'
  import { notice } from '../lib/notice.svelte'
  import type { Game, SaveHistoryEntry, SaveKind } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  import { filterHistory, groupHistory, kindLabel, sessionPeriod, type Grouping } from '../lib/saveHistory'
  let query = $state('')
  let profileId = $state('')
  let kind = $state<SaveKind | ''>('')
  let from = $state('')
  let to = $state('')
  let grouping = $state<Grouping>('session')


  let games = $state<Game[]>([])
  let gameId = $state<string | null>(null)
  let saves = $state<SaveHistoryEntry[]>([])
  const profiles = $derived([...new Map(saves.map(save => [save.profileId, save.profileName])).entries()])
  const filtered = $derived(filterHistory(saves, { query, profileId, kind, from, to }))
  const groups = $derived(groupHistory(filtered, grouping))
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  // Подтверждение требуется перед заменой текущих файлов.
  let confirming = $state<string | null>(null)
  let restoring = $state(false)
  let capturing = $state(false)
  async function capture() {
    if (capturing) return
    capturing = true
    try { await api.createManualSave(gameId); notice.show('Ручной снимок сохранён') }
    catch (e) { notice.error(e) }
    finally { capturing = false }
  }
  let generation = 0

  async function load() {
    const current = ++generation
    loading = true
    confirming = null
    try {
      const result = await api.listSaveHistory(gameId)
      if (current !== generation) return
      saves = result
      loadError = null
    } catch (e) {
      if (current === generation) loadError = describeError(e)
    } finally {
      if (current === generation) loading = false
    }
  }

  async function restore(save: SaveHistoryEntry) {
    if (restoring) return
    restoring = true
    confirming = null
    try {
      await api.restoreSave(save.id)
      notice.show(`${save.gameName}: откат выполнен. Создана резервная копия предыдущего состояния.`)
      await load()
    } catch (e) {
      notice.error(e)
    } finally { restoring = false }
  }

  onMount(() => {
    if (!features.saveHistory) return
    api.listGames().then((list) => (games = list), notice.error.bind(notice))
    const stop = subscribe(events.savesChanged, load, load)
    return () => { generation++; stop() }
  })
</script>

<h2>Сохранения и откат</h2>
<p class="muted">Локальные снимки файлов сохранений после игры и перед заменой данных. Это отдельная история, не список Git-коммитов.</p>

{#if !features.saveHistory}<p class="availability-note">История сохранений и восстановление пока недоступны в этой версии. Автоматические копии ещё не создаются.</p>{/if}
{#if features.saveHistory}
<label class="field">
  <span class="muted">Игра</span>
  <select disabled={restoring} bind:value={gameId} onchange={load}>
    <option value={null}>Все игры</option>
    {#each games as game (game.id)}
      <option value={game.id}>{game.name}</option>
    {/each}
  </select>
</label>

<button disabled={capturing || restoring} onclick={capture}>{capturing ? 'Создаём снимок…' : 'Снимок сейчас'}</button>
<p class="muted">Для выбранной запущенной игры; при выборе «Все игры» должна быть запущена ровно одна игра.</p>
<div class="filters">
  <label class="field"><span>Поиск</span><input type="search" bind:value={query} placeholder="Игра, профиль, причина или ID" /></label>
  <label class="field"><span>Профиль</span><select bind:value={profileId}><option value="">Все профили</option>{#each profiles as [id, name]}<option value={id}>{name}</option>{/each}</select></label>
  <label class="field"><span>Причина</span><select bind:value={kind}><option value="">Все причины</option>{#each Object.entries(kindLabel) as [value, label]}<option {value}>{label}</option>{/each}</select></label>
  <label class="field"><span>С даты</span><input type="date" bind:value={from} /></label>
  <label class="field"><span>По дату</span><input type="date" bind:value={to} min={from} /></label>
  <label class="field"><span>Группировка</span><select bind:value={grouping}><option value="session">По игровой сессии</option><option value="game">По игре</option><option value="profile">По профилю</option><option value="day">По дню</option><option value="none">Без группировки</option></select></label>
  <button onclick={() => { query = ''; profileId = ''; kind = ''; from = ''; to = '' }}>Сбросить фильтры</button>
</div>
<p class="muted">Найдено снимков: {filtered.length} из {saves.length}. Старые снимки без идентификатора сессии показаны отдельно.</p>
{#if from && to && from > to}<p class="error" role="alert">Дата начала должна быть не позже даты окончания.</p>{/if}
<Loadable {loading} error={loadError}>
  {#each groups as group (group.key)}
    <section aria-label={group.title}>
      <h3>{group.title} <span class="muted">({group.saves.length})</span></h3>
      {#if group.period}<p class="muted">{sessionPeriod(saves.filter(save => save.sessionId === group.key))}</p>{/if}
  <ul class="list">
    {#each group.saves as save (save.id)}
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
          <button disabled={restoring} onclick={() => (confirming = null)}>Отмена</button>
          <button disabled={restoring} class="danger" onclick={() => restore(save)}>Точно откатить</button>
        {:else}
          <button disabled={restoring} onclick={() => (confirming = save.id)}>Откатить</button>
        {/if}
      </li>
    {/each}
  </ul>
    </section>
  {:else}<p class="muted">{saves.length ? 'По заданным фильтрам ничего не найдено.' : 'Сохранений пока нет.'}</p>
  {/each}
</Loadable>
{/if}

<style>
  .filters { display: flex; flex-wrap: wrap; gap: 12px; align-items: end; }
  .filters .field { flex: 1 1 180px; }
</style>
