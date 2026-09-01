import type { KeyEvent } from '../input.js'
import type { OverlayState } from '../viewmodel/index.js'
import type { EditorState } from '../input/editor.js'
import { isEditorEmpty } from '../input/editor.js'

export type ReplControlInput = {
  event: KeyEvent
  overlay: OverlayState
  isLoading: boolean
  hasStream: boolean
  editor: EditorState
  exitHint: boolean
  logMode: boolean
  hasQueuedPrompt: boolean
  isCompacting?: boolean
  /**
   * Something is waiting on a background task that ctrl+b can release without
   * killing it: a shell watched in the foreground, or a blocking `task_output`
   * call holding the turn.
   */
  canReclaimTurn?: boolean
}

export type ReplControlAction =
  | { kind: 'restore-queued' }
  | { kind: 'reclaim-turn' }
  | { kind: 'interrupt' }
  | { kind: 'exit' }
  | { kind: 'show-exit-hint' }
  | { kind: 'clear-editor' }
  | { kind: 'close-completion' }
  | { kind: 'clear-exit-hint' }
  | { kind: 'cancel-ask' }
  | { kind: 'clear-selector-query' }
  | { kind: 'close-overlay' }
  | { kind: 'exit-log-mode' }
  | { kind: 'selector-key' }
  | { kind: 'ask-key' }
  | { kind: 'toggle-expanded' }
  | { kind: 'loading-enter' }
  | { kind: 'loading-char' }
  | { kind: 'loading-paste' }
  | { kind: 'normal-key' }

export function decideReplControl(input: ReplControlInput): ReplControlAction[] {
  const { event, overlay, isLoading, hasStream, editor, exitHint, logMode, hasQueuedPrompt, isCompacting, canReclaimTurn } = input
  const actions: ReplControlAction[] = []

  if (isCompacting && (event.type === 'escape' || (event.type === 'ctrl' && event.key === 'c'))) {
    return [{ kind: 'interrupt' }]
  }

  if (event.type === 'ctrl' && event.key === 'c') {
    if (isLoading && hasStream) return [{ kind: 'interrupt' }]
    if (isEditorEmpty(editor)) return [{ kind: exitHint ? 'exit' : 'show-exit-hint' }]
    return [{ kind: 'clear-editor' }]
  }

  // Ctrl+B is the dedicated "stop waiting, keep the work" gesture. Keeping it
  // separate from esc is what makes both unambiguous: esc always kills, ctrl+b
  // never does. Overloading esc meant the same key had two outcomes depending on
  // state the user could not see.
  if (event.type === 'ctrl' && event.key === 'b') {
    if (isLoading && hasStream && canReclaimTurn) return [{ kind: 'reclaim-turn' }]
    return [{ kind: 'normal-key' }]
  }

  if (exitHint) actions.push({ kind: 'clear-exit-hint' })

  if (event.type === 'escape') {
    if (overlay.kind !== 'none') {
      if (overlay.kind === 'ask-user' && hasStream) return actions.concat({ kind: 'cancel-ask' })
      if (overlay.kind === 'selector' && overlay.state.query) return actions.concat({ kind: 'clear-selector-query' })
      return actions.concat({ kind: 'close-overlay' })
    }
    if (editor.completion) return actions.concat({ kind: 'close-completion' })
    if (isLoading && hasStream && hasQueuedPrompt) return actions.concat({ kind: 'restore-queued' })
    // Esc always interrupts. Backgrounding lives on ctrl+b, so neither key's
    // meaning depends on state the user cannot see.
    if (isLoading && hasStream) return actions.concat({ kind: 'interrupt' })
    if (!isEditorEmpty(editor)) return actions.concat({ kind: 'clear-editor' })
    if (logMode) return actions.concat({ kind: 'exit-log-mode' })
    return actions
  }

  if (overlay.kind === 'help') return actions.concat({ kind: 'close-overlay' })
  if (overlay.kind === 'selector') return actions.concat({ kind: 'selector-key' })
  if (overlay.kind === 'ask-user') return actions.concat({ kind: 'ask-key' })

  // ctrl+o toggles expanded view in both loading and idle states
  if (event.type === 'ctrl' && event.key === 'o') return actions.concat({ kind: 'toggle-expanded' })

  if (isLoading) {
    if (event.type === 'enter') return actions.concat({ kind: 'loading-enter' })
    if (event.type === 'char' || event.type === 'shift-char') return actions.concat({ kind: 'loading-char' })
    if (event.type === 'paste') return actions.concat({ kind: 'loading-paste' })
  }

  return actions.concat({ kind: 'normal-key' })
}
