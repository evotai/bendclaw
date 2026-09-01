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
   * Something is waiting on a background task that esc can release without
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

  if (exitHint) actions.push({ kind: 'clear-exit-hint' })

  if (event.type === 'escape') {
    if (overlay.kind !== 'none') {
      if (overlay.kind === 'ask-user' && hasStream) return actions.concat({ kind: 'cancel-ask' })
      if (overlay.kind === 'selector' && overlay.state.query) return actions.concat({ kind: 'clear-selector-query' })
      return actions.concat({ kind: 'close-overlay' })
    }
    if (editor.completion) return actions.concat({ kind: 'close-completion' })
    if (isLoading && hasStream && hasQueuedPrompt) return actions.concat({ kind: 'restore-queued' })
    // Waiting on a background task is released before esc is allowed to mean
    // "interrupt". Killing was previously the only way to reclaim the turn,
    // which threw away however far a build or test run had already got.
    // Escalation is by repeat: once nothing is waiting, the next esc interrupts
    // as it always did.
    if (isLoading && hasStream && canReclaimTurn) {
      return actions.concat({ kind: 'reclaim-turn' })
    }
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
