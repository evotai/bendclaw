import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createEditorState,
  insertText,
  backspace,
  moveLeft,
  moveRight,
  moveHome,
  moveEnd,
  clearEditor,
  getEditorText,
  isEditorEmpty,
} from '../src/term/input/editor.js'
import { buildPromptBlocks, type PromptVMInput } from '../src/term/viewmodel/prompt.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import chalk from 'chalk'

beforeAll(() => { chalk.level = 3 })

function defaultPromptVM(overrides?: Partial<PromptVMInput>): PromptVMInput {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    active: true,
    model: 'test-model',
    provider: '',
    thinkingLevel: '',
    planning: false,
    logMode: false,
    dashboardUrl: null,
    exitHint: false,
    completion: null,
    ghostHint: '',
    columns: 80,
    rows: 24,
    placeholder: true,
    cwd: '/Users/test/project',
    gitBranch: 'main',
    contextTokens: 0,
    contextWindow: 0,
    ...overrides,
  }
}

describe('createEditorState', () => {
  test('creates initial state with empty line', () => {
    const state = createEditorState()
    expect(state.lines).toEqual([''])
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(0)
  })
})

describe('insertText', () => {
  test('inserts single character', () => {
    let state = createEditorState()
    state = insertText(state, 'a')
    expect(state.lines).toEqual(['a'])
    expect(state.cursorCol).toBe(1)
  })

  test('inserts at cursor position', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = { ...state, cursorCol: 2 }
    state = insertText(state, 'X')
    expect(state.lines).toEqual(['heXllo'])
    expect(state.cursorCol).toBe(3)
  })

  test('inserts multi-line text', () => {
    let state = createEditorState()
    state = insertText(state, 'line1\nline2\nline3')
    expect(state.lines).toEqual(['line1', 'line2', 'line3'])
    expect(state.cursorLine).toBe(2)
    expect(state.cursorCol).toBe(5)
  })

  test('inserts multi-line in middle of existing text', () => {
    let state = createEditorState()
    state = insertText(state, 'before after')
    state = { ...state, cursorCol: 7 }
    state = insertText(state, 'X\nY')
    expect(state.lines).toEqual(['before X', 'Yafter'])
    expect(state.cursorLine).toBe(1)
    expect(state.cursorCol).toBe(1)
  })
})

describe('backspace', () => {
  test('deletes character before cursor', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = backspace(state)
    expect(state.lines).toEqual(['hell'])
    expect(state.cursorCol).toBe(4)
  })

  test('at start of line joins with previous', () => {
    let state = createEditorState()
    state = insertText(state, 'ab\ncd')
    state = { ...state, cursorLine: 1, cursorCol: 0 }
    state = backspace(state)
    expect(state.lines).toEqual(['abcd'])
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(2)
  })

  test('at start of first line does nothing', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = { ...state, cursorCol: 0 }
    state = backspace(state)
    expect(state.lines).toEqual(['hello'])
    expect(state.cursorCol).toBe(0)
  })
})

describe('cursor movement', () => {
  test('moveLeft moves left', () => {
    let state = createEditorState()
    state = insertText(state, 'abc')
    state = moveLeft(state)
    expect(state.cursorCol).toBe(2)
  })

  test('moveLeft at start of line wraps to previous', () => {
    let state = createEditorState()
    state = insertText(state, 'ab\ncd')
    state = { ...state, cursorLine: 1, cursorCol: 0 }
    state = moveLeft(state)
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(2)
  })

  test('moveLeft at start of first line does nothing', () => {
    let state = createEditorState()
    state = moveLeft(state)
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(0)
  })

  test('moveRight moves right', () => {
    let state = createEditorState()
    state = insertText(state, 'abc')
    state = { ...state, cursorCol: 1 }
    state = moveRight(state)
    expect(state.cursorCol).toBe(2)
  })

  test('moveRight at end of line wraps to next', () => {
    let state = createEditorState()
    state = insertText(state, 'ab\ncd')
    state = { ...state, cursorLine: 0, cursorCol: 2 }
    state = moveRight(state)
    expect(state.cursorLine).toBe(1)
    expect(state.cursorCol).toBe(0)
  })

  test('moveRight at end of last line does nothing', () => {
    let state = createEditorState()
    state = insertText(state, 'abc')
    state = moveRight(state)
    expect(state.cursorCol).toBe(3)
  })

  test('moveHome moves to start', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = moveHome(state)
    expect(state.cursorCol).toBe(0)
  })

  test('moveEnd moves to end', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = { ...state, cursorCol: 2 }
    state = moveEnd(state)
    expect(state.cursorCol).toBe(5)
  })
})

describe('clearEditor', () => {
  test('resets to empty', () => {
    let state = createEditorState()
    state = insertText(state, 'hello\nworld')
    state = clearEditor(state)
    expect(state.lines).toEqual([''])
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(0)
  })
})

describe('getEditorText', () => {
  test('returns joined lines', () => {
    let state = createEditorState()
    state = insertText(state, 'line1\nline2')
    expect(getEditorText(state)).toBe('line1\nline2')
  })

  test('single line', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    expect(getEditorText(state)).toBe('hello')
  })
})

describe('isEditorEmpty', () => {
  test('true for initial state', () => {
    const state = createEditorState()
    expect(isEditorEmpty(state)).toBe(true)
  })

  test('false after input', () => {
    let state = createEditorState()
    state = insertText(state, 'x')
    expect(isEditorEmpty(state)).toBe(false)
  })

  test('true after clear', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = clearEditor(state)
    expect(isEditorEmpty(state)).toBe(true)
  })
})

describe('renderPrompt', () => {
  test('returns array of strings', () => {
    const lines = blocksToLines(buildPromptBlocks(defaultPromptVM()))
    expect(Array.isArray(lines)).toBe(true)
    expect(lines.length).toBeGreaterThan(0)
  })

  test('contains the caret', () => {
    const lines = blocksToLines(buildPromptBlocks(defaultPromptVM()))
    const joined = lines.join('\n')
    expect(joined).toContain('\x1b_pi:c\x07')
    expect(joined).not.toContain('▍')
  })

  test('contains border', () => {
    const lines = blocksToLines(buildPromptBlocks(defaultPromptVM()))
    const joined = lines.join('\n')
    expect(joined).toContain('─')
  })

  test('shows input text', () => {
    const lines = blocksToLines(buildPromptBlocks(defaultPromptVM({
      lines: ['my input'],
      cursorCol: 8,
      placeholder: false,
    })))
    const joined = lines.join('\n')
    expect(joined).toContain('my input')
  })
})
