import type { Hint } from './app/hint.js'

export interface SelectorItem {
  label: string
  detail?: string
  /** Renders as a non-focusable group divider (e.g. a provider name). */
  header?: boolean
  /** Marks the active choice without mixing status into detail text. */
  selected?: boolean
  /** Opaque identifier (e.g. full session id) — not displayed. */
  id?: string
  /** Extra text searched but not displayed (e.g. full session id, cwd). */
  searchText?: string
  /** When false, up/down navigation skips this item. Defaults to true. */
  focusable?: boolean
  /** Associates an item with a non-focusable group header. Filtering keeps the
   *  header whenever at least one item in the group matches. */
  group?: string
  /** Prefix retained when full-text filtering replaces detail with a snippet. */
  contextPrefix?: string
  /** Lines shown in the side pane while this row is focused. First line is the
   *  heading. Rows without a preview render the list at full width. */
  preview?: string[]
  /** Footer hints shown while this row is focused, overriding the selector's.
   *  Rows advertise their own gestures so a key that would do nothing on this
   *  row is never offered. */
  hints?: Hint[]
  /** Row count for a `header` item. Present headers render as a bold label with
   *  a dim count (`Shells (2)`); absent, they render as a `── label ──` rule. */
  headerCount?: number
}

export interface SelectorState {
  items: SelectorItem[]
  allItems: SelectorItem[]
  focusIndex: number
  /** First visible row. Moves only when focus walks past a window edge, so
   *  the list slides one row at a time instead of recentering (droid-style). */
  scrollOffset: number
  title: string
  /** Optional secondary context displayed below the title. */
  subtitle?: string
  /** Model selection and live background output use dedicated editor-replacement
   * presentations instead of the generic titled selector. */
  presentation?: 'model' | 'background-output'
  /** Wraps up/down navigation between the first and last focusable items. */
  circularNavigation?: boolean
  /** Session id armed for deletion, awaiting a confirming second keypress.
   *  Keyed by id rather than index so an async list refresh or reorder cannot
   *  redirect the confirmation onto a different session. */
  pendingDeleteId?: string
  /** Footer hints for this selector. When set, they replace the generic
   *  move/select/filter/close line; a focused row's own `hints` win over these. */
  hints?: Hint[]
  /** Suppresses the filter line and type-to-search. Lists that reserve bare
   *  letters for actions opt out, so typing can never build a hidden query
   *  against a list with no visible filter. */
  noFilter?: boolean
  /** Body text shown in place of the list when there are no rows. Replaces the
   *  generic "No matching items", which would describe a filter some lists
   *  do not have. */
  emptyMessage?: string
  query: string
}

/** Rows visible at once in the selector window. Shared by movement logic and
 *  the renderer so scroll behavior and display stay in sync. */
export const SELECTOR_VIEWPORT = 10

/** Slide the window minimally so `focus` is visible: no jumps, no recentering. */
function ensureVisible(offset: number, focus: number, total: number): number {
  const maxOffset = Math.max(0, total - SELECTOR_VIEWPORT)
  let next = Math.min(Math.max(offset, 0), maxOffset)
  if (focus < next) next = focus
  else if (focus >= next + SELECTOR_VIEWPORT) next = focus - SELECTOR_VIEWPORT + 1
  return Math.min(next, maxOffset)
}

/** Find the first focusable index in items, defaulting to 0. */
function firstFocusable(items: SelectorItem[]): number {
  const idx = items.findIndex(i => i.focusable !== false)
  return idx >= 0 ? idx : 0
}

function lastFocusable(items: SelectorItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) {
    if (items[i]?.focusable !== false) return i
  }
  return 0
}

export function createSelectorState(title: string, items: SelectorItem[], allItems?: SelectorItem[], initialQuery?: string): SelectorState {
  const all = allItems ?? items
  if (initialQuery) {
    return applyFilter({ items: all, allItems: all, focusIndex: 0, scrollOffset: 0, title, query: '' }, initialQuery)
  }
  const focusIndex = firstFocusable(items)
  return { items, allItems: all, focusIndex, scrollOffset: ensureVisible(0, focusIndex, items.length), title, query: '' }
}

