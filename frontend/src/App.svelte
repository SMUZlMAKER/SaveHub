<script lang="ts">
  import { onMount } from 'svelte'
  import { api, events, subscribe } from './lib/api'
  import { formatAgo } from './lib/format'
  import { notice } from './lib/notice.svelte'
  import type { ScreenId, SyncStatus } from './lib/types'
  import GamesScreen from './screens/GamesScreen.svelte'
  import ProfilesScreen from './screens/ProfilesScreen.svelte'
  import SnapshotsScreen from './screens/SnapshotsScreen.svelte'
  import SettingsScreen from './screens/SettingsScreen.svelte'
  import ConflictsScreen from './screens/ConflictsScreen.svelte'

  const tabs: { id: ScreenId; title: string }[] = [
    { id: 'games', title: 'Игры' },
    { id: 'profiles', title: 'Профили' },
    { id: 'snapshots', title: 'Снапшоты' },
    { id: 'settings', title: 'Настройки' },
  ]
  const screenIds: ScreenId[] = ['games', 'profiles', 'snapshots', 'settings', 'conflicts']

  let screen = $state<ScreenId>('games')
  let sync = $state<SyncStatus | null>(null)
  let conflictCount = $state(0)
  let syncing = $state(false)
  // Перерисовывает «N мин назад»
  let now = $state(Date.now())

  const syncText = $derived.by(() => {
    if (!sync) return ''
    switch (sync.state) {
      case 'notConfigured':
        return 'Синхронизация не настроена'
      case 'syncing':
        return 'Синхронизируем…'
      case 'error':
        return `Ошибка синхронизации: ${sync.error ?? 'неизвестно'}`
      case 'idle':
        return sync.lastSyncAt ? `Синхронизировано ${formatAgo(sync.lastSyncAt, now)}` : 'Ещё не синхронизировано'
    }
  })

  async function loadSync() {
    try {
      sync = await api.getSyncStatus()
    } catch (e) {
      notice.error(e)
    }
  }

  async function loadConflicts() {
    try {
      conflictCount = (await api.listConflicts()).length
    } catch (e) {
      notice.error(e)
    }
  }

  async function syncNow() {
    syncing = true
    try {
      await api.syncNow()
    } catch (e) {
      notice.error(e)
    } finally {
      syncing = false
      await loadSync()
    }
  }

  onMount(() => {
    loadSync()
    loadConflicts()
    const timer = setInterval(() => (now = Date.now()), 30_000)
    const stops = [
      subscribe(events.syncStatusChanged, loadSync),
      subscribe(events.conflictsChanged, loadConflicts),
      // Пункты трея «Снапшоты и откат…» и «Настройки…» открывают окно на нужном экране
      subscribe<string>(events.navigate, (target) => {
        if ((screenIds as string[]).includes(target)) screen = target as ScreenId
      }),
    ]
    return () => {
      clearInterval(timer)
      stops.forEach((stop) => stop())
    }
  })
</script>

<header>
  <div class="brand">
    <h1>SaveHub</h1>
    <span class="muted sync-state" class:error={sync?.state === 'error'}>{syncText}</span>
  </div>
  <button onclick={syncNow} disabled={syncing || sync?.state === 'syncing'}>Синхронизировать</button>
</header>

<nav>
  {#each tabs as tab (tab.id)}
    <button class="tab" class:active={screen === tab.id} onclick={() => (screen = tab.id)}>{tab.title}</button>
  {/each}
  {#if conflictCount > 0}
    <button class="tab conflicts" class:active={screen === 'conflicts'} onclick={() => (screen = 'conflicts')}>
      Конфликты <span class="badge warn">{conflictCount}</span>
    </button>
  {/if}
</nav>

<main>
  {#if screen === 'games'}
    <GamesScreen />
  {:else if screen === 'profiles'}
    <ProfilesScreen />
  {:else if screen === 'snapshots'}
    <SnapshotsScreen />
  {:else if screen === 'settings'}
    <SettingsScreen />
  {:else}
    <ConflictsScreen />
  {/if}
</main>

{#if notice.current}
  <div class="notice" class:error={notice.current.kind === 'error'} role="status">{notice.current.text}</div>
{/if}
