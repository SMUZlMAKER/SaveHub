import test from 'node:test'
import assert from 'node:assert/strict'
import { shortcutFromKey } from '../src/lib/shortcut.ts'
const key = (code, modifiers = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, isComposing: false, ...modifiers })
test('records supported combinations in canonical order', () => {
  assert.equal(shortcutFromKey(key('F9', { ctrlKey: true, altKey: true })), 'Ctrl+Alt+F9')
  assert.equal(shortcutFromKey(key('Digit3', { altKey: true, shiftKey: true })), 'Alt+Shift+3')
})
test('physical letter code is independent of text produced by the layout', () => {
  assert.equal(shortcutFromKey({ ...key('KeyA', { ctrlKey: true }), key: 'ф' }), 'Ctrl+A')
})
test('modifier keys alone wait for a main key', () => {
  assert.equal(shortcutFromKey(key('ControlLeft', { ctrlKey: true })), null)
  assert.equal(shortcutFromKey(key('AltRight', { ctrlKey: true, altKey: true })), null)
})
test('unsupported keys and modifiers are rejected before saving', () => {
  for (const event of [key('KeyA'), key('KeyA', { shiftKey: true }), key('F25', { ctrlKey: true }), key('Numpad1', { ctrlKey: true }), key('KeyA', { ctrlKey: true, metaKey: true }), key('KeyA', { ctrlKey: true, isComposing: true })]) {
    assert.throws(() => shortcutFromKey(event))
  }
})
