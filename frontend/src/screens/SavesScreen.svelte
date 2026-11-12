<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { formatAgo, formatSize } from '../lib/format'
  import { notice } from '../lib/notice.svelte'
  import type { AppFeatures, Game, SaveHistoryEntry, SaveKind } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'
  import { filterHistory, groupHistory, kindLabel, sessionPeriod, type Grouping, type HistorySort } from '../lib/saveHistory'

  let { features }: { features: AppFeatures } = $props()
  let profileId = $state('')
  let kind = $state<SaveKind | ''>('')
  let from = $state('')
  let to = $state('')
  let grouping = $state<Grouping>('session')
  let order = $state<HistorySort>('newest')
  let games = $state<Game[]>([])
  let gameId = $state<string | null>(null)
  let saves = $state<SaveHistoryEntry[]>([])
  const profiles = $derived([...new Map(saves.map(save => [save.profileId, save.profileName])).entries()])
  const filtered = $derived(filterHistory(saves, { gameId, profileId, kind, from, to }))
  const groups = $derived(groupHistory(filtered, grouping, order))
  const extraFilters = $derived(Number(!!kind) + Number(!!from) + Number(!!to))
  const hasFilters = $derived(!!gameId || !!profileId || extraFilters > 0)
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let confirming = $state<string | null>(null)
  let restoring = $state(false)
  let capturing = $state(false)
  let generation = 0

  function resetFilters() { gameId = null; profileId = ''; kind = ''; from = ''; to = '' }

  async function capture() {
    if (capturing) return
    capturing = true
    try { await api.createManualSave(gameId); notice.show('Ручной снимок сохранён') }
    catch (e) { notice.error(e) }
    finally { capturing = false }
  }

  async function load() {
    const current = ++generation
    loading = true
    confirming = null
    try {
      const result = await api.listSaveHistory(null)
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
      notice.show(`${save.gameName}: снимок применён. Создана резервная копия предыдущего состояния.`)
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

{#snippet saveList(rows: SaveHistoryEntry[])}
    <ul class="list">
      {#each rows as save (save.id)}
        <li class="row">
          <div class="grow">
            <div>{kindLabel[save.kind]}</div>
            <div class="muted">
              {save.gameName} · {save.profileName} · <time datetime={new Date(save.createdAt).toISOString()} title={new Date(save.createdAt).toLocaleString('ru-RU')}>{formatAgo(save.createdAt)}</time> · {formatSize(save.sizeBytes)}
            </div>
            {#if confirming === save.id}<p class="muted confirmation">Текущие файлы будут заменены. Перед этим создадим резервную копию.</p>{/if}
          </div>
          {#if confirming === save.id}
            <button disabled={restoring} onclick={() => (confirming = null)}>Отмена</button>
            <button disabled={restoring} class="danger" onclick={() => restore(save)}>Применить снимок</button>
          {:else}
            <button disabled={restoring} onclick={() => (confirming = save.id)}>Применить</button>
          {/if}
        </li>
      {/each}
    </ul>
{/snippet}

<div class="screen-heading">
  <div>
    <h2>Сохранения</h2>
    <p class="muted">Снимки файлов игры и резервные копии перед заменой данных.</p>
  </div>
  {#if features.saveHistory}
    <button class="primary capture" disabled={capturing || restoring} onclick={capture}
      title="Создать снимок выбранной запущенной игры. Для «Все игры» должна быть запущена ровно одна игра.">
      {capturing ? 'Создаём снимок…' : 'Снимок сейчас'}
    </button>
  {/if}
</div>

{#if !features.saveHistory}
  <p class="availability-note">История сохранений и восстановление пока недоступны в этой версии.</p>
{:else}
  <div class="selection">
    <label class="field"><span class="muted">Игра</span>
      <select disabled={restoring} bind:value={gameId}>
        <option value={null}>Все игры</option>
        {#each games as game (game.id)}<option value={game.id}>{game.name}</option>{/each}
      </select>
    </label>
    <label class="field"><span class="muted">Профиль</span>
      <select bind:value={profileId}>
        <option value="">Все профили</option>
        {#each profiles as [id, name]}<option value={id}>{name}</option>{/each}
      </select>
    </label>
  </div>

  <div class="view-options">
    <label class="field"><span class="muted">Группировка</span>
      <select bind:value={grouping}>
        <option value="session">По игровой сессии</option><option value="game">По игре</option>
        <option value="profile">По профилю</option><option value="day">По дню</option><option value="none">Без группировки</option>
      </select>
    </label>
    <label class="field"><span class="muted">По дате</span>
      <select bind:value={order} title="Сессии — по дате окончания, снимки — по дате создания. Порядок общий для групп и снимков.">
        <option value="newest">Сначала новые</option><option value="oldest">Сначала старые</option>
      </select>
    </label>
  </div>

  <details class="filters">
    <summary>Дополнительные фильтры{extraFilters ? ` · ${extraFilters}` : ''}</summary>
    <div class="filter-fields">
      <label class="field"><span class="muted">Причина</span><select bind:value={kind}><option value="">Все причины</option>{#each Object.entries(kindLabel) as [value, label]}<option {value}>{label}</option>{/each}</select></label>
      <label class="field"><span class="muted">С даты</span><input type="date" bind:value={from} /></label>
      <label class="field"><span class="muted">По дату</span><input type="date" bind:value={to} min={from} /></label>
    </div>
  </details>
  <div class="results-meta">
    <span class="muted">Снимков: {filtered.length}{hasFilters ? ` из ${saves.length}` : ''}</span>
    {#if hasFilters}<button class="reset" onclick={resetFilters}>Сбросить фильтры</button>{/if}
  </div>
  {#if from && to && from > to}<p class="error" role="alert">Дата начала должна быть не позже даты окончания.</p>{/if}

  <Loadable {loading} error={loadError}>

    {#each groups as group (group.key)}
      {#if grouping === 'none'}
        {@render saveList(group.saves)}
      {:else}
        <details class="save-group" open>
          <summary>
            <span class="group-title">{group.title} <span class="muted">({group.saves.length})</span></span>
            {#if group.period}<span class="group-period muted">{sessionPeriod(saves.filter(save => save.sessionId === group.key))}</span>{/if}
          </summary>
          {@render saveList(group.saves)}
        </details>
      {/if}
    {:else}<p class="muted">{saves.length ? 'По заданным фильтрам ничего не найдено.' : 'Сохранений пока нет.'}</p>
    {/each}
  </Loadable>
{/if}

<style>
  .screen-heading { display: flex; justify-content: space-between; align-items: start; gap: 16px; margin-bottom: 20px; }
  .screen-heading p { margin: 6px 0 0; }
  .capture { flex-shrink: 0; }
  .selection, .view-options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
  .selection .field, .view-options .field, .filter-fields .field { margin: 0; min-width: 0; }
  .view-options { margin-top: 14px; }
  .filters { margin-top: 16px; padding: 12px 0; border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  summary { cursor: pointer; color: var(--muted); width: fit-content; }
  summary:hover { color: var(--text); }
  .filter-fields { display: grid; grid-template-columns: minmax(0, 1.5fr) repeat(2, minmax(0, 1fr)); gap: 16px; padding-top: 14px; }
  .save-group { margin-top: 20px; }
  .save-group > summary { position: relative; list-style: none; padding-left: 20px; width: 100%; color: var(--text); }
  .save-group > summary::-webkit-details-marker { display: none; }
  .save-group > summary::before { content: '▸'; position: absolute; left: 0; top: 0; color: var(--muted); }
  .save-group[open] > summary::before { content: '▾'; }
  .group-title { display: block; font-size: 15px; font-weight: 700; }
  .group-period { display: block; margin-top: 6px; }
  .save-group[open] > summary { margin-bottom: 12px; }
  .results-meta { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; margin-top: 8px; }
  .reset { padding: 2px 0; border: 0; background: transparent; color: var(--accent); font-size: 13px; }
  time { text-decoration: underline dotted; text-underline-offset: 3px; cursor: help; }
  .confirmation { margin: 8px 0 0; }
  @media (max-width: 600px) {
    .screen-heading { flex-wrap: wrap; }
    .selection, .view-options, .filter-fields { grid-template-columns: minmax(0, 1fr); }
    .row { flex-wrap: wrap; }
    .row .grow { flex-basis: 100%; }
  }
</style>
