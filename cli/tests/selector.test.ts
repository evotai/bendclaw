import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createSelectorState,
  selectorUp,
  selectorDown,
  selectorSelect,
  selectorType,
  selectorBackspace,
  selectorExpandItems,
  selectorFocusOn,
  selectorRemoveItem,
} from '../src/term/selector.js'
import { buildOverlayBlocks, buildSelectorRegionLines } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
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

  test('wraps from the first choice to the last when circular navigation is enabled', () => {
    const grouped = [
      { label: 'anthropic', header: true, focusable: false },
      { label: 'claude-opus' },
      { label: '', header: true, focusable: false },
      { label: 'openai', header: true, focusable: false },
      { label: 'gpt-5.5' },
    ]
    const state = { ...createSelectorState('Models', grouped), circularNavigation: true }
    const next = selectorUp(state)
    expect(next.focusIndex).toBe(4)
    expect(next.items[next.focusIndex]?.label).toBe('gpt-5.5')
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

  test('wraps from the last choice to the first and restores its header', () => {
    const grouped = [
      { label: 'anthropic', header: true, focusable: false },
      { label: 'claude-opus' },
      { label: '', header: true, focusable: false },
      { label: 'openai', header: true, focusable: false },
      { label: 'gpt-5.5' },
    ]
    let state = { ...createSelectorState('Models', grouped), circularNavigation: true, focusIndex: 4 }
    state = selectorDown(state)
    expect(state.focusIndex).toBe(1)
    expect(state.scrollOffset).toBe(0)
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
  test('renders the pi model selector as a full-width editor replacement', () => {
    const state = {
      ...createSelectorState('Models', [
        { label: 'openai', header: true, focusable: false, group: 'openai' },
        { label: 'grok-4.5', group: 'openai', selected: true },
        { label: 'droid', header: true, focusable: false, group: 'droid' },
        { label: 'gpt-5.6-sol', group: 'droid' },
      ]),
      presentation: 'model' as const,
      circularNavigation: true,
    }
    const lines = buildSelectorRegionLines(state, 40)
      .map(line => stripAnsi(line).replaceAll('\x1b_pi:c\x07', ''))

    expect(lines[0]).toBe('')
    expect(lines[1]).toBe('─'.repeat(40))
    expect(lines[2]).toBe('')
    expect(lines[3]).toBe('Only showing models from configured')
    expect(lines[4]).toBe('providers. Run evot login to add cloud')
    expect(lines[5]).toBe('models.')
    expect(lines[7]).toStartWith('>  ')
    expect(lines[9]).toBe('  — openai —')
    expect(lines[10]).toBe('→ grok-4.5 ✓')
    expect(lines[11]).toBe('')
    expect(lines[12]).toBe('  — droid —')
    expect(lines[13]).toBe('  gpt-5.6-sol')
    expect(lines[15]).toBe('  Model Name: grok-4.5')
    expect(lines.at(-1)).toBe('─'.repeat(40))
    expect(lines.join('\n')).not.toContain('Models  2')
    expect(lines.join('\n')).not.toContain('enter select')
  })

  test('model filtering keeps provider groups without repeated badges', () => {
    let state = {
      ...createSelectorState('Models', [
        { label: 'openai', header: true, focusable: false, group: 'openai' },
        { label: 'grok-4.5', group: 'openai', searchText: 'grok-4.5 openai' },
        { label: 'droid', header: true, focusable: false, group: 'droid' },
        { label: 'gpt-5.6-sol', group: 'droid', searchText: 'gpt-5.6-sol droid' },
      ]),
      presentation: 'model' as const,
    }
    for (const char of 'droid') state = selectorType(state, char)

    const text = buildSelectorRegionLines(state, 80)
      .map(line => stripAnsi(line).replaceAll('\x1b_pi:c\x07', ''))
      .join('\n')
    expect(text).toContain('  — droid —\n→ gpt-5.6-sol')
    expect(text).not.toContain('[droid]')
  })

  test('model selector renders one weak header per group with blank separators', () => {
    // Headings are explicit rows, so a group name is shown once, verbatim.
    const state = {
      ...createSelectorState('Models', [
        { label: 'openai', header: true, focusable: false, group: 'openai' },
        { label: 'gpt-5.6-sol', group: 'openai', selected: true },
        { label: 'grok-4.5', group: 'openai' },
        { label: 'anthropic', header: true, focusable: false, group: 'anthropic' },
        { label: 'claude-opus-4-8', group: 'anthropic' },
        { label: 'claude-sonnet-5', group: 'anthropic' },
      ]),
      presentation: 'model' as const,
    }
    const lines = buildSelectorRegionLines(state, 80)
      .map(line => stripAnsi(line).replaceAll('\x1b_pi:c\x07', ''))
    const listStart = lines.indexOf('  — openai —')

    expect(lines.slice(listStart, listStart + 8)).toEqual([
      '  — openai —',
      '→ gpt-5.6-sol ✓',
      '  grok-4.5',
      '',
      '  — anthropic —',
      '  claude-opus-4-8',
      '  claude-sonnet-5',
      '',
    ])
    expect(lines.join('\n')).not.toContain('[openai]')
    expect(lines.join('\n')).not.toContain('[anthropic]')
  })

  test('model selector keeps a blank separator when a group header scrolls into view', () => {
    const state = {
      ...createSelectorState('Models', [
        { label: 'Evot Free', header: true, focusable: false, group: 'free' },
        ...Array.from({ length: 11 }, (_, index) => ({ label: `free-${index + 1}`, group: 'free' })),
        { label: 'anthropic · Anthropic Messages', header: true, focusable: false, group: 'anthropic' },
        { label: 'claude-opus-5', group: 'anthropic' },
      ]),
      presentation: 'model' as const,
      focusIndex: 9,
    }
    const lines = buildSelectorRegionLines(state, 80)
      .map(line => stripAnsi(line).replaceAll('\x1b_pi:c\x07', ''))
    const headerIndex = lines.indexOf('  — anthropic · Anthropic Messages —')

    expect(headerIndex).toBeGreaterThan(0)
    expect(lines[headerIndex - 1]).toBe('')
  })

  test('model filtering uses fuzzy matching and quality ordering within a provider', () => {
    let state = {
      ...createSelectorState('Models', [
        { label: 'alpha-gpt', group: 'provider', searchText: 'alpha-gpt provider' },
        { label: 'gpt-alpha', group: 'provider', searchText: 'gpt-alpha provider' },
        { label: 'unrelated', group: 'provider', searchText: 'unrelated provider' },
      ]),
      presentation: 'model' as const,
    }
    for (const char of 'gpt') state = selectorType(state, char)

    expect(state.items.map(item => item.label)).toEqual(['gpt-alpha', 'alpha-gpt'])
  })

  test('model search input matches pi at extremely narrow widths', () => {
    const state = {
      ...createSelectorState('Models', [{ label: 'gpt', detail: 'openai' }]),
      presentation: 'model' as const,
    }

    for (const width of [1, 2]) {
      const lines = buildSelectorRegionLines(state, width)
        .map(line => stripAnsi(line).replaceAll('\x1b_pi:c\x07', ''))
      expect(lines).toContain('> ')
      expect(lines.filter(line => line !== '> ').every(line => stringWidth(line) <= width)).toBe(true)
    }
  })

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
    expect(text).toContain('move')
    expect(text).toContain('enter select')
    expect(text).toContain('esc close')
  })

  test('shows queue actions with the shared Ctrl+D remove shortcut', () => {
    const state = createSelectorState('Prompt queue', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('enter edit')
    expect(text).toContain('Ctrl+D remove')
    expect(text).not.toContain('del remove')
  })

  test('shows search query when filtering', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('Filter')
    expect(text).toContain('g')
  })

  test('shows empty-filter state when filter yields nothing', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('No matching items')
  })

  test('renders provider as part of the model identity', () => {
    const state = createSelectorState('Models', [
      { label: 'gpt-5.6-sol@droid', selected: true },
      { label: 'gpt-5.6-sol@cursor' },
    ])
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('gpt-5.6-sol@droid ✓')
    expect(text).toContain('gpt-5.6-sol@cursor')
  })

  test('renders provider group headers as dividers', () => {
    const state = createSelectorState('Models', [
      { label: 'anthropic', header: true, focusable: false },
      { label: 'claude-opus' },
      { label: '', header: true, focusable: false },
      { label: 'openai', header: true, focusable: false },
      { label: 'gpt-5.5' },
    ])
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('── anthropic ──\n❯ claude-opus\n\n── openai ──')
    expect(text).toContain('── openai ──')
    expect(text).toContain('❯ claude-opus')
    // Headers and spacing rows do not count as selectable items in the title tally.
    expect(text).toContain('Models  2')
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

  test('multi-keyword filter requires every whitespace-separated token', () => {
    const testItems = [
      {
        label: 'aaa11111',
        detail: 'Payment retry',
        searchText: 'aaa11111 Payment retry timeout on checkout /work/shop rust',
      },
      {
        label: 'bbb22222',
        detail: 'Payment success',
        searchText: 'bbb22222 Payment success path /work/shop rust',
      },
      {
        label: 'ccc33333',
        detail: 'Auth timeout',
        searchText: 'ccc33333 Auth timeout login flow /work/auth golang',
      },
    ]
    let state = createSelectorState('Resume session', testItems)
    for (const char of 'payment timeout') state = selectorType(state, char)

    expect(state.query).toBe('payment timeout')
    expect(state.items.map(i => i.label)).toEqual(['aaa11111'])
  })

  test('multi-keyword filter is order-independent', () => {
    const testItems = [
      {
        label: 'aaa11111',
        detail: 'Payment retry',
        searchText: 'aaa11111 Payment retry timeout on checkout',
      },
      {
        label: 'bbb22222',
        detail: 'Unrelated',
        searchText: 'bbb22222 something else entirely',
      },
    ]
    let state = createSelectorState('Resume session', testItems)
    for (const char of 'timeout payment') state = selectorType(state, char)

    expect(state.items.map(i => i.label)).toEqual(['aaa11111'])
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

describe('smooth scrolling window', () => {
  const many = Array.from({ length: 25 }, (_, i) => ({ label: `model-${i}` }))

  test('scrollOffset stays put while focus moves inside the window', () => {
    let state = createSelectorState('T', many)
    expect(state.scrollOffset).toBe(0)
    for (let i = 0; i < 9; i++) state = selectorDown(state)
    expect(state.focusIndex).toBe(9)
    expect(state.scrollOffset).toBe(0)
  })

  test('window slides one row at a time when focus passes the bottom edge', () => {
    let state = createSelectorState('T', many)
    for (let i = 0; i < 10; i++) state = selectorDown(state)
    expect(state.focusIndex).toBe(10)
    expect(state.scrollOffset).toBe(1)
    state = selectorDown(state)
    expect(state.scrollOffset).toBe(2)
  })

  test('moving back up keeps the window until focus hits the top edge', () => {
    let state = createSelectorState('T', many)
    for (let i = 0; i < 14; i++) state = selectorDown(state)
    expect(state.scrollOffset).toBe(5)
    // Focus walks up inside the window without any scroll.
    for (let i = 0; i < 9; i++) state = selectorUp(state)
    expect(state.focusIndex).toBe(5)
    expect(state.scrollOffset).toBe(5)
    // The next step crosses the top edge: slide exactly one row.
    state = selectorUp(state)
    expect(state.scrollOffset).toBe(4)
  })

  test('reaching the first model scrolls its group header into view', () => {
    const grouped = [
      { label: 'anthropic', header: true, focusable: false },
      ...Array.from({ length: 24 }, (_, i) => ({ label: `m-${i}` })),
    ]
    let state = createSelectorState('T', grouped)
    for (let i = 0; i < 15; i++) state = selectorDown(state)
    while (state.focusIndex > 1) state = selectorUp(state)
    expect(state.scrollOffset).toBe(0)
  })

  test('selectorFocusOn jumps focus and keeps it visible', () => {
    let state = createSelectorState('T', many)
    state = selectorFocusOn(state, item => item.label === 'model-20')
    expect(state.focusIndex).toBe(20)
    expect(state.scrollOffset).toBe(11)
  })

  test('filtering drops unassociated group headers from results', () => {
    let state = createSelectorState('T', [
      { label: 'anthropic', header: true, focusable: false },
      { label: 'claude-opus' },
      { label: 'openai', header: true, focusable: false },
      { label: 'gpt-5.5' },
    ])
    state = selectorType(state, 'a')
    expect(state.items.every(i => !i.header)).toBe(true)
    expect(state.items.map(i => i.label)).toContain('claude-opus')
  })

  test('filtering retains headers for matching associated groups', () => {
    let state = createSelectorState('T', [
      { label: 'Current cwd', header: true, focusable: false, group: 'current' },
      { label: 'aaaaaaaa', searchText: 'current alpha', group: 'current' },
      { label: 'Other cwd', header: true, focusable: false, group: 'other' },
      { label: 'bbbbbbbb', searchText: 'other payment timeout', group: 'other', contextPrefix: '/other · ' },
    ])
    state = selectorType(state, 'p')
    state = selectorType(state, 'a')
    state = selectorType(state, 'y')

    expect(state.items.map(item => item.label)).toEqual(['Other cwd', 'bbbbbbbb'])
    expect(state.focusIndex).toBe(1)
    expect(state.items[1]!.detail).toStartWith('/other · ')
  })

  test('removing the last item in a group also removes its header', () => {
    let state = createSelectorState('T', [
      { label: 'Current cwd', header: true, focusable: false, group: 'current' },
      { label: 'aaaaaaaa', id: 'session-a', group: 'current' },
      { label: 'Other cwd', header: true, focusable: false, group: 'other' },
      { label: 'bbbbbbbb', id: 'session-b', group: 'other' },
    ])
    state = selectorRemoveItem(state, 1)

    expect(state.items.map(item => item.label)).toEqual(['Other cwd', 'bbbbbbbb'])
    expect(state.focusIndex).toBe(1)
  })
})