/** Move focus to the first item matching `predicate`, keeping it visible. */
export function selectorFocusOn(state: SelectorState, predicate: (item: SelectorItem) => boolean): SelectorState {
  const idx = state.items.findIndex(i => i.focusable !== false && predicate(i))
  if (idx < 0) return state
  return { ...state, focusIndex: idx, scrollOffset: ensureVisible(state.scrollOffset, idx, state.items.length) }
}

export function selectorUp(state: SelectorState): SelectorState {
  let next = state.focusIndex - 1
  while (next >= 0 && state.items[next]?.focusable === false) next--
  if (next < 0) {
    if (!state.circularNavigation || state.items.length === 0) return state
    next = lastFocusable(state.items)
    if (state.items[next]?.focusable === false) return state
    return {
      ...state,
      focusIndex: next,
      scrollOffset: ensureVisible(Math.max(0, state.items.length - SELECTOR_VIEWPORT), next, state.items.length),
    }
  }
  // When only headers remain above, slide the window to the very top so the
  // leading group divider scrolls into view with its first model.
  const anyFocusableAbove = state.items.slice(0, next).some(i => i.focusable !== false)
  const target = anyFocusableAbove ? next : 0
  return { ...state, focusIndex: next, scrollOffset: ensureVisible(state.scrollOffset, target, state.items.length) }
}

export function selectorDown(state: SelectorState): SelectorState {
  let next = state.focusIndex + 1
  while (next < state.items.length && state.items[next]?.focusable === false) next++
  if (next >= state.items.length) {
    if (!state.circularNavigation || state.items.length === 0) return state
    next = firstFocusable(state.items)
    if (state.items[next]?.focusable === false) return state
    // Include a leading group header when wrapping back to its first model.
    const target = state.items.slice(0, next).some(i => i.focusable !== false) ? next : 0
    return { ...state, focusIndex: next, scrollOffset: ensureVisible(0, target, state.items.length) }
  }
  return { ...state, focusIndex: next, scrollOffset: ensureVisible(state.scrollOffset, next, state.items.length) }
}

export function selectorSelect(state: SelectorState): SelectorItem | null {
  return state.items[state.focusIndex] ?? null
}

export function selectorType(state: SelectorState, char: string): SelectorState {
  const query = state.query + char
  return applyFilter(state, query)
}

export function selectorBackspace(state: SelectorState): SelectorState {
  if (state.query.length === 0) return state
  const query = state.query.slice(0, -1)
  return applyFilter(state, query)
}

export function selectorExpandItems(state: SelectorState, allItems: SelectorItem[]): SelectorState {
  // Replacing the pool can reorder or drop rows, so any armed delete is
  // dropped: the confirming keypress must never land on a different session.
  const focused = state.items[state.focusIndex]
  const updated = { ...state, allItems, pendingDeleteId: undefined, subtitle: undefined }
  const next = state.query
    ? applyFilter(updated, state.query)
    : {
        ...updated,
        items: allItems,
        focusIndex: firstFocusable(allItems),
        scrollOffset: ensureVisible(updated.scrollOffset, firstFocusable(allItems), allItems.length),
      }
  if (!focused || focused.header) return next
  // Keep the row the user was looking at across an async catalog refresh.
  return selectorFocusOn(next, item => focused.id ? item.id === focused.id : item.label === focused.label)
}

export function selectorClearQuery(state: SelectorState): SelectorState {
  if (!state.query) return state
  return applyFilter(state, '')
}

export function selectorRemoveItem(state: SelectorState, index: number): SelectorState {
  const target = state.items[index]
  if (!target || target.header) return state
  const key = target.id ?? target.label
  const allItems = pruneEmptyGroups(state.allItems.filter(item => (item.id ?? item.label) !== key))
  // Removal consumes the armed delete; a later keypress must re-arm.
  const cleared = { ...state, pendingDeleteId: undefined }
  if (cleared.query) return applyFilter({ ...cleared, allItems }, cleared.query)

  const items = allItems
  let focusIndex = Math.min(index, Math.max(0, items.length - 1))
  while (focusIndex < items.length && items[focusIndex]?.focusable === false) focusIndex++
  if (focusIndex >= items.length) focusIndex = lastFocusable(items)
  return { ...cleared, items, allItems, focusIndex, scrollOffset: ensureVisible(cleared.scrollOffset, focusIndex, items.length) }
}

function pruneEmptyGroups(items: SelectorItem[]): SelectorItem[] {
  const populatedGroups = new Set(items.flatMap(item => !item.header && item.group ? [item.group] : []))
  return items.filter(item => !item.header || !item.group || populatedGroups.has(item.group))
}

