import test from 'node:test'
import assert from 'node:assert/strict'
import { filterHistory, groupHistory, sessionPeriod } from '../src/lib/saveHistory.ts'
const save = (id, extra = {}) => ({ id, gameId: 'game', gameName: 'Игра', profileId: 'p1', profileName: 'Один', kind: 'afterSession', createdAt: new Date(2026, 10, 12, 12).getTime(), sizeBytes: 1, sessionId: null, sessionStartedAt: null, sessionEndedAt: null, ...extra })
test('filters combine search, profile, reason and inclusive local dates', () => {
  const saves = [save('a'), save('b', { profileId: 'p2' })]
  assert.deepEqual(filterHistory(saves, { query: 'ИГРА', profileId: 'p1', kind: 'afterSession', from: '2026-11-12', to: '2026-11-12' }).map(s => s.id), ['a'])
})
test('sessions remain distinct and legacy records have a separate group', () => {
  const groups = groupHistory([save('a', { sessionId: 's1' }), save('b', { sessionId: 's2' }), save('c')], 'session')
  assert.equal(groups.length, 3)
  assert.equal(groups[2].title, 'Без данных о сессии')
})
test('profiles with identical names remain distinct', () => {
  assert.equal(groupHistory([save('a'), save('b', { profileId: 'p2' })], 'profile').length, 2)
})
test('session period uses recorded process times rather than snapshot time', () => {
  const rows = [save('a', { sessionStartedAt: 1000, sessionEndedAt: 2000 })]
  assert.equal(sessionPeriod(rows), `${new Date(1000).toLocaleString('ru-RU')} — ${new Date(2000).toLocaleString('ru-RU')}`)
})
