import { describe, expect, test } from 'bun:test'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import {
  createSkillSelectorState,
  isSkillSelectorTitle,
  SKILL_SELECTOR_TITLE,
} from '../src/term/app/skill-window.js'

describe('skill command window', () => {
  test('builds a searchable read-only inventory from installed skills', () => {
    const state = createSkillSelectorState([
      { name: 'review', dir: '/skills/review' },
      { name: 'deploy', dir: '/skills/cloud/deploy', group: 'cloud' },
    ])

    expect(state.title).toBe(SKILL_SELECTOR_TITLE)
    expect(state.subtitle).toBe('2 installed · /skill list for sources and management')
    expect(state.items.map(item => item.label)).toEqual(['review', 'deploy'])
    expect(state.items[1]?.detail).toBe('in cloud/')
    expect(state.items[1]?.searchText).toContain('/skills/cloud/deploy')
    expect(state.hints).toEqual([
      { keys: ['up', 'down'], action: 'move' },
      { keys: 'type', action: 'filter' },
      { keys: 'escape', action: 'close' },
    ])
  })

  test('supports filtering after focus transfer', () => {
    const state = createSkillSelectorState([
      { name: 'review', dir: '/skills/review' },
      { name: 'deploy', dir: '/skills/cloud/deploy', group: 'cloud' },
    ])
    const action = handleSelectorControl(state, { type: 'char', char: 'c' })

    expect(action.kind).toBe('update')
    if (action.kind === 'update') {
      expect(action.state.items.map(item => item.label)).toEqual(['deploy'])
    }
  })

  test('shows an explicit empty state', () => {
    const state = createSkillSelectorState([])
    expect(state.emptyMessage).toBe('No skills installed')
    expect(state.items).toEqual([])
  })

  test('recognizes only the skill selector title', () => {
    expect(isSkillSelectorTitle(SKILL_SELECTOR_TITLE)).toBe(true)
    expect(isSkillSelectorTitle('Models')).toBe(false)
  })
})
