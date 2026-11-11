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
  type Choice = { side: Side; keepBoth: boolean }

  let conflicts = $state<Conflict[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let choices = $state<Record<string, Choice>>({})

  async function load() {
    try {
      conflicts = await api.listConflicts()
      for (const c of conflicts) {
        // По умолчанию — более свежая версия
        choices[c.id] ??= {
          side: c.local.modifiedAt >= c.remote.modifiedAt ? 'local' : 'remote',
          keepBoth: true,
        }
      }
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  function resolution({ side, keepBoth }: Choice): ConflictResolution {
    if (keepBoth) return side === 'local' ? 'keepBothLocalFirst' : 'keepBothRemoteFirst'
    return side === 'local' ? 'keepLocal' : 'keepRemote'
  }

  async function apply(conflict: Conflict) {
    try {
      await api.resolveConflict(conflict.id, resolution(choices[conflict.id]))
      notice.show(`${conflict.gameName}: конфликт решён`)
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  onMount(() => {
    if (!features.conflicts) return
    load()
    return subscribe(events.conflictsChanged, load)
  })
</script>

{#snippet version(conflict: Conflict, side: Side, v: SaveVersion)}
  <button
    type="button"
    class="version"
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
      <label class="toggle">
        <span>Сохранить вторую версию снапшотом</span>
        <input type="checkbox" bind:checked={choices[conflict.id].keepBoth} />
      </label>
      <div class="actions">
        <button class="primary" onclick={() => apply(conflict)}>Применить</button>
      </div>
    </section>
  {:else}
    <p class="muted">Конфликтов нет.</p>
  {/each}
</Loadable>
{/if}
