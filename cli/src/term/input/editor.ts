import { complete, getGhostHint } from '../../commands/completion.js'
import { needsContinuation } from './continuation.js'

export interface EditorState {
  lines: string[]
  cursorLine: number
  cursorCol: number
  ghostHint: string
  completionCandidates: string[]
}

/** Check if current editor content needs continuation (unclosed fence, trailing backslash). */
export function editorNeedsContinuation(state: EditorState): boolean {
  return needsContinuation(getEditorText(state))
}

export interface HistoryState {
  entries: string[]
  index: number
  savedInput: string
}

export interface CompletionApplyResult {
  applied: boolean
  candidates: string[]
}

export function createEditorState(): EditorState {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    ghostHint: '',
    completionCandidates: [],
  }
}

export function getEditorText(state: EditorState): string {
  return state.lines.join('\n')
}

export function isEditorEmpty(state: EditorState): boolean {
  return state.lines.length === 1 && state.lines[0] === ''
}

export function clearEditor(state: EditorState): EditorState {
  return {
    ...state,
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    ghostHint: '',
    completionCandidates: [],
  }
}

export function insertText(state: EditorState, text: string): EditorState {
  const insertedLines = text.split('\n')
  const newLines = [...state.lines]
  const currentLine = newLines[state.cursorLine]!
  const before = currentLine.slice(0, state.cursorCol)
  const after = currentLine.slice(state.cursorCol)

  if (insertedLines.length === 1) {
    newLines[state.cursorLine] = before + insertedLines[0] + after
    return withCompletionsCleared({
      ...state,
      lines: newLines,
      cursorCol: state.cursorCol + insertedLines[0]!.length,
    })
  }

  const first = before + insertedLines[0]!
  const last = insertedLines[insertedLines.length - 1]! + after
  const middle = insertedLines.slice(1, -1)
  newLines.splice(state.cursorLine, 1, first, ...middle, last)
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine + insertedLines.length - 1,
    cursorCol: insertedLines[insertedLines.length - 1]!.length,
  })
}

export function backspace(state: EditorState): EditorState {
  if (state.cursorCol > 0) {
    const newLines = [...state.lines]
    const currentLine = newLines[state.cursorLine]!
    newLines[state.cursorLine] = currentLine.slice(0, state.cursorCol - 1) + currentLine.slice(state.cursorCol)
    return withCompletionsCleared({
      ...state,
      lines: newLines,
      cursorCol: state.cursorCol - 1,
    })
  }

  if (state.cursorLine === 0) return state

  const newLines = [...state.lines]
  const prevLine = newLines[state.cursorLine - 1]!
  const currentLine = newLines[state.cursorLine]!
  newLines.splice(state.cursorLine - 1, 2, prevLine + currentLine)
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine - 1,
    cursorCol: prevLine.length,
  })
}

export function moveLeft(state: EditorState): EditorState {
  if (state.cursorCol > 0) {
    return withoutGhost({ ...state, cursorCol: state.cursorCol - 1 })
  }
  if (state.cursorLine > 0) {
    return withoutGhost({
      ...state,
      cursorLine: state.cursorLine - 1,
      cursorCol: state.lines[state.cursorLine - 1]!.length,
    })
  }
  return state
}

export function moveRight(state: EditorState): EditorState {
  const lineLen = state.lines[state.cursorLine]!.length
  if (state.cursorCol < lineLen) {
    return withoutGhost({ ...state, cursorCol: state.cursorCol + 1 })
  }
  if (state.cursorLine < state.lines.length - 1) {
    return withoutGhost({ ...state, cursorLine: state.cursorLine + 1, cursorCol: 0 })
  }
  return state
}

export function moveHome(state: EditorState): EditorState {
  return withoutGhost({ ...state, cursorCol: 0 })
}

export function moveEnd(state: EditorState): EditorState {
  return withoutGhost({ ...state, cursorCol: state.lines[state.cursorLine]!.length })
}

export function applyCompletion(state: EditorState): CompletionApplyResult & { state: EditorState } {
  const currentLine = state.lines[state.cursorLine]!
  const result = complete(currentLine, state.cursorCol)
  if (!result) {
    return { state, applied: false, candidates: [] }
  }

  const before = currentLine.slice(0, result.wordStart)
  const after = currentLine.slice(state.cursorCol)
  const newLine = before + result.replacement + after
  const newLines = [...state.lines]
  newLines[state.cursorLine] = newLine

  return {
    state: {
      ...state,
      lines: newLines,
      cursorCol: before.length + result.replacement.length,
      ghostHint: '',
      completionCandidates: result.candidates,
    },
    applied: true,
    candidates: result.candidates,
  }
}

export function refreshGhostHint(state: EditorState): EditorState {
  const currentLine = state.lines[state.cursorLine]!
  return {
    ...state,
    ghostHint: getGhostHint(currentLine, state.cursorCol) ?? '',
  }
}

export function createHistoryState(entries: string[]): HistoryState {
  return {
    entries,
    index: entries.length,
    savedInput: '',
  }
}

