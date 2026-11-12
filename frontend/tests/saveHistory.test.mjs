import test from 'node:test'
import assert from 'node:assert/strict'
import { filterHistory, groupHistory, sessionPeriod } from '../src/lib/saveHistory.ts'
const save = (id, extra = {}) => ({ id, gameId: 'game', gameName: 'Игра', profileId: 'p1', profileName: 'Один', kind: 'afterSession', createdAt: new Date(2026, 10, 12, 12).getTime(), sizeBytes: 1, sessionId: null, sessionStartedAt: null, sessionEndedAt: null, ...extra })
test('filters combine game, profile, reason and inclusive local dates', () => {
  const saves = [save('a'), save('b', { profileId: 'p2' })]
  assert.deepEqual(filterHistory(saves, { gameId: 'game', profileId: 'p1', kind: 'afterSession', from: '2026-11-12', to: '2026-11-12' }).map(s => s.id), ['a'])
})
test('sessions remain distinct and legacy records have a separate group', () => {
  const groups = groupHistory([save('a', { sessionId: 's1' }), save('b', { sessionId: 's2' }), save('c')], 'session')
  assert.equal(groups.length, 3)
  assert.equal(groups.find(group => group.key === 'unknown').title, 'Без данных о сессии')
})
test('profiles with identical names remain distinct', () => {
  assert.equal(groupHistory([save('a'), save('b', { profileId: 'p2' })], 'profile').length, 2)
})
test('session period uses recorded process times rather than snapshot time', () => {
  const rows = [save('a', { sessionStartedAt: 1000, sessionEndedAt: 2000 })]
  assert.equal(sessionPeriod(rows), `${new Date(1000).toLocaleString('ru-RU')} — ${new Date(2000).toLocaleString('ru-RU')}`)
})

test('one date direction sorts sessions by end and their snapshots by creation', () => {
  const saves = [
    save('a-late', { sessionId: 'a', sessionEndedAt: 100, createdAt: 999 }),
    save('b-late', { sessionId: 'b', sessionEndedAt: 200, createdAt: 400 }),
    save('b-early', { sessionId: 'b', sessionEndedAt: 200, createdAt: 300 }),
    save('a-early', { sessionId: 'a', sessionEndedAt: 100, createdAt: 500 }),
  ]
  const newest = groupHistory(saves, 'session', 'newest')
  assert.deepEqual(newest.map(group => group.key), ['b', 'a'])
  assert.deepEqual(newest[0].saves.map(save => save.id), ['b-late', 'b-early'])
  const oldest = groupHistory(saves, 'session', 'oldest')
  assert.deepEqual(oldest.map(group => group.key), ['a', 'b'])
  assert.deepEqual(oldest[0].saves.map(save => save.id), ['a-early', 'a-late'])
  assert.equal(saves[0].id, 'a-late')
})
test('game and profile groups sort by latest snapshot in either direction', () => {
  const saves = [save('old', { gameId: 'old', profileId: 'old', createdAt: 10 }), save('new', { gameId: 'new', profileId: 'new', createdAt: 30 }), save('middle', { gameId: 'old', profileId: 'old', createdAt: 20 })]
  for (const grouping of ['game', 'profile']) {
    assert.deepEqual(groupHistory(saves, grouping, 'newest').map(group => group.key), ['new', 'old'])
    assert.deepEqual(groupHistory(saves, grouping, 'oldest').map(group => group.key), ['old', 'new'])
  }
  assert.deepEqual(groupHistory(saves, 'none', 'oldest')[0].saves.map(save => save.id), ['old', 'middle', 'new'])
})
test('active sessions and legacy groups use their latest snapshot date', () => {
  const saves = [save('legacy', { createdAt: 10 }), save('active', { sessionId: 'active', createdAt: 30 }), save('completed', { sessionId: 'completed', sessionEndedAt: 20, createdAt: 40 })]
  assert.deepEqual(groupHistory(saves, 'session', 'newest').map(group => group.key), ['active', 'completed', 'unknown'])
})

test('session period takes earliest start and latest end regardless of snapshot order', () => {
  const rows = [
    save('middle', { sessionId: 'session', createdAt: 20, sessionStartedAt: 1000, sessionEndedAt: 3000 }),
    save('start', { sessionId: 'session', createdAt: 10, sessionStartedAt: 0, sessionEndedAt: null }),
    save('end', { sessionId: 'session', createdAt: 30, sessionStartedAt: null, sessionEndedAt: 4000 }),
  ]
  const expected = `${new Date(0).toLocaleString('ru-RU')} — ${new Date(4000).toLocaleString('ru-RU')}`
  for (const input of [rows, [...rows].reverse(), [rows[2], rows[0], rows[1]]]) {
    assert.equal(sessionPeriod(input), expected)
    for (const order of ['newest', 'oldest']) {
      const [group] = groupHistory(input, 'session', order)
      assert.equal(group.period, expected)
      assert.equal(group.date, 4000)
    }
  }
})
test('missing session bounds preserve unknown and active session labels', () => {
  assert.equal(sessionPeriod([save('unknown')]), 'Период игры неизвестен')
  const rows = [save('later', { sessionStartedAt: 1000 }), save('earlier', { sessionStartedAt: 0 })]
  assert.equal(sessionPeriod(rows), `${new Date(0).toLocaleString('ru-RU')} — сессия ещё не завершена`)
})
