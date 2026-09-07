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
import { decideQueueSelectorAction, type ManagedQueuedPrompt } from './queue-manage.js'
import { toggleSkillGroup } from './skill-window.js'
import { SELECTOR_OWNER } from './selector-identity.js'

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
      // Focus transfer and movement are one gesture: never consume the first
      // navigation key just to blur the composer/filter.
      return { kind: 'update', state: selectorUp(selectorFocusList(disarmDelete(state))) }
    }
    case 'down':
    case 'tab': {
      return { kind: 'update', state: selectorDown(selectorFocusList(disarmDelete(state))) }
    }
    case 'char':
      // Lists that reserve bare letters for their own gestures never build a
      // filter query: doing so would silently drop rows with no filter line on
      // screen to explain why.
      if (state.noFilter || state.owner === SELECTOR_OWNER.queue) return { kind: 'none' }
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
  if (state.owner === SELECTOR_OWNER.skill) {
    const next = toggleSkillGroup(state)
    return next ? { kind: 'update', state: next } : { kind: 'none' }
  }
  // Only explicitly owned actionable lists can dispatch business operations.
  // Skill/background/unknown lists must never fall through to model selection.
  if (state.owner !== SELECTOR_OWNER.model
    && state.owner !== SELECTOR_OWNER.resume
    && state.owner !== SELECTOR_OWNER.queue) return { kind: 'none' }

  const selected = selectorSelect(state)
  if (!selected) return { kind: 'close' }

  if (state.owner === SELECTOR_OWNER.resume) return { kind: 'resume', sessionId: selected.id ?? selected.label }

  if (state.owner === SELECTOR_OWNER.queue) {
    const action = decideQueueSelectorAction(selected, 'enter')
    if (action.kind === 'edit') return { kind: 'queue-edit', entry: action.entry }
    return { kind: 'none' }
  }

  return { kind: 'select-model', spec: selected.id ?? selected.label }
}

function deleteAction(state: SelectorState): SelectorControlAction {
  const target = selectorSelect(state)
  if (!target?.id) return { kind: 'none' }

  if (state.owner === SELECTOR_OWNER.queue) {
    const action = decideQueueSelectorAction(target, 'delete')
    if (action.kind !== 'remove') return { kind: 'none' }
    return {
      kind: 'queue-remove',
      entry: action.entry,
      state: selectorRemoveItem(state, state.focusIndex),
    }
  }

  if (state.owner !== SELECTOR_OWNER.resume) return { kind: 'none' }

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
