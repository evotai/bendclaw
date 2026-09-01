import { describe, expect, test } from 'bun:test'
import { decideReplControl } from '../src/term/app/repl-control.js'
import { createEditorState, insertText } from '../src/term/input/editor.js'
import { createSelectorState } from '../src/term/selector.js'

const editor = createEditorState()
const textEditor = insertText(createEditorState(), 'hello')
const completionEditor = {
  ...editor,
  completion: {
    items: [{ label: '/help', value: '/help ', description: 'Show help' }],
    selectedIndex: 0,
    replaceStart: 0,
    replaceEnd: 2,
  },
}
const none = { kind: 'none' as const }
const help = { kind: 'help' as const }
const selector = { kind: 'selector' as const, state: createSelectorState('T', [{ label: 'one' }]) }
const selectorWithQuery = { kind: 'selector' as const, state: createSelectorState('T', [{ label: 'one' }], undefined, 'o') }
const askUser = { kind: 'ask-user' as const, state: { questions: [], currentIndex: 0, answers: {} } as any }

const kinds = (input: Parameters<typeof decideReplControl>[0]) => decideReplControl(input).map(a => a.kind)
const base = { overlay: none, isLoading: false, hasStream: false, editor, exitHint: false, logMode: false, hasQueuedPrompt: false }

describe('repl control', () => {
  test('ctrl-c interrupts loading stream', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, isLoading: true, hasStream: true })).toEqual(['interrupt'])
  })

  test('escape cancels manual compaction without requiring a query stream', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, isLoading: true, isCompacting: true })).toEqual(['interrupt'])
  })

  test('ctrl-c cancels manual compaction before exit handling', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, isLoading: true, isCompacting: true })).toEqual(['interrupt'])
  })

  test('ctrl-c shows exit hint on empty editor', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' } })).toEqual(['show-exit-hint'])
  })

  test('ctrl-c exits when exit hint is already visible', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, exitHint: true })).toEqual(['exit'])
  })

  test('ctrl-c clears non-empty editor', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, editor: textEditor })).toEqual(['clear-editor'])
  })

  test('non ctrl-c clears exit hint then continues', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, exitHint: true })).toEqual(['clear-exit-hint', 'normal-key'])
  })

  test('escape cancels ask overlay with stream', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: askUser, hasStream: true })).toEqual(['cancel-ask'])
  })

  test('ctrl-c during an ask overlay routes to interrupt, not cancel-ask', () => {
    // Regression guard: an ask/plan-review overlay is only shown while a run is
    // loading, so Ctrl+C hits the loading-stream interrupt branch before the
    // overlay checks. Both `interrupt` and `cancel-ask` must resolve the
    // pending ask promise or the suspended host-tool dispatch hangs the loop.
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, overlay: askUser, isLoading: true, hasStream: true })).toEqual(['interrupt'])
  })

  test('escape clears selector query before closing overlay', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: selectorWithQuery })).toEqual(['clear-selector-query'])
  })

  test('escape closes overlay without query', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: selector })).toEqual(['close-overlay'])
  })

  test('escape closes completion before clearing input', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, editor: completionEditor })).toEqual(['close-completion'])
  })

  test('escape restores newest queued prompt without interrupting', () => {
    expect(kinds({
      ...base,
      event: { type: 'escape' },
      isLoading: true,
      hasStream: true,
      hasQueuedPrompt: true,
    })).toEqual(['restore-queued'])
  })

  test('escape interrupts loading stream without queued prompts', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, isLoading: true, hasStream: true })).toEqual(['interrupt'])
  })

  test('escape always interrupts, even with work that could be backgrounded', () => {
    // The two gestures are kept separate on purpose: esc means kill regardless
    // of state, so its outcome never depends on something the user cannot see.
    // Backgrounding lives on ctrl+b.
    expect(kinds({
      ...base,
      event: { type: 'escape' },
      isLoading: true,
      hasStream: true,
      canReclaimTurn: true,
    })).toEqual(['interrupt'])
  })

  test('ctrl-b backgrounds the running work instead of killing it', () => {
    expect(kinds({
      ...base,
      event: { type: 'ctrl', key: 'b' },
      isLoading: true,
      hasStream: true,
      canReclaimTurn: true,
    })).toEqual(['reclaim-turn'])
  })

  test('ctrl-b falls through to the editor when there is nothing to background', () => {
    // It must never become a second interrupt: this key is non-destructive by
    // contract, so with nothing running it is just an ordinary keypress.
    expect(kinds({
      ...base,
      event: { type: 'ctrl', key: 'b' },
      isLoading: true,
      hasStream: true,
      canReclaimTurn: false,
    })).toEqual(['normal-key'])
  })

  test('ctrl-b is inert while idle', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'b' } })).toEqual(['normal-key'])
  })

  test('a queued prompt still outranks interrupting on escape', () => {
    // Pulling back text the user typed is the more recent intent.
    expect(kinds({
      ...base,
      event: { type: 'escape' },
      isLoading: true,
      hasStream: true,
      hasQueuedPrompt: true,
      canReclaimTurn: true,
    })).toEqual(['restore-queued'])
  })

  test('ctrl-c still interrupts immediately while work is backgroundable', () => {
    // Both stop gestures stay unambiguous; only ctrl+b is the soft one.
    expect(kinds({
      ...base,
      event: { type: 'ctrl', key: 'c' },
      isLoading: true,
      hasStream: true,
      canReclaimTurn: true,
    })).toEqual(['interrupt'])
  })

  test('compaction escape interrupts regardless of waited-on work', () => {
    expect(kinds({
      ...base,
      event: { type: 'escape' },
      isLoading: true,
      isCompacting: true,
      canReclaimTurn: true,
    })).toEqual(['interrupt'])
  })

  test('escape clears editor before exiting log mode', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, editor: textEditor, logMode: true })).toEqual(['clear-editor'])
  })

  test('escape exits log mode when editor is empty', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, logMode: true })).toEqual(['exit-log-mode'])
  })

  test('help overlay closes on any key', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, overlay: help })).toEqual(['close-overlay'])
  })

  test('selector overlay delegates key', () => {
    expect(kinds({ ...base, event: { type: 'down' }, overlay: selector })).toEqual(['selector-key'])
  })

  test('down reaches the editor while loading, so ↓ can open the task panel', () => {
    // Shells usually run mid-turn. If loading swallowed ↓ the way it swallows
    // typed characters, the gesture the prompt advertises would never fire.
    expect(kinds({ ...base, event: { type: 'down' }, isLoading: true, hasStream: true }))
      .toEqual(['normal-key'])
  })

  test('ask overlay delegates key', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'y' }, overlay: askUser })).toEqual(['ask-key'])
  })

  test('loading enter/char/paste have loading actions', () => {
    expect(kinds({ ...base, event: { type: 'enter' }, isLoading: true })).toEqual(['loading-enter'])
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, isLoading: true })).toEqual(['loading-char'])
    expect(kinds({ ...base, event: { type: 'paste', text: 'x' }, isLoading: true })).toEqual(['loading-paste'])
  })

  test('ctrl-o toggles expanded view while loading', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'o' }, isLoading: true })).toEqual(['toggle-expanded'])
  })

  test('loading movement falls through to normal key', () => {
    expect(kinds({ ...base, event: { type: 'left' }, isLoading: true })).toEqual(['normal-key'])
  })
})
