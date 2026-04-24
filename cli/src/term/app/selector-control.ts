import type { KeyEvent } from '../input.js'
import {
  selectorBackspace,
  selectorDown,
  selectorRemoveItem,
  selectorSelect,
  selectorType,
  selectorUp,
  type SelectorState,
} from '../selector.js'
import { isResumeSelectorTitle } from './resume.js'

export type SelectorControlAction =
  | { kind: 'update'; state: SelectorState }
  | { kind: 'close' }
  | { kind: 'resume'; sessionId: string }
  | { kind: 'history-goto'; seq: string }
  | { kind: 'history-preview'; label: string; text: string }
  | { kind: 'select-model'; model: string }
  | { kind: 'delete-session'; sessionId: string; label: string; state: SelectorState }
  | { kind: 'none' }

export function handleSelectorControl(state: SelectorState, event: KeyEvent): SelectorControlAction {
  switch (event.type) {
    case 'up':
      return { kind: 'update', state: selectorUp(state) }
    case 'down':
      return { kind: 'update', state: selectorDown(state) }
    case 'char':
      return { kind: 'update', state: selectorType(state, event.char) }
    case 'backspace':
      return { kind: 'update', state: selectorBackspace(state) }
    case 'enter':
      return selectAction(state)
    case 'escape':
      return { kind: 'close' }
    case 'delete':
      return deleteAction(state)
    case 'ctrl':
      return event.key === 'd' ? deleteAction(state) : { kind: 'none' }
    default:
      return { kind: 'none' }
  }
}

function selectAction(state: SelectorState): SelectorControlAction {
  const selected = selectorSelect(state)
  if (!selected) return { kind: 'close' }

  if (isResumeSelectorTitle(state.title)) return { kind: 'resume', sessionId: selected.id ?? selected.label }

  if (state.title.startsWith('History')) {
    if (selected.label === '…') return { kind: 'close' }
    const role = (selected as { role?: string }).role
    if (role === 'assistant') {
      const text = (selected.detail ?? '').replace(/^\s*assistant\s+/, '')
      return { kind: 'history-preview', label: selected.label, text }
    }
    return { kind: 'history-goto', seq: selected.label.replace('#', '') }
  }

  return { kind: 'select-model', model: selected.label }
}

function deleteAction(state: SelectorState): SelectorControlAction {
  if (!isResumeSelectorTitle(state.title)) return { kind: 'none' }
  const target = selectorSelect(state)
  if (!target?.id) return { kind: 'none' }
  return {
    kind: 'delete-session',
    sessionId: target.id,
    label: target.label,
    state: selectorRemoveItem(state, state.focusIndex),
  }
}
