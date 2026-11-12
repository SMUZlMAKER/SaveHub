<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { api, describeError, subscribe } from '../api'
  import { notice } from '../notice.svelte'
  import { shortcutFromKey } from '../shortcut'
  let shortcut = $state('')
  let error = $state('')
  let loading = $state(true)
  let saving = $state(false)
  let recording = $state(false)
  let arming = $state(false)
  let input: HTMLInputElement
  let ready = Promise.resolve()
  let disposed = false
  let generation = 0

  async function load() {
    loading = true; error = ''
    try { const preferences = await api.getManualSavePreferences(); shortcut = preferences.shortcut; error = preferences.error ?? '' }
    catch (e) { error = describeError(e) }
    finally { loading = false }
  }
  async function stopRecording() {
    generation++
    recording = false
    arming = false
    try { await api.setShortcutRecording(false) }
    catch (e) { if (!disposed) error = describeError(e) }
  }
  async function startRecording() {
    if (loading || saving || arming) return
    const current = ++generation
    arming = true; error = ''
    try {
      await ready
      await api.setShortcutRecording(true)
      if (disposed || current !== generation) { await api.setShortcutRecording(false); return }
      recording = true
      arming = false
      await tick()
      if (!disposed && current === generation && recording) input.focus()
    } catch (e) { error = describeError(e) }
    finally { arming = false }
  }
  function accept(value: string) {
    if (!recording) return
    shortcut = value
    error = ''
    void stopRecording()
  }
  function capture(event: KeyboardEvent) {
    if (!recording) return
    if (event.code === 'Tab' && !event.ctrlKey && !event.altKey && !event.metaKey) return
    event.preventDefault()
    event.stopPropagation()
    if (event.repeat) return
    if (event.code === 'Escape') { void stopRecording(); return }
    try {
      const value = shortcutFromKey(event)
      if (value) accept(value)
    } catch (e) { error = e instanceof Error ? e.message : describeError(e) }
  }
  async function save(event: SubmitEvent) {
    event.preventDefault()
    if (saving || loading || recording || arming) return
    saving = true; error = ''
    try { await api.saveManualSaveShortcut(shortcut); notice.show('Сочетание для ручного снимка сохранено') }
    catch (e) { error = describeError(e) }
    finally { saving = false }
  }
  onMount(() => {
    load()
    const stop = subscribe<string>('shortcut-recorded', accept)
    ready = stop.ready
    return () => { disposed = true; stop(); void stopRecording() }
  })
</script>
<section aria-labelledby="manual-save-title">
  <h3 id="manual-save-title">Ручные снимки во время игры</h3>
  <p class="muted">SaveHub копирует файлы, уже записанные игрой на диск. Это не команда сохранения внутри игры. Занятые записью файлы не копируются — повторите попытку после завершения записи.</p>
  <form onsubmit={save}>
    <label for="manual-save-shortcut" class="muted">Глобальное сочетание клавиш</label>
    <div class="shortcut-row">
      <input id="manual-save-shortcut" bind:this={input} class="grow" readonly
        value={arming ? 'Подготовка…' : recording ? 'Нажмите сочетание…' : shortcut || 'Отключено'}
        disabled={loading || saving || arming} onkeydown={capture}
        onblur={() => { if (recording) void stopRecording() }} aria-describedby="shortcut-hint" />
      <button type="button" disabled={loading || saving || arming} onpointerdown={(event) => { if (recording) event.preventDefault() }} onclick={() => recording ? stopRecording() : startRecording()}>{recording ? 'Отмена' : 'Записать'}</button>
      <button type="button" disabled={loading || saving || recording || arming || !shortcut} onclick={() => { shortcut = ''; error = '' }}>Отключить</button>
      <button class="primary" disabled={loading || saving || recording || arming}>{saving ? 'Сохраняем…' : 'Сохранить'}</button>
    </div>
    <p id="shortcut-hint" class="muted" aria-live="polite">{recording ? 'Нажмите сочетание. Esc — отмена. Оно начнёт действовать после сохранения.' : 'Нажмите «Записать» и зажмите Ctrl или Alt, необязательный Shift и клавишу A–Z, 0–9 или F1–F24. Для отключения выберите «Отключить» и сохраните.'}</p>
    {#if error}<p class="error" role="alert">{error}</p><button type="button" onclick={load} disabled={saving || recording || arming}>Перечитать настройку</button>{/if}
  </form>
  <p class="muted">Сочетание работает и при свёрнутом хабе, если запущена ровно одна игра. При нескольких играх используйте кнопку рядом с нужной игрой. Результат появится поверх игры без перехвата фокуса и кликов.</p>
</section>
<style>
  .shortcut-row { display: flex; align-items: center; gap: 8px; margin: 4px 0 8px; }
  .shortcut-row input { width: 0; min-width: 100px; }
  @media (max-width: 520px) { .shortcut-row { flex-wrap: wrap; } .shortcut-row input { flex-basis: 100%; } }
</style>
