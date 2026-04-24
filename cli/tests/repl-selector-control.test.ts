import { describe, expect, test } from 'bun:test'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import { RESUME_SELECTOR_TITLE } from '../src/term/app/resume.js'
import { createSelectorState, type SelectorItem } from '../src/term/selector.js'

const char = (value: string) => ({ type: 'char' as const, char: value })
const key = (type: 'up' | 'down' | 'backspace' | 'enter' | 'escape' | 'delete') => ({ type })

describe('repl selector control', () => {
  const items: SelectorItem[] = [
    { label: 'one', id: 'session-one', detail: 'first' },
    { label: 'two', id: 'session-two', detail: 'second' },
  ]

  test('updates focus on down/up', () => {
    let state = createSelectorState('Select model', items)
    const down = handleSelectorControl(state, key('down'))
    expect(down.kind).toBe('update')
    if (down.kind === 'update') {
      expect(down.state.focusIndex).toBe(1)
      state = down.state
    }

    const up = handleSelectorControl(state, key('up'))
    expect(up.kind).toBe('update')
    if (up.kind === 'update') expect(up.state.focusIndex).toBe(0)
  })

  test('updates query on char and backspace', () => {
    let action = handleSelectorControl(createSelectorState('Select model', items), char('w'))
    expect(action.kind).toBe('update')
    if (action.kind !== 'update') return
    expect(action.state.query).toBe('w')
    expect(action.state.items.map(i => i.label)).toEqual(['two'])

    action = handleSelectorControl(action.state, key('backspace'))
    expect(action.kind).toBe('update')
    if (action.kind === 'update') {
      expect(action.state.query).toBe('')
      expect(action.state.items.length).toBe(2)
    }
  })

  test('escape closes selector', () => {
    expect(handleSelectorControl(createSelectorState('T', items), key('escape')).kind).toBe('close')
  })

  test('resume enter returns selected session id', () => {
    const action = handleSelectorControl(createSelectorState(RESUME_SELECTOR_TITLE, items), key('enter'))
    expect(action).toEqual({ kind: 'resume', sessionId: 'session-one' })
  })

  test('history user enter returns goto action', () => {
    const state = createSelectorState('History  (↩ goto · enter preview)', [{ label: '#12', detail: 'user hello' }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'history-goto', seq: '12' })
  })

  test('history assistant enter returns preview action', () => {
    const state = createSelectorState('History  (↩ goto · enter preview)', [
      { label: '#13', detail: '  assistant hello', role: 'assistant' } as SelectorItem,
    ])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'history-preview', label: '#13', text: 'hello' })
  })

  test('history ellipsis enter closes selector', () => {
    const state = createSelectorState('History  (↩ goto · enter preview)', [{ label: '…', focusable: false }])
    expect(handleSelectorControl(state, key('enter')).kind).toBe('close')
  })

  test('model enter returns select-model action', () => {
    const state = createSelectorState('Select model', [{ label: 'claude' }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'select-model', model: 'claude' })
  })

  test('delete removes resume session item', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const action = handleSelectorControl(state, key('delete'))
    expect(action.kind).toBe('delete-session')
    if (action.kind === 'delete-session') {
      expect(action.sessionId).toBe('session-one')
      expect(action.label).toBe('one')
      expect(action.state.items.map(i => i.label)).toEqual(['two'])
    }
  })

  test('ctrl-d removes resume session item', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const action = handleSelectorControl(state, { type: 'ctrl', key: 'd' })
    expect(action.kind).toBe('delete-session')
  })

  test('non resume delete is ignored', () => {
    const state = createSelectorState('Select model', items)
    expect(handleSelectorControl(state, key('delete')).kind).toBe('none')
  })

  test('other ctrl key is ignored', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    expect(handleSelectorControl(state, { type: 'ctrl', key: 'c' }).kind).toBe('none')
  })
})
