<script lang="ts">
  import { onMount } from 'svelte'
  import { api, describeError } from '../api'
  import { notice } from '../notice.svelte'
  let shortcut = $state('')
  let error = $state('')
  let loading = $state(true)
  let saving = $state(false)
  async function load() {
    loading = true; error = ''
    try { const preferences = await api.getManualSavePreferences(); shortcut = preferences.shortcut; error = preferences.error ?? '' }
    catch (e) { error = describeError(e) }
    finally { loading = false }
  }
  async function save(event: SubmitEvent) {
    event.preventDefault()
    if (saving || loading) return
    saving = true; error = ''
    try { await api.saveManualSaveShortcut(shortcut); notice.show('Сочетание для ручного снимка сохранено') }
    catch (e) { error = describeError(e) }
    finally { saving = false }
  }
  onMount(load)
</script>
<section aria-labelledby="manual-save-title">
  <h3 id="manual-save-title">Ручные снимки во время игры</h3>
  <p class="muted">SaveHub копирует файлы, уже записанные игрой на диск. Это не команда сохранения внутри игры. Занятые записью файлы не копируются — повторите попытку после завершения записи.</p>
  <form onsubmit={save}>
    <label class="field"><span>Глобальное сочетание клавиш</span><input bind:value={shortcut} disabled={loading || saving} placeholder="Ctrl+Alt+F8" /></label>
    <p class="muted">Ctrl или Alt, необязательный Shift и клавиша A–Z, 0–9 или F1–F24. Пустое поле отключает сочетание. Оно работает и при свёрнутом хабе, если запущена ровно одна игра. При нескольких играх используйте кнопку рядом с нужной игрой.</p>
    <button disabled={loading || saving}>{saving ? 'Сохраняем…' : 'Сохранить сочетание'}</button>
    {#if error}<p class="error" role="alert">{error}</p><button type="button" onclick={load} disabled={saving}>Перечитать настройку</button>{/if}
  </form>
  <p class="muted">Результат появится поверх игры без перехвата фокуса и кликов. Для эксклюзивного полноэкранного режима используйте оконный режим без рамки, если уведомление не видно.</p>
</section>
