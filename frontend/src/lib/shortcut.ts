/** Physical letter/digit codes keep shortcuts stable across keyboard layouts. */
export function shortcutFromKey(event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey' | 'isComposing'>): string | null {
  if (/^(Control|Alt|Shift|Meta)(Left|Right)$/.test(event.code)) return null
  if (event.metaKey || event.isComposing) throw new Error('Используйте Ctrl или Alt, необязательный Shift и одну клавишу A–Z, 0–9 или F1–F24.')
  const key = /^(Key[A-Z]|Digit[0-9])$/.test(event.code) ? event.code.replace(/^(Key|Digit)/, '')
    : /^F([1-9]|1[0-9]|2[0-4])$/.test(event.code) ? event.code : null
  if (!key || (!event.ctrlKey && !event.altKey)) throw new Error('Нажмите Ctrl или Alt вместе с клавишей A–Z, 0–9 или F1–F24.')
  return [event.ctrlKey && 'Ctrl', event.altKey && 'Alt', event.shiftKey && 'Shift', key].filter(Boolean).join('+')
}