function searchableText(item: SelectorItem): string {
  if (item.searchText) return item.searchText.toLowerCase()
  return `${item.label} ${item.detail ?? ''}`.toLowerCase()
}

function isSubsequence(text: string, query: string): boolean {
  let j = 0
  for (let i = 0; i < text.length && j < query.length; i++) {
    if (text[i] === query[j]) j++
  }
  return j === query.length
}

/** Split a free-text filter into whitespace-separated keywords. */
function queryTokens(query: string): string[] {
  return query.toLowerCase().trim().split(/\s+/).filter(Boolean)
}

/** True when every token is a substring of `text` (already lowercased). */
function matchesAllTokens(text: string, tokens: string[]): boolean {
  if (tokens.length === 0) return true
  return tokens.every(token => text.includes(token))
}

function extractContext(source: string, query: string, width: number): string | null {
  const lower = source.toLowerCase()
  const tokens = queryTokens(query)
  if (tokens.length === 0) return null

  // Prefer a snippet around the first multi-token hit; fall back to the first
  // individual token so multi-keyword filters still surface useful context.
  let idx = -1
  let matchLen = 0
  if (tokens.length === 1) {
    idx = lower.indexOf(tokens[0]!)
    matchLen = tokens[0]!.length
  } else {
    const joined = tokens.join(' ')
    idx = lower.indexOf(joined)
    matchLen = joined.length
    if (idx === -1) {
      for (const token of tokens) {
        const tokenIdx = lower.indexOf(token)
        if (tokenIdx !== -1) {
          idx = tokenIdx
          matchLen = token.length
          break
        }
      }
    }
  }
  if (idx === -1) return null
  const half = Math.floor((width - matchLen) / 2)
  const start = Math.max(0, idx - half)
  const end = Math.min(source.length, idx + matchLen + half)
  let snippet = source.slice(start, end).replace(/\n/g, ' ')
  if (start > 0) snippet = '…' + snippet
  if (end < source.length) snippet = snippet + '…'
  return snippet
}

function fuzzyMatchScore(query: string, text: string): number | null {
  const normalizedQuery = query.toLowerCase()
  const normalizedText = text.toLowerCase()
  if (normalizedQuery.length === 0) return 0
  if (normalizedQuery.length > normalizedText.length) return null

  let queryIndex = 0
  let score = 0
  let lastMatchIndex = -1
  let consecutiveMatches = 0
  for (let index = 0; index < normalizedText.length && queryIndex < normalizedQuery.length; index++) {
    if (normalizedText[index] !== normalizedQuery[queryIndex]) continue
    const atWordBoundary = index === 0 || /[\s\-_./:]/.test(normalizedText[index - 1]!)
    if (lastMatchIndex === index - 1) {
      consecutiveMatches++
      score -= consecutiveMatches * 5
    } else {
      consecutiveMatches = 0
      if (lastMatchIndex >= 0) score += (index - lastMatchIndex - 1) * 2
    }
    if (atWordBoundary) score -= 10
    score += index * 0.1
    lastMatchIndex = index
    queryIndex++
  }

  if (queryIndex < normalizedQuery.length) return null
  if (normalizedQuery === normalizedText) score -= 100
  return score
}

function modelFuzzyScore(query: string, item: SelectorItem): number | null {
  const tokens = query.trim().split(/[\s/]+/).filter(Boolean)
  if (tokens.length === 0) return 0
  const text = item.searchText ?? `${item.label} ${item.detail ?? ''}`
  let total = 0
  for (const token of tokens) {
    const primary = fuzzyMatchScore(token, text)
    if (primary !== null) {
      total += primary
      continue
    }
    const alphaNumeric = /^(?<letters>[a-z]+)(?<digits>[0-9]+)$/i.exec(token)
    const numericAlpha = /^(?<digits>[0-9]+)(?<letters>[a-z]+)$/i.exec(token)
    const swapped = alphaNumeric
      ? `${alphaNumeric.groups?.digits ?? ''}${alphaNumeric.groups?.letters ?? ''}`
      : numericAlpha
        ? `${numericAlpha.groups?.letters ?? ''}${numericAlpha.groups?.digits ?? ''}`
        : ''
    const swappedScore = swapped ? fuzzyMatchScore(swapped, text) : null
    if (swappedScore === null) return null
    total += swappedScore + 5
  }
  return total
}

