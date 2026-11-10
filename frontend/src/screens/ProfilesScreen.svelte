<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from '../lib/api'
  import { notice } from '../lib/notice.svelte'
  import type { Profile } from '../lib/types'
  import DevNote from '../lib/components/DevNote.svelte'
  import Loadable from '../lib/components/Loadable.svelte'

  let profiles = $state<Profile[]>([])
  let loading = $state(true)
  let loadError = $state<string | null>(null)
  let switchingTo = $state<string | null>(null)
  let newName = $state('')

  async function load() {
    try {
      profiles = await api.listProfiles()
      loadError = null
    } catch (e) {
      loadError = describeError(e)
    } finally {
      loading = false
    }
  }

  async function activate(profile: Profile) {
    switchingTo = profile.id
    try {
      await api.switchProfile(profile.id)
      notice.show(`Сохранения переключены на профиль «${profile.name}». Снапшот прежнего состояния создан.`)
      await load()
    } catch (e) {
      notice.error(e)
    } finally {
      switchingTo = null
    }
  }

  async function remove(profile: Profile) {
    try {
      await api.deleteProfile(profile.id)
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault()
    try {
      await api.createProfile(newName.trim())
      newName = ''
      await load()
    } catch (e) {
      notice.error(e)
    }
  }

  function describe(profile: Profile): string {
    if (profile.gamesCount === 0) return 'без сохранений'
    const last = profile.lastPlayedGame ? ` · последний запуск: ${profile.lastPlayedGame}` : ''
    return `игр с сохранениями: ${profile.gamesCount}${last}`
  }

  onMount(() => {
    load()
    return subscribe(events.profileChanged, load)
  })
</script>

<h2>Профили</h2>
<p class="muted">У каждого профиля свой набор сохранений во всех играх. Переключение — сразу для всех игр.</p>

<Loadable {loading} error={loadError}>
  <ul class="list">
    {#each profiles as profile (profile.id)}
      <li class="row">
        <div class="grow">
          <div>{profile.name}</div>
          <div class="muted">{describe(profile)}</div>
        </div>
        {#if profile.isActive}
          <span class="badge ok">активен</span>
        {:else}
          <button class="danger" onclick={() => remove(profile)}>Удалить</button>
          <button class="primary" onclick={() => activate(profile)} disabled={switchingTo !== null}>
            {switchingTo === profile.id ? 'Переключаем…' : 'Сделать активным'}
          </button>
        {/if}
      </li>
    {/each}
  </ul>

  <h3>Новый профиль</h3>
  <form class="inline-form" onsubmit={create}>
    <input class="grow" bind:value={newName} placeholder="Имя профиля" />
    <button class="primary" disabled={!newName.trim()}>Создать</button>
  </form>
</Loadable>

<DevNote
  file="src/api/profiles.rs"
  commands={['list_profiles', 'switch_profile', 'create_profile', 'delete_profile']}
/>