export function pushHistory(state: HistoryState, entry: string): HistoryState {
  return {
    entries: [...state.entries, entry],
    index: state.entries.length + 1,
    savedInput: '',
  }
}

export function historyPrev(history: HistoryState, editor: EditorState): { history: HistoryState; editor: EditorState; changed: boolean } {
  if (editor.lines.length !== 1 || history.index <= 0) {
    return { history, editor, changed: false }
  }

  let nextHistory = history
  if (history.index === history.entries.length) {
    nextHistory = { ...history, savedInput: getEditorText(editor) }
  }
  const nextIndex = nextHistory.index - 1
  const entry = nextHistory.entries[nextIndex]!
  return {
    history: { ...nextHistory, index: nextIndex },
    editor: withoutGhost({ ...editor, lines: [entry], cursorLine: 0, cursorCol: entry.length, completionCandidates: [] }),
    changed: true,
  }
}

export function historyNext(history: HistoryState, editor: EditorState): { history: HistoryState; editor: EditorState; changed: boolean } {
  if (editor.lines.length !== 1 || history.index >= history.entries.length) {
    return { history, editor, changed: false }
  }

  const nextIndex = history.index + 1
  if (nextIndex === history.entries.length) {
    return {
      history: { ...history, index: nextIndex },
      editor: withoutGhost({ ...editor, lines: [history.savedInput], cursorLine: 0, cursorCol: history.savedInput.length, completionCandidates: [] }),
      changed: true,
    }
  }

  const entry = history.entries[nextIndex]!
  return {
    history: { ...history, index: nextIndex },
    editor: withoutGhost({ ...editor, lines: [entry], cursorLine: 0, cursorCol: entry.length, completionCandidates: [] }),
    changed: true,
  }
}

// ---------------------------------------------------------------------------
// Ctrl+U — clear line before cursor
// ---------------------------------------------------------------------------

export function clearLineBefore(state: EditorState): EditorState {
  const newLines = [...state.lines]
  newLines[state.cursorLine] = newLines[state.cursorLine]!.slice(state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines, cursorCol: 0 })
}

// ---------------------------------------------------------------------------
// Ctrl+K — clear line after cursor
// ---------------------------------------------------------------------------

export function clearLineAfter(state: EditorState): EditorState {
  const newLines = [...state.lines]
  newLines[state.cursorLine] = newLines[state.cursorLine]!.slice(0, state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines })
}

// ---------------------------------------------------------------------------
// Ctrl+D — delete char at cursor (or signal exit if empty)
// ---------------------------------------------------------------------------

export function deleteForward(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  if (state.cursorCol < line.length) {
    const newLines = [...state.lines]
    newLines[state.cursorLine] = line.slice(0, state.cursorCol) + line.slice(state.cursorCol + 1)
    return withCompletionsCleared({ ...state, lines: newLines })
  }
  // Join with next line
  if (state.cursorLine < state.lines.length - 1) {
    const newLines = [...state.lines]
    newLines[state.cursorLine] = newLines[state.cursorLine]! + newLines[state.cursorLine + 1]!
    newLines.splice(state.cursorLine + 1, 1)
    return withCompletionsCleared({ ...state, lines: newLines })
  }
  return state
}

// ---------------------------------------------------------------------------
// Ctrl+W — delete word before cursor
// ---------------------------------------------------------------------------

export function deleteWordBefore(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  let i = state.cursorCol
  // skip trailing whitespace backward
  while (i > 0 && line[i - 1] === ' ') i--
  // skip word backward
  while (i > 0 && line[i - 1] !== ' ') i--
  const newLines = [...state.lines]
  newLines[state.cursorLine] = line.slice(0, i) + line.slice(state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines, cursorCol: i })
}

// ---------------------------------------------------------------------------
// Insert newline (Alt+Enter / continuation)
// ---------------------------------------------------------------------------

export function insertNewline(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  const newLines = [...state.lines]
  newLines.splice(state.cursorLine, 1, line.slice(0, state.cursorCol), line.slice(state.cursorCol))
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine + 1,
    cursorCol: 0,
  })
}

// ---------------------------------------------------------------------------
// Multi-line cursor movement (up/down within multi-line editor)
// ---------------------------------------------------------------------------

export function moveUp(state: EditorState): EditorState {
  if (state.cursorLine <= 0) return state
  const newLine = state.cursorLine - 1
  const newCol = Math.min(state.cursorCol, state.lines[newLine]!.length)
  return withoutGhost({ ...state, cursorLine: newLine, cursorCol: newCol })
}

export function moveDown(state: EditorState): EditorState {
  if (state.cursorLine >= state.lines.length - 1) return state
  const newLine = state.cursorLine + 1
  const newCol = Math.min(state.cursorCol, state.lines[newLine]!.length)
  return withoutGhost({ ...state, cursorLine: newLine, cursorCol: newCol })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function withCompletionsCleared(state: EditorState): EditorState {
  return refreshGhostHint({ ...state, completionCandidates: [] })
}

function withoutGhost(state: EditorState): EditorState {
  return { ...state, ghostHint: '' }
}
