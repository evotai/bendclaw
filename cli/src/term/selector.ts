export interface SelectorItem {
  label: string
  detail?: string
}

export interface SelectorState {
  items: SelectorItem[]
  allItems: SelectorItem[]
  focusIndex: number
  title: string
  query: string
}

export function createSelectorState(title: string, items: SelectorItem[], allItems?: SelectorItem[]): SelectorState {
  return { items, allItems: allItems ?? items, focusIndex: 0, title, query: '' }
}

export function selectorUp(state: SelectorState): SelectorState {
  if (state.focusIndex <= 0) return state
  return { ...state, focusIndex: state.focusIndex - 1 }
}

export function selectorDown(state: SelectorState): SelectorState {
  if (state.focusIndex >= state.items.length - 1) return state
  return { ...state, focusIndex: state.focusIndex + 1 }
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

function applyFilter(state: SelectorState, query: string): SelectorState {
  if (!query) {
    return { ...state, query, items: state.allItems, focusIndex: 0 }
  }
  const lower = query.toLowerCase()
  const items = state.allItems.filter(
    (item) =>
      item.label.toLowerCase().includes(lower) ||
      (item.detail?.toLowerCase().includes(lower) ?? false)
  )
  return { ...state, query, items, focusIndex: 0 }
}
