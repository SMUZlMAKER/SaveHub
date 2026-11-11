<script lang="ts">
  import { untrack } from 'svelte'
  import { api, describeError } from '../api'
  import type { Game, GameInput, PathAction, SaveSuggestion } from '../types'

  let { game, mode, onsaved, oncancel }: {
    game?: Game
    mode: 'add' | 'edit' | 'resolve'
    onsaved: (game: Game) => void
    oncancel: () => void
  } = $props()
  let name = $state(untrack(() => game?.name ?? ''))
  let exePath = $state(untrack(() => game?.exePath ?? ''))
  let savePath = $state(untrack(() => game?.savePath ?? ''))
  let action = $state<PathAction>('useLocal')
  let saving = $state(false)
  let picking = $state(false)
  let suggesting = $state(false)
  let suggestion = $state<SaveSuggestion | null>(null)
  let suggestionNote = $state('')
  let suggestionError = $state('')
  let error = $state('')
  let errors = $state<Partial<Record<keyof GameInput, string>>>({})
  let request = 0
  const ignored = $derived(mode === 'resolve' && action === 'ignore')
  const busy = $derived(saving || picking)

  function exeChanged() {
    request++; suggesting = false; suggestion = null; suggestionNote = ''; suggestionError = ''
    errors.exePath = undefined
  }
  async function suggest() {
    if (!exePath.trim()) return
    const current = ++request
    const originalName = name, originalSave = savePath, originalExe = exePath
    suggesting = true; suggestion = null; suggestionError = ''; suggestionNote = ''
    try {
      const result = await api.suggestSaveDirs(originalExe.trim())
      if (current !== request || originalExe !== exePath) return
      suggestion = result
      if (!result) suggestionNote = 'Игра не найдена в базе. Укажите название и папку вручную.'
      else {
        if (!originalName.trim() && name === originalName) { name = result.game; errors.name = undefined }
        if (!result.saveDirs.length) suggestionNote = 'Игра найдена, но сохранений пока нет. Запустите игру или выберите папку вручную.'
        else if (result.saveDirs.length === 1) {
          // Never replace a path the user supplied, including while this request was running.
          if (!originalSave.trim() && savePath === originalSave) { savePath = result.saveDirs[0]; errors.savePath = undefined }
          suggestionNote = 'Проверьте предложенную папку перед добавлением.'
        } else suggestionNote = 'Найдено несколько папок. Выберите нужную; некоторые игры используют несколько папок, а сейчас можно добавить только одну.'
      }
    } catch (e) {
      if (current === request) suggestionError = describeError(e)
    } finally {
      if (current === request) suggesting = false
    }
  }
  async function pick(kind: 'exe' | 'save') {
    picking = true
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open(kind === 'exe'
        ? { title: 'Выберите файл запуска игры', multiple: false, directory: false, filters: [{ name: 'Приложение', extensions: ['exe'] }] }
        : { title: 'Выберите папку сохранений', multiple: false, directory: true }) as string | null
      if (!selected) return
      if (kind === 'exe') { exePath = selected; exeChanged(); await suggest() }
      else { savePath = selected; errors.savePath = undefined }
    } catch (e) { error = describeError(e) }
    finally { picking = false }
  }
  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (busy) return
    errors = {}; error = ''
    if (mode !== 'resolve' && !name.trim()) errors.name = 'Введите название игры'
    if (!ignored) {
      if (!exePath.trim()) errors.exePath = 'Укажите файл запуска игры'
      if (!savePath.trim()) errors.savePath = 'Укажите папку сохранений'
    }
    if (Object.values(errors).some(Boolean)) return
    request++; suggesting = false; saving = true
    try {
      const input = { name: name.trim(), exePath: exePath.trim(), savePath: savePath.trim() }
      if (!ignored) {
        const result = await api.validateGamePaths(input)
        errors = { exePath: result.exePath ?? undefined, savePath: result.savePath ?? undefined }
        if (result.exePath || result.savePath) return
      }
      const result = mode === 'resolve' && game
        ? await api.resolveGamePaths(game.id, action, input)
        : game ? await api.updateGame(game.id, input) : await api.addGame(input)
      onsaved(result)
    } catch (e) { error = describeError(e) }
    finally { saving = false }
  }
</script>

