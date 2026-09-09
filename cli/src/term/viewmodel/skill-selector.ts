import stringWidth from 'string-width'
import { getTheme } from '../../render/theme/index.js'
import { clipDisplayText } from '../../render/format.js'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { CURSOR_MARKER } from '../render-frame.js'
import { SELECTOR_VIEWPORT, type SelectorState } from '../selector.js'
import { buildSelectorRow } from './selector-row.js'
import { styledLineToAnsi } from './types.js'

/** Source-grouped tree on the left, bounded usage details on the right. */
export function buildSkillSelectorLines(state: SelectorState, width: number, rows: number, active: boolean): string[] {
  const theme = getTheme()
  const muted = (text: string) => theme.thinkText.paint(text)
  const wide = width >= 76
  const listWidth = wide ? Math.min(40, Math.floor(width * 0.44)) : Math.max(1, width - 2)
  const detailWidth = wide ? Math.min(56, width - listWidth - 5) : Math.max(1, width - 2)
  const clip = (text: string, size = width) => clipDisplayText(text, Math.max(0, size))
  const budget = Math.max(1, Math.min(SELECTOR_VIEWPORT, Math.floor(rows) - (wide ? 14 : 20)))
  let start = Math.min(Math.max(0, state.scrollOffset), Math.max(0, state.items.length - budget))
  if (state.focusIndex < start) start = state.focusIndex
  if (state.focusIndex >= start + budget) start = state.focusIndex - budget + 1
  const end = Math.min(state.items.length, start + budget)
  const list: string[] = []
  let category: string | undefined
  for (let index = start; index < end; index++) {
    const item = state.items[index]!
    const source = item.badge === 'official' ? 'Official' : 'Custom'
    if (source !== category) {
      if (category) list.push('')
      list.push(theme.accent.paint(`  ${source}`))
      category = source
    }
    const child = Boolean(item.group && item.expanded === undefined)
    // Preserve package context when its parent has scrolled out of the window.
    if (index === start && child) list.push(muted(clip(`    ${item.group}/`, listWidth)))
    const groupOpen = item.expanded !== undefined && (item.expanded || Boolean(state.query.trim()))
    const prefix = item.expanded !== undefined ? (groupOpen ? '▾ ' : '▸ ') : child ? '    ' : ''
    // Official source is already shown by the section heading; children inherit parent badges.
    const badge = item.badge && item.badge !== 'official' && !child ? ` [${item.badge}]` : ''
    const label = clip(prefix + item.label, listWidth - 2 - badge.length)
    let text = styledLineToAnsi(buildSelectorRow({ ...item, label, detail: undefined }, {
      highlighted: index === state.focusIndex, query: state.query,
    }))
    if (badge) text += theme.accent.paint(badge)
    list.push(text)
  }
  if (!state.items.length) list.push(muted(state.query ? '  No matching skills' : `  ${state.emptyMessage ?? 'No skills installed'}`))
  if (start > 0 || end < state.items.length) list.push(muted(`  ${start + 1}–${end} · ↑↓ scroll`))

  const selected = state.items[state.focusIndex]
  const preview = selected?.preview ?? []
  const details: string[] = []
  const append = (text: string, limit: number, paint: (text: string) => string): void => {
    const wrapped = wrapTextWithAnsi(text, detailWidth)
    const shown = wrapped.slice(0, limit)
    if (wrapped.length > limit && shown.length) {
      shown[shown.length - 1] = `${clip(shown[shown.length - 1] ?? '', detailWidth - 2)} …`
    }
    details.push(...shown.map(paint))
  }
  if (selected) {
    append(preview[0] ?? selected.label, 1, theme.brandBold.paint)
    details.push('')
    append(preview[2] ?? 'No description available.', 3, theme.text.paint)
    if (preview[5]) {
      details.push('', muted('Example'))
      append(preview[5], 3, theme.text.paint)
    }
  }
  const body: string[] = []
  if (wide) {
    const height = Math.max(list.length, details.length, Math.min(budget + 2, 8))
    for (let index = 0; index < height; index++) {
      const left = list[index] ?? ''
      body.push(left + ' '.repeat(Math.max(0, listWidth - stringWidth(left))) + muted('  │  ') + (details[index] ?? ''))
    }
  } else {
    body.push(...list, ...(details.length ? ['', ...details.map(text => `  ${text}`)] : []))
  }
  const focused = active && state.listFocused !== true
  const cursor = focused ? CURSOR_MARKER : ''
  const search = state.query ? theme.text.paint(clip(state.query, width - 4)) : muted('Search skills…')
  return [
    theme.accentBold.paint('Skills'),
    `  ${cursor}${search}`,
    '',
    ...body,
    '',
    muted(clip(selected?.expanded !== undefined && !state.query
      ? `↑↓ move · Enter ${selected.expanded ? 'collapse' : 'expand'} · Esc close`
      : '↑↓ move · type to search · Esc close · /skill list manage')),
  ].map(text => wrapTextWithAnsi(text, width)[0] ?? '')
}
