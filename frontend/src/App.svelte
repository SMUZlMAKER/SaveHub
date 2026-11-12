<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError, events, subscribe } from './lib/api'
  import { formatAgo } from './lib/format'
  import { notice } from './lib/notice.svelte'
  import type { AppFeatures } from './lib/types'
  import type { GameProcessEvent, ScreenId, SyncStatus, UpdateStatus } from './lib/types'
  import GamesScreen from './screens/GamesScreen.svelte'
  import ProfilesScreen from './screens/ProfilesScreen.svelte'
  import SavesScreen from './screens/SavesScreen.svelte'
  import SettingsScreen from './screens/SettingsScreen.svelte'
  import ConflictsScreen from './screens/ConflictsScreen.svelte'

  const tabs: { id: ScreenId; title: string }[] = [
    { id: 'games', title: 'Игры' },
    { id: 'profiles', title: 'Профили' },
    { id: 'saves', title: 'Сохранения' },
    { id: 'settings', title: 'Настройки' },
  ]
  const screenIds: ScreenId[] = ['games', 'profiles', 'saves', 'settings', 'conflicts']

  let features = $state<AppFeatures | null>(null)
  let featureError = $state('')
  let syncError = $state('')

  async function loadFeatures() {
    featureError = ''
    try {
      features = await api.getAppFeatures()
      await Promise.all([loadSync(), loadConflicts()])
    } catch (e) { featureError = describeError(e) }
  }

  let processError = $state('')
  let update = $state<UpdateStatus | null>(null)
  let screen = $state<ScreenId>('games')
  let settingsTarget = $state<'repository' | 'token' | undefined>(undefined)
  let sync = $state<SyncStatus | null>(null)
  let conflictCount = $state(0)
  let syncing = $state(false)
  // Перерисовывает «N мин назад»
  let now = $state(Date.now())

  const syncText = $derived.by(() => {
    if (!features) return 'Проверяем доступные функции…'
    if (!features.sync) return 'Синхронизация пока недоступна в этой версии'
    if (syncError) return 'Не удалось получить статус синхронизации'
    if (!sync) return 'Проверяем состояние синхронизации…'
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
    if (!features?.sync) return
    try {
      syncError = ''
      sync = await api.getSyncStatus()
    } catch (e) {
      sync = null
      syncError = describeError(e)
    }
  }

  async function loadConflicts() {
    if (!features?.conflicts) return
    try {
      conflictCount = (await api.listConflicts()).length
    } catch (e) {
      notice.error(e)
    }
  }

  async function syncNow() {
    if (!features?.sync || syncing || !sync || sync.state === 'notConfigured' || sync.state === 'syncing') return
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
    api.getUpdateStatus().then((value) => { update = value }).catch(console.error)
    loadFeatures()
    const timer = setInterval(() => (now = Date.now()), 30_000)
    const stops = [
      subscribe<GameProcessEvent>(events.gameProcessChanged, (event) => { processError = event.error ?? '' }),
      subscribe<UpdateStatus>(events.updateStatusChanged, (value) => { update = value }),
      subscribe(events.syncStatusChanged, loadSync),
      subscribe(events.conflictsChanged, loadConflicts),
      // Пункты трея «Сохранения…» и «Настройки…» открывают окно на нужном экране
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
  <button data-diagnostic="sync_now" onclick={syncNow} disabled={!features?.sync || !sync || sync.state === 'notConfigured' || syncing || sync.state === 'syncing'}>Синхронизировать</button>
</header>

<nav>
  {#each tabs as tab (tab.id)}
    <button data-diagnostic={`navigate.${tab.id}`} class="tab" class:active={screen === tab.id} onclick={() => (screen = tab.id)}>{tab.title}</button>
  {/each}
  {#if conflictCount > 0}
    <button data-diagnostic="navigate.conflicts" class="tab conflicts" class:active={screen === 'conflicts'} onclick={() => (screen = 'conflicts')}>
      Конфликты <span class="badge warn">{conflictCount}</span>
    </button>
  {/if}
</nav>

{#if update?.version}
  <aside class="row" role="status">
    Доступно обновление SaveHub {update.version}.
    <button onclick={() => (screen = 'settings')}>Открыть обновления</button>
  </aside>
{/if}

{#if processError}<aside class="error" role="alert">{processError}</aside>{/if}


<main>
  {#if featureError}
    <div class="card" role="alert"><h2>Не удалось проверить доступные функции</h2><p>{featureError}</p><button onclick={loadFeatures}>Повторить</button></div>
  {:else if !features}
    <p role="status">Загружаем SaveHub…</p>
  {:else}
  {#if syncError}<p role="alert">{syncError} <button onclick={loadSync}>Повторить проверку</button></p>{/if}
  {#if screen === 'games'}
    <GamesScreen {features} onsettings={(target) => { settingsTarget = target; screen = 'settings' }} />
  {:else if screen === 'profiles'}
    <ProfilesScreen {features} />
  {:else if screen === 'saves'}
    <SavesScreen {features} />
  {:else if screen === 'settings'}
    <SettingsScreen {features} target={settingsTarget} />
  {:else}
    <ConflictsScreen {features} />
  {/if}
  {/if}
</main>

{#if notice.current}
  <div class="notice" class:error={notice.current.kind === 'error'} role="status">{notice.current.text}</div>
{/if}
