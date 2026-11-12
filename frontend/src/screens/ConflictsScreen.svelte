<script lang="ts">
  import type { AppFeatures, Conflict, ConflictCommit } from '../lib/types'
  let { features }: { features: AppFeatures } = $props()

  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  type Side = 'local' | 'remote'
  interface ConflictGroup {
    profileId: string
    profileName: string
    profileBranch: string
    localCommit: ConflictCommit
    remoteCommit: ConflictCommit
    files: Conflict[]
  }

  let conflicts = $state<Conflict[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let choices = $state<Record<string, 'keepLocal' | 'keepRemote'>>({})

  let generation = 0
  let applying = $state(false)
  let confirming = $state<string | null>(null)

  const groups = $derived.by(() => {
    const byProfile = new Map<string, ConflictGroup>()
    for (const conflict of conflicts) {
      let group = byProfile.get(conflict.profileId)
      if (!group) {
        group = {
          profileId: conflict.profileId,
          profileName: conflict.profileName,
          profileBranch: conflict.profileBranch,
          localCommit: conflict.localCommit,
          remoteCommit: conflict.remoteCommit,
          files: [],
        }
        byProfile.set(conflict.profileId, group)
      }
      group.files.push(conflict)
    }
    return [...byProfile.values()]
  })

  async function load() {
    const current = ++generation
    try {
      const result = await api.listConflicts()
      if (current !== generation) return
      conflicts = result
      loadError = null
    } catch (e) {
      if (current === generation) loadError = describeError(e)
    } finally {
      if (current === generation) loading = false
    }
  }

  function formatCommitDate(timestamp: number): string {
    return timestamp > 0
      ? new Date(timestamp).toLocaleString('ru-RU')
      : 'время коммита неизвестно'
  }

  async function apply(group: ConflictGroup) {
    if (applying) return
    const resolution = choices[group.profileId]
    if (!resolution) return
    applying = true
    try {
      await api.resolveConflict(group.files[0].id, resolution)
      confirming = null
      notice.show(
        resolution === 'keepLocal'
          ? `${group.profileName}: локальный коммит выбран и опубликован в origin.`
          : `${group.profileName}: профиль переключён на коммит из origin.`,
      )
      await load()
    } catch (e) {
      confirming = null
      notice.error(e)
      await load()
    } finally { applying = false }
  }

  onMount(() => {
    if (!features.conflicts) return
    const stop = subscribe(events.conflictsChanged, load, load)
    return () => { generation++; stop() }
  })
</script>

{#snippet commit(group: ConflictGroup, side: Side, value: ConflictCommit)}
  <button
    type="button"
    class="version"
    disabled={applying}
    class:selected={choices[group.profileId] === (side === 'local' ? 'keepLocal' : 'keepRemote')}
    onclick={() => {
      choices[group.profileId] = side === 'local' ? 'keepLocal' : 'keepRemote'
      confirming = null
    }}
  >
    <strong>{side === 'local' ? 'Локальная версия профиля' : 'Версия профиля из origin'}</strong>
    <span class="muted block">{formatCommitDate(value.committedAt)}</span>
    <span class="muted block"><code>{value.id.slice(0, 8)}</code> · {value.summary || 'без описания'}</span>
  </button>
{/snippet}

<h2>Конфликты сохранений</h2>
<p class="muted">Выберите, какая версия профиля останется: локальная или полученная из origin. Выбор применяется ко всем сохранениям профиля.</p>

{#if !features.conflicts}<p class="availability-note">Проверка и разрешение конфликтов пока недоступны в этой версии.</p>{/if}
{#if features.conflicts}
<Loadable {loading} error={loadError}>
  {#each groups as group (group.profileId)}
    <section class="card">
      <h3>{group.profileName}</h3>
      <p class="muted">Ветка <code>{group.profileBranch}</code> · {group.files.length} конфликтующих файлов</p>
      <div class="versions">
        {@render commit(group, 'local', group.localCommit)}
        {@render commit(group, 'remote', group.remoteCommit)}
      </div>
      {#if choices[group.profileId] === 'keepLocal'}
        <p class="muted">Локальный коммит станет HEAD. Для этого удалённая ветка профиля будет заменена выбранным локальным коммитом; другая сторона станет недоступна через SaveHub, отменить выбор здесь нельзя.</p>
      {:else if choices[group.profileId] === 'keepRemote'}
        <p class="muted">Коммит из origin станет HEAD локального профиля. Локальная сторона перестанет быть HEAD профиля; отменить выбор через SaveHub нельзя.</p>
      {:else}
        <p class="muted">Выберите одну из сторон. Решение применится сразу ко всем файлам профиля; остальные изменения выбранного коммита также станут активными.</p>
      {/if}
      <div class="actions">
        {#if confirming === group.profileId}
          <button disabled={applying} onclick={() => (confirming = null)}>Назад</button>
          <button class="danger" disabled={applying} onclick={() => apply(group)}>{applying ? 'Применяем…' : 'Подтвердить без отмены'}</button>
        {:else}
          <button class="primary" disabled={applying || !choices[group.profileId]} onclick={() => (confirming = group.profileId)}>Применить выбранный коммит</button>
        {/if}
      </div>
    </section>
  {:else}
    <p class="muted">Конфликтов нет.</p>
  {/each}
</Loadable>
{/if}
