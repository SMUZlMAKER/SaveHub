<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { AppFeatures, Profile } from '../lib/types'
  import Loadable from '../lib/components/Loadable.svelte'

  let { features }: { features: AppFeatures } = $props()
  let mutating = $state(false)

  let profiles = $state<Profile[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let switchingTo = $state<string | null>(null)
  let newName = $state('')
  let generation = 0

  async function load() {
    const current = ++generation
    try {
      const result = await api.listProfiles()
      if (current !== generation) return
      profiles = result
      loadError = null
    } catch (e) {
      if (current === generation) loadError = describeError(e)
    } finally {
      if (current === generation) loading = false
    }
  }

  async function activate(profile: Profile) {
    if (!features.profileSwitch || switchingTo || mutating) return
    switchingTo = profile.id
    try {
      await api.switchProfile(profile.id)
      notice.show(`Активен профиль «${profile.name}».`)
      await load()
    } catch (e) {
      notice.error(e)
    } finally {
      switchingTo = null
    }
  }

  async function remove(profile: Profile) {
    if (mutating || switchingTo) return
    mutating = true
    try {
      await api.deleteProfile(profile.id)
      await load()
    } catch (e) {
      notice.error(e)
    } finally {
      mutating = false
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault()
    if (!newName.trim() || mutating || switchingTo) return
    mutating = true
    try {
      await api.createProfile(newName.trim())
      newName = ''
      await load()
    } catch (e) {
      notice.error(e)
    } finally {
      mutating = false
    }
  }

  function describe(profile: Profile): string {
    if (!features.saveHistory) return 'Сведения о сохранениях пока недоступны'
    if (profile.gamesCount === 0) return 'без сохранений'
    const last = profile.lastPlayedGame ? ` · последний запуск: ${profile.lastPlayedGame}` : ''
    return `игр с сохранениями: ${profile.gamesCount}${last}`
  }

  onMount(() => {
    const stop = subscribe(events.profileChanged, load, load)
    return () => { generation++; stop() }
  })
</script>

<h2>Профили</h2>
<p class="muted">У каждого профиля свой набор сохранений во всех играх. Переключение — сразу для всех игр.</p>

{#if !features.persistentProfiles}<p class="availability-note">Созданные профили доступны только до закрытия приложения.</p>{/if}
{#if !features.profileSwitch}<p class="availability-note">Переключение сохранений между профилями пока недоступно в этой версии.</p>{/if}

<Loadable {loading} error={loadError}>
  <ul class="list">
    {#each profiles as profile (profile.id)}
      <li class="row">
        <div class="grow">
          <div>{profile.name}</div>
          <div class="muted">Ветка: <code>{profile.branch}</code></div>
          <div class="muted">{describe(profile)}</div>
        </div>
        {#if profile.isActive}
          <span class="badge ok">активен</span>
        {:else}
          <button class="danger" onclick={() => remove(profile)} disabled={mutating || switchingTo !== null}>Удалить</button>
          <button class="primary" onclick={() => activate(profile)} disabled={!features.profileSwitch || mutating || switchingTo !== null}>
            {switchingTo === profile.id ? 'Переключаем…' : 'Сделать активным'}
          </button>
        {/if}
      </li>
    {/each}
  </ul>

  <h3>Новый профиль</h3>
  <form class="inline-form" onsubmit={create}>
    <input class="grow" aria-label="Имя профиля" disabled={mutating || switchingTo !== null} bind:value={newName} placeholder="Имя профиля" />
    <button class="primary" disabled={!newName.trim() || mutating || switchingTo !== null}>{mutating ? 'Сохраняем…' : 'Создать'}</button>
  </form>
</Loadable>
