import type { SaveHistoryEntry, SaveKind } from './types.ts'

export const kindLabel: Record<SaveKind, string> = {
  beforeSwitch: 'Перед переключением профиля',
  afterSession: 'После игровой сессии',
  conflict: 'Вторая версия при конфликте',
  beforeRestore: 'Перед применением снимка',
  fileChanged: 'Изменение файлов',
  manual: 'Ручной снимок',
}
export type Grouping = 'session' | 'game' | 'profile' | 'day' | 'none'
export type HistorySort = 'newest' | 'oldest'
export interface HistoryFilters {
  gameId: string | null; profileId: string; kind: SaveKind | ''; from: string; to: string
}
export function filterHistory(saves: SaveHistoryEntry[], filters: HistoryFilters) {
  return saves.filter(save => {
    const day = localDay(save.createdAt)
    return (!filters.gameId || save.gameId === filters.gameId)
      && (!filters.profileId || save.profileId === filters.profileId)
      && (!filters.kind || save.kind === filters.kind)
      && (!filters.from || day >= filters.from)
      && (!filters.to || day <= filters.to)
  })
}
function localDay(time: number) {
  const date = new Date(time)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}
function sessionBounds(saves: SaveHistoryEntry[]) {
  let start: number | null = null
  let end: number | null = null
  for (const save of saves) {
    if (save.sessionStartedAt != null) start = Math.min(start ?? save.sessionStartedAt, save.sessionStartedAt)
    if (save.sessionEndedAt != null) end = Math.max(end ?? save.sessionEndedAt, save.sessionEndedAt)
  }
  return { start, end }
}
export function sessionPeriod(saves: SaveHistoryEntry[]) {
  const { start, end } = sessionBounds(saves)
  if (start == null) return 'Период игры неизвестен'
  const format = (time: number) => new Date(time).toLocaleString('ru-RU')
  return `${format(start)} — ${end == null ? 'сессия ещё не завершена' : format(end)}`
}
export function groupHistory(saves: SaveHistoryEntry[], grouping: Grouping, order: HistorySort = 'newest') {
  const direction = order === 'oldest' ? 1 : -1
  const ordered = [...saves].sort((a, b) => direction * (a.createdAt - b.createdAt || a.id.localeCompare(b.id)))
  const groups = new Map<string, { key: string; title: string; period: string; date: number; saves: SaveHistoryEntry[] }>()
  for (const save of ordered) {
    const key = grouping === 'session' ? save.sessionId ?? 'unknown'
      : grouping === 'game' ? save.gameId
      : grouping === 'profile' ? save.profileId
      : grouping === 'day' ? localDay(save.createdAt) : 'all'
    if (!groups.has(key)) groups.set(key, {
      key,
      title: grouping === 'session' ? save.sessionId ? `${save.gameName} · ${save.profileName}` : 'Без данных о сессии'
        : grouping === 'game' ? save.gameName : grouping === 'profile' ? save.profileName
        : grouping === 'day' ? localDay(save.createdAt) : 'Все снимки',
      period: '', date: 0, saves: [],
    })
    const group = groups.get(key)!
    group.saves.push(save)
    group.date = Math.max(group.date, save.createdAt)
  }
  for (const group of groups.values()) {
    if (grouping === 'session' && group.key !== 'unknown') {
      group.period = sessionPeriod(group.saves)
      const { end } = sessionBounds(group.saves)
      if (end != null) group.date = end
    }
  }
  return [...groups.values()].sort((a, b) => direction * (a.date - b.date || a.key.localeCompare(b.key)))
}
