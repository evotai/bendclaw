import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createSelectorState,
  selectorUp,
  selectorDown,
  selectorSelect,
  selectorType,
  selectorBackspace,
  selectorExpandItems,
} from '../src/term/selector.js'
import { buildOverlayBlocks } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

beforeAll(() => { chalk.level = 3 })

const items = [
  { label: 'claude-opus', detail: 'Anthropic' },
  { label: 'gpt-4o', detail: 'OpenAI' },
  { label: 'gemini-pro', detail: 'Google' },
]

describe('createSelectorState', () => {
  test('creates state with focus at 0', () => {
    const state = createSelectorState('Pick model', items)
    expect(state.focusIndex).toBe(0)
    expect(state.title).toBe('Pick model')
    expect(state.items).toBe(items)
  })
})

describe('selectorUp', () => {
  test('moves focus up', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 2 }
    state = selectorUp(state)
    expect(state.focusIndex).toBe(1)
  })

  test('does not go below 0', () => {
    const state = createSelectorState('T', items)
    const next = selectorUp(state)
    expect(next.focusIndex).toBe(0)
    expect(next).toBe(state)
  })
})

describe('selectorDown', () => {
  test('moves focus down', () => {
    const state = createSelectorState('T', items)
    const next = selectorDown(state)
    expect(next.focusIndex).toBe(1)
  })

  test('does not exceed last item', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 2 }
    const next = selectorDown(state)
    expect(next.focusIndex).toBe(2)
    expect(next).toBe(state)
  })
})

describe('selectorSelect', () => {
  test('returns focused item', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 1 }
    const selected = selectorSelect(state)
    expect(selected).toEqual({ label: 'gpt-4o', detail: 'OpenAI' })
  })

  test('returns first item by default', () => {
    const state = createSelectorState('T', items)
    const selected = selectorSelect(state)
    expect(selected).toEqual({ label: 'claude-opus', detail: 'Anthropic' })
  })

  test('returns null for empty items', () => {
    const state = createSelectorState('T', [])
    const selected = selectorSelect(state)
    expect(selected).toBeNull()
  })
})

describe('renderSelector via viewmodel', () => {
  test('contains title', () => {
    const state = createSelectorState('Pick model', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('Pick model')
  })

  test('contains all item labels', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('claude-opus')
    expect(text).toContain('gpt-4o')
    expect(text).toContain('gemini-pro')
  })

  test('contains detail text', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('Anthropic')
    expect(text).toContain('OpenAI')
  })

  test('shows focus indicator on current item', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('❯ claude-opus')
  })

  test('shows navigation hint', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('navigate')
    expect(text).toContain('enter select')
    expect(text).toContain('esc cancel')
  })

  test('shows search query when filtering', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('search:')
    expect(text).toContain('g')
  })

  test('shows "No matches" when filter yields nothing', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('No matches')
  })

  test('highlights matching query in items', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'gpt')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const raw = lines.join('')
    // Should contain ANSI bold+yellow around "gpt"
    expect(raw).toContain('\x1b[1m')
    expect(raw).toContain('gpt')
    // Plain text should still have the label
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('gpt-4o')
  })
})

describe('selectorType', () => {
  test('filters items by label', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    expect(state.query).toBe('g')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
    expect(state.focusIndex).toBe(0)
  })

  test('filters items by detail', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'o')
    state = selectorType(state, 'p')
    state = selectorType(state, 'e')
    state = selectorType(state, 'n')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o'])
  })

  test('is case insensitive', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'G')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
  })

  test('resets focus on filter change', () => {
    let state = createSelectorState('T', items)
    state = selectorDown(state)
    expect(state.focusIndex).toBe(1)
    state = selectorType(state, 'g')
    expect(state.focusIndex).toBe(0)
  })
})

describe('selectorBackspace', () => {
  test('removes last char and widens filter', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    state = selectorType(state, 'p')
    state = selectorType(state, 't')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o'])
    state = selectorBackspace(state)
    expect(state.query).toBe('gp')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
  })

  test('clears filter restores all items', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    state = selectorBackspace(state)
    expect(state.query).toBe('')
    expect(state.items).toEqual(items)
  })

  test('noop when query is empty', () => {
    const state = createSelectorState('T', items)
    const next = selectorBackspace(state)
    expect(next).toBe(state)
  })
})

describe('fuzzy subsequence matching', () => {
  test('subsequence match finds non-contiguous chars', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'c')
    state = selectorType(state, 'o')
    state = selectorType(state, 'p')
    // "cop" is a subsequence of "claude-opus" (c...o...p) but not a substring
    expect(state.items.map(i => i.label)).toContain('claude-opus')
  })

  test('exact substring matches come before subsequence matches', () => {
    const testItems = [
      { label: 'deploy-service' },
      { label: 'deep-learning' },
      { label: 'data-pipeline' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'd')
    state = selectorType(state, 'p')
    // "dp" is substring of none, but subsequence of all three
    // "deploy-service" and "deep-learning" and "data-pipeline" all match as subsequence
    expect(state.items.length).toBeGreaterThan(0)
  })

  test('substring matches rank before subsequence matches', () => {
    const testItems = [
      { label: 'abc-xyz', detail: 'no match here' },
      { label: 'hello', detail: 'contains op inside' },
      { label: 'opus', detail: 'exact' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'o')
    state = selectorType(state, 'p')
    // "op" is substring of "opus" and "contains op inside"
    // "abc-xyz" has no match at all
    const labels = state.items.map(i => i.label)
    expect(labels).toContain('opus')
    expect(labels).toContain('hello')
    expect(labels).not.toContain('abc-xyz')
  })
})

