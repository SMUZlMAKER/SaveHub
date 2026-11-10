// Всплывающее сообщение внизу окна: результат действия или ошибка.

import { describeError } from './api'

type Notice = { text: string; kind: 'info' | 'error' }

let current = $state<Notice | null>(null)
let timer: ReturnType<typeof setTimeout> | undefined

export const notice = {
  get current() {
    return current
  },
  show(text: string, kind: Notice['kind'] = 'info') {
    current = { text, kind }
    clearTimeout(timer)
    timer = setTimeout(() => (current = null), 5000)
  },
  error(e: unknown) {
    this.show(describeError(e), 'error')
  },
}
