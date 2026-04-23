export interface SelectorItem {
  label: string
  detail?: string
  /** Opaque identifier (e.g. full session id) — not displayed. */
  id?: string
  /** Extra text searched but not displayed (e.g. full session id, cwd). */
  searchText?: string
  /** When false, up/down navigation skips this item. Defaults to true. */
  focusable?: boolean
}

export interface SelectorState {
  items: SelectorItem[]
  allItems: SelectorItem[]
  focusIndex: number
  title: string
  query: string
}

/** Find the first focusable index in items, defaulting to 0. */
function firstFocusable(items: SelectorItem[]): number {
  const idx = items.findIndex(i => i.focusable !== false)
  return idx >= 0 ? idx : 0
}

export function createSelectorState(title: string, items: SelectorItem[], allItems?: SelectorItem[], initialQuery?: string): SelectorState {
  const all = allItems ?? items
  if (initialQuery) {
    return applyFilter({ items: all, allItems: all, focusIndex: 0, title, query: '' }, initialQuery)
  }
  return { items, allItems: all, focusIndex: firstFocusable(items), title, query: '' }
}

export function selectorUp(state: SelectorState): SelectorState {
  let next = state.focusIndex - 1
  while (next >= 0 && state.items[next]?.focusable === false) next--
  if (next < 0) return state
  return { ...state, focusIndex: next }
}

export function selectorDown(state: SelectorState): SelectorState {
  let next = state.focusIndex + 1
  while (next < state.items.length && state.items[next]?.focusable === false) next++
  if (next >= state.items.length) return state
  return { ...state, focusIndex: next }
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
  const updated = { ...state, allItems }
  return state.query ? applyFilter(updated, state.query) : { ...updated, items: allItems }
}

export function selectorClearQuery(state: SelectorState): SelectorState {
  if (!state.query) return state
  return applyFilter(state, '')
}

export function selectorRemoveItem(state: SelectorState, index: number): SelectorState {
  const label = state.items[index]?.label
  if (!label) return state
  const items = state.items.filter((_, i) => i !== index)
  const allItems = state.allItems.filter(i => i.label !== label)
  const focusIndex = Math.min(state.focusIndex, Math.max(0, items.length - 1))
  return { ...state, items, allItems, focusIndex }
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

function extractContext(source: string, query: string, width: number): string | null {
  const lower = source.toLowerCase()
  const idx = lower.indexOf(query.toLowerCase())
  if (idx === -1) return null
  const half = Math.floor((width - query.length) / 2)
  const start = Math.max(0, idx - half)
  const end = Math.min(source.length, idx + query.length + half)
  let snippet = source.slice(start, end).replace(/\n/g, ' ')
  if (start > 0) snippet = '…' + snippet
  if (end < source.length) snippet = snippet + '…'
  return snippet
}

function applyFilter(state: SelectorState, query: string): SelectorState {
  if (!query) {
    return { ...state, query, items: state.allItems, focusIndex: firstFocusable(state.allItems) }
  }
  const lower = query.toLowerCase()
  const exact: SelectorItem[] = []
  const fuzzy: SelectorItem[] = []
  for (const item of state.allItems) {
    const text = searchableText(item)
    if (text.includes(lower)) {
      exact.push(withContext(item, query))
    } else if (!item.searchText && isSubsequence(text, lower)) {
      fuzzy.push(item)
    }
  }
  const filtered = exact.concat(fuzzy)
  return { ...state, query, items: filtered, focusIndex: firstFocusable(filtered) }
}

function withContext(item: SelectorItem, query: string): SelectorItem {
  if (!item.searchText) return item
  const ctx = extractContext(item.searchText, query, 80)
  if (!ctx) return item
  return { ...item, detail: ctx }
}