function applyFilter(state: SelectorState, query: string): SelectorState {
  // Filtering reorders and hides rows, so an armed delete is always dropped.
  state = { ...state, pendingDeleteId: undefined }
  if (!query) {
    const focusIndex = firstFocusable(state.allItems)
    return { ...state, query, items: state.allItems, focusIndex, scrollOffset: ensureVisible(0, focusIndex, state.allItems.length) }
  }

  if (state.presentation === 'model') {
    // Groups keep their configured order while filtering, so matches stay under
    // the heading they belong to.
    const groupOrder = new Map<string, number>()
    for (const item of state.allItems) {
      const group = item.group ?? ''
      if (!groupOrder.has(group)) groupOrder.set(group, groupOrder.size)
    }
    const matched = state.allItems
      .filter(item => !item.header)
      .map((item, index) => ({ item, index, score: modelFuzzyScore(query, item) }))
      .filter((entry): entry is { item: SelectorItem; index: number; score: number } => entry.score !== null)
      .sort((left, right) => {
        const leftGroup = groupOrder.get(left.item.group ?? '') ?? Number.MAX_SAFE_INTEGER
        const rightGroup = groupOrder.get(right.item.group ?? '') ?? Number.MAX_SAFE_INTEGER
        return leftGroup - rightGroup || left.score - right.score || left.index - right.index
      })
      .map(entry => entry.item)
    const filtered = insertGroupHeaders(state.allItems, matched)
    const focusIndex = firstFocusable(filtered)
    return { ...state, query, items: filtered, focusIndex, scrollOffset: ensureVisible(0, focusIndex, filtered.length) }
  }

  // Whitespace-separated keywords are AND-matched as independent substrings.
  // A single token still supports fuzzy subsequence matching for short labels
  // (e.g. model pickers without searchText).
  const tokens = queryTokens(query)
  const exact: SelectorItem[] = []
  const fuzzy: SelectorItem[] = []
  for (const item of state.allItems) {
    if (item.header) continue
    const text = searchableText(item)
    if (matchesAllTokens(text, tokens)) {
      exact.push(withContext(item, query))
    } else if (tokens.length === 1 && !item.searchText && isSubsequence(text, tokens[0]!)) {
      fuzzy.push(item)
    }
  }
  const matched = exact.concat(fuzzy)
  const filtered = restoreGroupHeaders(state.allItems, matched)
  const focusIndex = firstFocusable(filtered)
  return { ...state, query, items: filtered, focusIndex, scrollOffset: ensureVisible(0, focusIndex, filtered.length) }
}

function restoreGroupHeaders(allItems: SelectorItem[], matched: SelectorItem[]): SelectorItem[] {
  if (!matched.some(item => item.group)) return matched

  const matchedIds = new Set(matched.map(item => item.id ?? item.label))
  const matchedGroups = new Set(matched.flatMap(item => item.group ? [item.group] : []))
  return allItems.flatMap(item => {
    if (item.header) return item.group && matchedGroups.has(item.group) ? [item] : []
    if (!matchedIds.has(item.id ?? item.label)) return []
    return [matched.find(candidate => (candidate.id ?? candidate.label) === (item.id ?? item.label)) ?? item]
  })
}

/** Insert group headings without reordering, so ranked matches keep their
 *  order. Groups are already contiguous when matches are sorted by group. */
function insertGroupHeaders(allItems: SelectorItem[], matched: SelectorItem[]): SelectorItem[] {
  if (!matched.some(item => item.group)) return matched

  const headers = new Map<string, SelectorItem>()
  for (const item of allItems) {
    if (item.header && item.group) headers.set(item.group, item)
  }

  const withHeaders: SelectorItem[] = []
  let current: string | undefined
  for (const item of matched) {
    if (item.group && item.group !== current) {
      current = item.group
      const header = headers.get(item.group)
      if (header) withHeaders.push(header)
    }
    withHeaders.push(item)
  }
  return withHeaders
}

function withContext(item: SelectorItem, query: string): SelectorItem {
  if (!item.searchText) return item
  const ctx = extractContext(item.searchText, query, 80)
  if (!ctx) return item
  return { ...item, detail: `${item.contextPrefix ?? ''}${ctx}` }
}