<section class="card game-form" aria-label={mode === 'add' ? 'Добавление игры' : mode === 'edit' ? 'Редактирование игры' : 'Настройка путей'}>
  <h3>{mode === 'add' ? 'Добавить игру' : mode === 'edit' ? `Настройки · ${game?.name}` : `Пути на этом ПК · ${game?.name}`}</h3>
  <p class="muted">{mode === 'resolve' ? 'Укажите, как использовать игру на этом компьютере.' : 'Выберите файл запуска и папку, в которой игра хранит сохранения.'}</p>
  {#if mode === 'edit' && game?.pathScope === 'local'}
    <p class="muted">Изменения путей действуют только на этом ПК.</p>
  {/if}
  <form onsubmit={submit} novalidate>
    <fieldset disabled={busy}>
      {#if mode === 'resolve'}
        <label class="field"><span>Что сделать</span>
          <select bind:value={action} onchange={() => { error = ''; errors = {} }}>
            <option value="useLocal">Указать пути только на этом ПК</option>
            <option value="updateRepository">Изменить пути для всех ПК</option>
            <option value="ignore">Игнорировать игру на этом ПК</option>
          </select>
        </label>
        {#if action === 'updateRepository'}<p class="muted">Изменение путей для всех ПК пока недоступно. Можно указать локальные пути.</p>{/if}
        {#if ignored}<p class="muted">Игра останется в общем списке. Её файлы не изменятся.</p>{/if}
      {:else}
        <label class="field"><span>Название игры</span>
          <input bind:value={name} aria-invalid={!!errors.name} aria-describedby={errors.name ? 'name-error' : undefined} oninput={() => errors.name = undefined} placeholder="Например, Hollow Knight" />
          {#if errors.name}<span id="name-error" class="error">{errors.name}</span>{/if}
        </label>
      {/if}
      {#if !ignored}
        <div class="field">
          <label for="game-exe">Файл запуска игры</label>
          <div class="path-input">
            <input id="game-exe" bind:value={exePath} oninput={exeChanged} onblur={() => { if (!picking) suggest() }} placeholder="C:\Games\Game\game.exe" aria-invalid={!!errors.exePath} aria-describedby={errors.exePath ? 'exe-error' : undefined} />
            <button type="button" onclick={() => pick('exe')} aria-label="Выбрать файл запуска">Обзор…</button>
          </div>
          {#if errors.exePath}<span id="exe-error" class="error">{errors.exePath}</span>{/if}
        </div>
        <div class="field">
          <label for="game-saves">Папка сохранений</label>
          <div class="path-input">
            <input id="game-saves" bind:value={savePath} oninput={() => errors.savePath = undefined} placeholder="C:\Users\Player\Saved Games\Game" aria-invalid={!!errors.savePath} aria-describedby={errors.savePath ? 'save-error' : undefined} />
            <button type="button" onclick={() => pick('save')} aria-label="Выбрать папку сохранений">Обзор…</button>
          </div>
          {#if errors.savePath}<span id="save-error" class="error">{errors.savePath}</span>{/if}
        </div>
        <div class="suggestions" aria-live="polite">
          {#if suggesting}<p class="muted">Ищем папки сохранений…</p>
          {:else if suggestionError}
            <p class="error">{suggestionError}</p>
            <button type="button" onclick={suggest}>Повторить поиск</button>
          {:else if suggestionNote}<p class="muted">{suggestionNote}</p>{/if}
          {#if suggestion?.saveDirs.length}
            <div class="suggestion-options">
              {#each suggestion.saveDirs as path}
                <button type="button" class:selected={savePath === path} onclick={() => { savePath = path; errors.savePath = undefined }}>{path}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button type="button" onclick={() => { request++; oncancel() }}>Отмена</button>
        <button class="primary" disabled={mode === 'resolve' && action === 'updateRepository'}>{saving ? 'Сохраняем…' : mode === 'add' ? 'Добавить игру' : ignored ? 'Игнорировать на этом ПК' : 'Сохранить'}</button>
      </div>
    </fieldset>
  </form>
</section>

<style>
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  .game-form { padding: 16px; }
  h3 { margin: 0 0 8px; }
  .field { max-width: none; }
  .path-input { display: flex; gap: 8px; }
  .path-input input { flex: 1; width: 0; }
  .suggestion-options { display: flex; flex-direction: column; align-items: stretch; gap: 6px; }
  .suggestion-options button { white-space: normal; overflow-wrap: anywhere; text-align: left; }
  .selected { background: var(--accent-soft); border-color: var(--accent); }
  input[aria-invalid='true'] { border-color: var(--danger); }
</style>
