<script lang="ts">
  import type { AppFeatures } from '../lib/types'
  let { features }: { features: AppFeatures } = $props()

  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { formatAgo, formatSize } from '../lib/format'
  import { notice } from '../lib/notice.svelte'
  import type { Conflict, ConflictResolution, SaveVersion } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  type Side = 'local' | 'remote'
  type Choice = { side: Side }

  let conflicts = $state<Conflict[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let choices = $state<Record<string, Choice>>({})

  let generation = 0
  let applying = $state(false)

  async function load() {
    const current = ++generation
    try {
      const result = await api.listConflicts()
      if (current !== generation) return
      conflicts = result
      for (const c of conflicts) {
        // По умолчанию — более свежая версия
        choices[c.id] ??= {
          side: c.local.modifiedAt >= c.remote.modifiedAt ? 'local' : 'remote',
        }
      }
      loadError = null
    } catch (e) {
      if (current === generation) loadError = describeError(e)
    } finally {
      if (current === generation) loading = false
    }
  }

  function resolution({ side }: Choice): ConflictResolution {
    return side === 'local' ? 'keepLocal' : 'keepRemote'
  }

  async function apply(conflict: Conflict) {
    if (applying) return
    applying = true
    try {
      await api.resolveConflict(conflict.id, resolution(choices[conflict.id]))
      notice.show(`${conflict.gameName}: конфликт решён`)
      await load()
    } catch (e) {
      notice.error(e)
    } finally { applying = false }
  }

  onMount(() => {
    if (!features.conflicts) return
    const stop = subscribe(events.conflictsChanged, load, load)
    return () => { generation++; stop() }
  })
</script>

{#snippet version(conflict: Conflict, side: Side, v: SaveVersion)}
  <button
    type="button"
    class="version"
    disabled={applying}
    class:selected={choices[conflict.id]?.side === side}
    onclick={() => (choices[conflict.id].side = side)}
  >
    <strong>{v.machineName}</strong>
    <span class="muted block">изменено {formatAgo(v.modifiedAt)}</span>
    <span class="muted block">{formatSize(v.sizeBytes)}</span>
  </button>
{/snippet}

<h2>Конфликты сохранений</h2>
<p class="muted">Сохранение изменили на двух ПК независимо. Выбери, какую версию оставить.</p>

{#if !features.conflicts}<p class="availability-note">Проверка и разрешение конфликтов пока недоступны в этой версии. Сравнение сохранений между компьютерами ещё не выполняется.</p>{/if}
{#if features.conflicts}
<Loadable {loading} error={loadError}>
  {#each conflicts as conflict (conflict.id)}
    <section class="card">
      <h3>{conflict.gameName}</h3>
      <div class="versions">
        {@render version(conflict, 'local', conflict.local)}
        {@render version(conflict, 'remote', conflict.remote)}
      </div>
      <p class="muted">Обе версии сохраняются в технической резервной копии репозитория. Они не появятся отдельными записями в истории сохранений.</p>
      <div class="actions">
        <button class="primary" disabled={applying} onclick={() => apply(conflict)}>{applying ? 'Применяем…' : 'Применить'}</button>
      </div>
    </section>
  {:else}
    <p class="muted">Конфликтов нет.</p>
  {/each}
</Loadable>
{/if}