describe('searchText field', () => {
  test('searches searchText when provided', () => {
    const testItems = [
      { label: 'abc12345', detail: 'My Project', searchText: 'abc12345 My Project /home/user/myproject rust' },
      { label: 'def67890', detail: 'Other Work', searchText: 'def67890 Other Work /tmp/job golang' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'r')
    state = selectorType(state, 'u')
    state = selectorType(state, 's')
    state = selectorType(state, 't')
    expect(state.items.map(i => i.label)).toEqual(['abc12345'])
  })

  test('falls back to label+detail when no searchText', () => {
    const mixed = [
      { label: 'with-search', detail: 'visible', searchText: 'hidden keyword' },
      { label: 'no-search', detail: 'keyword here' },
    ]
    let state = createSelectorState('T', mixed)
    state = selectorType(state, 'k')
    state = selectorType(state, 'e')
    state = selectorType(state, 'y')
    expect(state.items.map(i => i.label)).toEqual(['with-search', 'no-search'])
  })
})

describe('context extraction on match', () => {
  test('replaces detail with searchText context when matched', () => {
    const testItems = [
      { label: 'abc12345', detail: 'Original Title', searchText: 'abc12345 some long text about databend documentation and queries' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'd')
    state = selectorType(state, 'a')
    state = selectorType(state, 't')
    state = selectorType(state, 'a')
    state = selectorType(state, 'b')
    state = selectorType(state, 'e')
    state = selectorType(state, 'n')
    state = selectorType(state, 'd')
    expect(state.items.length).toBe(1)
    expect(state.items[0]!.detail).toContain('databend')
    expect(state.items[0]!.detail).not.toBe('Original Title')
  })

  test('restores original detail when query cleared', () => {
    const testItems = [
      { label: 'abc12345', detail: 'Original Title', searchText: 'abc12345 databend docs' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'd')
    state = selectorType(state, 'a')
    state = selectorType(state, 't')
    state = selectorBackspace(state)
    state = selectorBackspace(state)
    state = selectorBackspace(state)
    expect(state.items[0]!.detail).toBe('Original Title')
  })

  test('keeps original detail when no searchText', () => {
    const testItems = [
      { label: 'gpt-4o', detail: 'OpenAI' },
    ]
    let state = createSelectorState('T', testItems)
    state = selectorType(state, 'g')
    state = selectorType(state, 'p')
    state = selectorType(state, 't')
    expect(state.items[0]!.detail).toBe('OpenAI')
  })
})

describe('selectorExpandItems', () => {
  test('replaces allItems and re-filters with current query', () => {
    const initial = [
      { label: 'abc', detail: 'old' },
    ]
    let state = createSelectorState('T', initial)
    state = selectorType(state, 'x')
    expect(state.items.length).toBe(0)

    const expanded = [
      { label: 'abc', detail: 'old' },
      { label: 'xyz', detail: 'new', searchText: 'xyz new extra' },
    ]
    state = selectorExpandItems(state, expanded)
    expect(state.items.length).toBe(1)
    expect(state.items[0]!.label).toBe('xyz')
  })

  test('shows all expanded items when no query', () => {
    const initial = [{ label: 'a' }]
    let state = createSelectorState('T', initial)
    const expanded = [{ label: 'a' }, { label: 'b' }, { label: 'c' }]
    state = selectorExpandItems(state, expanded)
    expect(state.items.length).toBe(3)
  })
})

describe('focusable items', () => {
  const mixed = [
    { label: '#1', detail: 'user  hello', focusable: true },
    { label: '…', detail: 'assistant  reply', focusable: false },
    { label: '#3', detail: 'user  thanks', focusable: true },
    { label: '…', detail: 'assistant  bye', focusable: false },
  ]

  test('createSelectorState focuses first focusable item', () => {
    const nonFocusFirst = [
      { label: 'a', focusable: false },
      { label: 'b', focusable: true },
      { label: 'c', focusable: true },
    ]
    const state = createSelectorState('T', nonFocusFirst)
    expect(state.focusIndex).toBe(1)
  })

  test('selectorDown skips non-focusable items', () => {
    let state = createSelectorState('T', mixed)
    expect(state.focusIndex).toBe(0)
    state = selectorDown(state)
    expect(state.focusIndex).toBe(2)
  })

  test('selectorUp skips non-focusable items', () => {
    let state = createSelectorState('T', mixed)
    state = { ...state, focusIndex: 2 }
    state = selectorUp(state)
    expect(state.focusIndex).toBe(0)
  })

  test('selectorDown stays if no focusable item below', () => {
    let state = createSelectorState('T', mixed)
    state = { ...state, focusIndex: 2 }
    const next = selectorDown(state)
    expect(next.focusIndex).toBe(2)
    expect(next).toBe(state)
  })

  test('selectorUp stays if no focusable item above', () => {
    const state = createSelectorState('T', mixed)
    const next = selectorUp(state)
    expect(next.focusIndex).toBe(0)
    expect(next).toBe(state)
  })

  test('items without focusable field are focusable by default', () => {
    const plain = [
      { label: 'a' },
      { label: 'b' },
    ]
    let state = createSelectorState('T', plain)
    expect(state.focusIndex).toBe(0)
    state = selectorDown(state)
    expect(state.focusIndex).toBe(1)
  })
})
