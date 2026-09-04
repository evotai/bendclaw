import type { KeyEvent } from '../input.js'
import {
  selectorBackspace,
  selectorDown,
  selectorFocusList,
  selectorRemoveItem,
  selectorSelect,
  selectorType,
  selectorUp,
  type SelectorState,
} from '../selector.js'
import { decideQueueSelectorAction, isQueueSelectorTitle, type ManagedQueuedPrompt } from './queue-manage.js'
import { isBackgroundPanelTitle } from './background-panel.js'
import { isResumeSelectorTitle } from './resume.js'
import { isSkillSelectorTitle } from './skill-window.js'

export type SelectorControlAction =
  | { kind: 'update'; state: SelectorState }
  | { kind: 'close' }
  | { kind: 'resume'; sessionId: string }
  | { kind: 'select-model'; spec: string }
  | { kind: 'delete-session'; sessionId: string; label: string; state: SelectorState }
  | { kind: 'queue-edit'; entry: ManagedQueuedPrompt }
  | { kind: 'queue-remove'; entry: ManagedQueuedPrompt; state: SelectorState }
  | { kind: 'none' }

const RESUME_DELETE_CONFIRM = 'Press Ctrl+D / Del again to delete'

/** Drop an armed delete so a stray confirming keypress cannot delete a session. */
function disarmDelete(state: SelectorState): SelectorState {
  if (state.pendingDeleteId === undefined) return state
  const subtitle = state.subtitle === RESUME_DELETE_CONFIRM ? undefined : state.subtitle
  return { ...state, pendingDeleteId: undefined, subtitle }
}

export function handleSelectorControl(state: SelectorState, event: KeyEvent): SelectorControlAction {
  switch (event.type) {
    case 'up':
    case 'shift-tab': {
      const ready = disarmDelete(state)
      // Every command window behaves the same way: the composer keeps focus
      // while the window previews, and the first arrow hands focus to the list.
      // Keying this off `listFocused` rather than a per-selector title keeps
      // `/model`, `/skill`, and `/resume` from each inventing their own rule.
      if (ready.listFocused === false) {
        return { kind: 'update', state: selectorFocusList(ready) }
      }
      return { kind: 'update', state: selectorUp(ready) }
    }
    case 'down':
    case 'tab': {
      const ready = disarmDelete(state)
      if (ready.listFocused === false) {
        return { kind: 'update', state: selectorFocusList(ready) }
      }
      return { kind: 'update', state: selectorDown(ready) }
    }
    case 'char':
      // Lists that reserve bare letters for their own gestures never build a
      // filter query: doing so would silently drop rows with no filter line on
      // screen to explain why.
      if (state.noFilter || isQueueSelectorTitle(state.title)) return { kind: 'none' }
      return { kind: 'update', state: selectorType(disarmDelete(state), event.char) }
    case 'backspace':
      if (state.noFilter) return { kind: 'none' }
      return { kind: 'update', state: selectorBackspace(disarmDelete(state)) }
    case 'enter':
      return selectAction(disarmDelete(state))
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
  // The live skill inventory is informational. Management remains explicit via
  // `/skill list|install|update|remove`, so Enter must never reinterpret a
  // skill name as a model spec or execute a destructive action.
  if (isSkillSelectorTitle(state.title)) return { kind: 'none' }

  const selected = selectorSelect(state)
  if (!selected) return { kind: 'close' }

  if (isResumeSelectorTitle(state.title)) return { kind: 'resume', sessionId: selected.id ?? selected.label }

  // The panel owns `enter` (view output) in its own handler. Reaching here means
  // there was nothing to act on, so fall through to nothing rather than letting
  // the default branch read the row as a model spec.
  if (isBackgroundPanelTitle(state.title)) return { kind: 'none' }

  if (isQueueSelectorTitle(state.title)) {
    const action = decideQueueSelectorAction(selected, 'enter')
    if (action.kind === 'edit') return { kind: 'queue-edit', entry: action.entry }
    return { kind: 'none' }
  }

  return { kind: 'select-model', spec: selected.id ?? selected.label }
}

function deleteAction(state: SelectorState): SelectorControlAction {
  const target = selectorSelect(state)
  if (!target?.id) return { kind: 'none' }

  if (isQueueSelectorTitle(state.title)) {
    const action = decideQueueSelectorAction(target, 'delete')
    if (action.kind !== 'remove') return { kind: 'none' }
    return {
      kind: 'queue-remove',
      entry: action.entry,
      state: selectorRemoveItem(state, state.focusIndex),
    }
  }

  if (!isResumeSelectorTitle(state.title)) return { kind: 'none' }

  // Deleting a session is irreversible, so the first press only arms it and a
  // second press confirms. The armed id must still be the focused row: an async
  // list refresh (listSessionsWithText) can reorder rows between the two
  // presses, and matching on index alone would delete the wrong session.
  if (state.pendingDeleteId === target.id) {
    return {
      kind: 'delete-session',
      sessionId: target.id,
      label: target.label,
      state: selectorRemoveItem({ ...state, subtitle: undefined }, state.focusIndex),
    }
  }

  return {
    kind: 'update',
    state: {
      ...state,
      listFocused: true,
      pendingDeleteId: target.id,
      subtitle: RESUME_DELETE_CONFIRM,
    },
  }
}
