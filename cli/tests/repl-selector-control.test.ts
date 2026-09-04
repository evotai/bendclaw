import { describe, expect, test } from 'bun:test'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import { RESUME_SELECTOR_TITLE } from '../src/term/app/resume.js'
import { SKILL_SELECTOR_TITLE } from '../src/term/app/skill-window.js'
import { createSelectorState, selectorExpandItems, selectorType, type SelectorItem } from '../src/term/selector.js'

const char = (value: string) => ({ type: 'char' as const, char: value })
const key = (type: 'up' | 'down' | 'tab' | 'shift-tab' | 'backspace' | 'enter' | 'escape' | 'delete') => ({ type })

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

  test('tab moves down and shift-tab moves up', () => {
    const state = createSelectorState('Select model', items)
    const next = handleSelectorControl(state, key('tab'))
    expect(next.kind).toBe('update')
    if (next.kind !== 'update') return
    expect(next.state.focusIndex).toBe(1)

    const previous = handleSelectorControl(next.state, key('shift-tab'))
    expect(previous.kind).toBe('update')
    if (previous.kind === 'update') expect(previous.state.focusIndex).toBe(0)
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

  test('resume first arrow focuses the first session before navigation', () => {
    const state = { ...createSelectorState(RESUME_SELECTOR_TITLE, items), listFocused: false }

    const down = handleSelectorControl(state, key('down'))
    expect(down.kind).toBe('update')
    if (down.kind !== 'update') return
    expect(down.state.listFocused).toBe(true)
    expect(down.state.focusIndex).toBe(0)

    const next = handleSelectorControl(down.state, key('down'))
    expect(next.kind).toBe('update')
    if (next.kind === 'update') expect(next.state.focusIndex).toBe(1)
  })

  test('every command window promotes focus on the first arrow, not just resume', () => {
    // The model and skill windows previously navigated on the first arrow while
    // the composer still owned the caret, so `/mo` + ↓ typed into the composer
    // instead of moving to a model. Focus promotion is now shared.
    for (const title of ['Models', SKILL_SELECTOR_TITLE]) {
      const state = { ...createSelectorState(title, items), listFocused: false }

      const first = handleSelectorControl(state, key('down'))
      expect(first.kind).toBe('update')
      if (first.kind !== 'update') return
      expect(first.state.listFocused).toBe(true)
      expect(first.state.focusIndex).toBe(0)

      const second = handleSelectorControl(first.state, key('down'))
      expect(second.kind).toBe('update')
      if (second.kind === 'update') expect(second.state.focusIndex).toBe(1)
    }
  })

  test('promotion keeps the row the preview already highlighted', () => {
    // `/model` opens focused on the active model. The first arrow must hand
    // focus to the list without snapping the selection back to the first row.
    const state = { ...createSelectorState('Models', items), focusIndex: 1, listFocused: false }

    const promoted = handleSelectorControl(state, key('down'))
    expect(promoted.kind).toBe('update')
    if (promoted.kind !== 'update') return
    expect(promoted.state.listFocused).toBe(true)
    expect(promoted.state.focusIndex).toBe(1)
  })

  test('typing returns resume focus to the filter', () => {
    const state = { ...createSelectorState(RESUME_SELECTOR_TITLE, items), listFocused: true }
    const next = handleSelectorControl(state, char('o'))
    expect(next.kind).toBe('update')
    if (next.kind === 'update') {
      expect(next.state.listFocused).toBe(false)
      expect(next.state.query).toBe('o')
    }
  })

  test('resume enter returns selected session id', () => {
    const action = handleSelectorControl(createSelectorState(RESUME_SELECTOR_TITLE, items), key('enter'))
    expect(action).toEqual({ kind: 'resume', sessionId: 'session-one' })
  })

  test('model enter returns provider-qualified select-model action', () => {
    const state = createSelectorState('Select model', [{ label: 'claude', id: 'anthropic:claude', detail: 'anthropic' }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'select-model', spec: 'anthropic:claude' })
  })

  test('skill inventory enter is read-only', () => {
    const state = createSelectorState(SKILL_SELECTOR_TITLE, [{ label: 'review', id: 'review' }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'none' })
  })

  test('delete requires a second press before removing resume session', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const first = handleSelectorControl(state, key('delete'))
    expect(first.kind).toBe('update')
    if (first.kind !== 'update') return
    expect(first.state.pendingDeleteId).toBe('session-one')
    expect(first.state.subtitle).toBe('Press Ctrl+D / Del again to delete')
    expect(first.state.items.map(i => i.label)).toEqual(['one', 'two'])

    const second = handleSelectorControl(first.state, key('delete'))
    expect(second.kind).toBe('delete-session')
    if (second.kind === 'delete-session') {
      expect(second.sessionId).toBe('session-one')
      expect(second.label).toBe('one')
      expect(second.state.items.map(i => i.label)).toEqual(['two'])
      expect(second.state.pendingDeleteId).toBeUndefined()
      expect(second.state.subtitle).toBeUndefined()
    }
  })

  test('ctrl-d requires a second press before removing resume session', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const first = handleSelectorControl(state, { type: 'ctrl', key: 'd' })
    expect(first.kind).toBe('update')
    if (first.kind !== 'update') return

    const second = handleSelectorControl(first.state, { type: 'ctrl', key: 'd' })
    expect(second.kind).toBe('delete-session')
  })

  test('navigation after arming resume delete cancels the confirmation', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const armed = handleSelectorControl(state, key('delete'))
    expect(armed.kind).toBe('update')
    if (armed.kind !== 'update') return

    const moved = handleSelectorControl(armed.state, key('down'))
    expect(moved.kind).toBe('update')
    if (moved.kind === 'update') {
      expect(moved.state.pendingDeleteId).toBeUndefined()
      expect(moved.state.subtitle).toBeUndefined()
      expect(moved.state.focusIndex).toBe(1)
    }
  })

  test('async list refresh between presses cannot delete a different session', () => {
    const armed = handleSelectorControl(createSelectorState(RESUME_SELECTOR_TITLE, items), key('delete'))
    expect(armed.kind).toBe('update')
    if (armed.kind !== 'update') return
    expect(armed.state.pendingDeleteId).toBe('session-one')

    // listSessionsWithText resolving reorders the pool. Keep the focused
    // session, but drop the armed delete so a confirming keypress cannot fire.
    const reordered = selectorExpandItems(armed.state, [
      { label: 'two', id: 'session-two', detail: 'second' },
      { label: 'one', id: 'session-one', detail: 'first' },
    ])
    expect(reordered.pendingDeleteId).toBeUndefined()
    expect(reordered.items[reordered.focusIndex]?.id).toBe('session-one')

    const next = handleSelectorControl(reordered, key('delete'))
    expect(next.kind).toBe('update')
    if (next.kind === 'update') expect(next.state.pendingDeleteId).toBe('session-one')
  })

  test('non resume delete is ignored', () => {
    const state = createSelectorState('Select model', items)
    expect(handleSelectorControl(state, key('delete')).kind).toBe('none')
  })

  test('queue selector supports selection, edit, and remove only', () => {
    const state = createSelectorState('Prompt queue', [{
      label: '1. later',
      id: 'follow_up|q1|3',
      searchText: 'later',
    }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({
      kind: 'queue-edit',
      entry: { queue: 'follow_up', id: 'q1', version: 3, text: 'later' },
    })
    expect(handleSelectorControl(state, key('delete')).kind).toBe('queue-remove')
    expect(handleSelectorControl(state, { type: 'ctrl', key: 'd' }).kind).toBe('queue-remove')
    expect(handleSelectorControl(state, { type: 'shift-char', char: 'j' }).kind).toBe('none')
    expect(handleSelectorControl(state, { type: 'ctrl-enter' }).kind).toBe('none')
  })

  test('queue character keys do not define alternate edit or remove shortcuts', () => {
    const state = createSelectorState('Prompt queue', [{
      label: '1. queued',
      id: 'steering|q2|0',
      searchText: 'queued',
    }])
    expect(handleSelectorControl(state, char('e')).kind).toBe('none')
    expect(handleSelectorControl(state, char('x')).kind).toBe('none')
  })

  test('other ctrl key is ignored', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    expect(handleSelectorControl(state, { type: 'ctrl', key: 'c' }).kind).toBe('none')
  })
})
