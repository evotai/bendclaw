import { buildOutputBlocks } from './output.js'
import { clipDisplayText } from '../../render/format.js'
import { SELECTOR_OWNER } from '../app/selector-identity.js'
import stringWidth from 'string-width'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { CURSOR_MARKER } from '../render-frame.js'
import { line, block, plain, dim, bold, colored, inverse, blocksToLines, styledLineToAnsi, type ViewBlock, type StyledSpan, type StyledLine } from './types.js'
import { finiteSize, spansWidth, truncateSpansToWidth, truncateToWidth } from './width.js'
import { SELECTOR_VIEWPORT, type SelectorItem, type SelectorState } from '../selector.js'
import { HINT_SEPARATOR, formatChord, type Hint } from '../design/key-hints.js'
import { getTheme } from '../../render/theme/index.js'
import { buildSelectorRow } from './selector-row.js'

/** Render a selector in pi's editorContainer position, never as a modal. */
export function buildSelectorRegionLines(
  state: SelectorState,
  columns: number,
  rows = 24,
  active = true,
): string[] {
  const width = Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 80
  if (state.presentation === 'model') return ['', ...buildModelSelectorRegionLines(state, width, active)]

  const border = styledLineToAnsi(line(dim('─'.repeat(width))))
  if (state.presentation === 'background-list') {
    const budget = Math.max(1, Math.floor(rows) - 10)
    const start = Math.min(state.scrollOffset, state.focusIndex)
    const selected = state.items[state.focusIndex]
    const visible: StyledLine[] = []
    // Walk back from focus to fill the viewport, accounting for activity rows.
    let first = state.focusIndex
    let cost = 0
    for (let i = state.focusIndex; i >= start; i--) {
      const item = state.items[i]!
      const height = item.activity ? 2 : 1
      if (cost + height > budget && cost > 0) break
      first = i
      cost += height
    }
    for (let i = first; i < state.items.length && visible.length < budget; i++) {
      const item = state.items[i]!
      visible.push(item.header ? line(dim(`  ${item.label}`)) : buildSelectorRow(item, { highlighted: i === state.focusIndex, query: '' }))
      if (item.activity && visible.length < budget) visible.push(line(dim(`    ↳ ${clipDisplayText(item.activity, Math.max(1, width - 6))}`)))
    }
    return ['', border,
      styledLineToAnsi(line(bold(state.title), dim(state.subtitle ? ` · ${state.subtitle}` : ''))),
      ...(visible.length ? visible : [line(dim(state.emptyMessage ?? 'No tasks'))]).map(styledLineToAnsi),
      styledLineToAnsi(buildHintLine(selected?.hints ?? state.hints ?? [])), border,
    ].map(text => wrapTextWithAnsi(text, width)[0] ?? '')
  }
  if (state.presentation === 'background-output') {
    return ['', border, ...buildBackgroundOutputRegionLines(state, width, rows), border]
  }

  return ['', border, ...blocksToLines(buildSelectorBlocks(state, width, active)), border]
}

/** Bounded detail: metadata cannot consume the activity viewport or footer. */
function buildBackgroundOutputRegionLines(state: SelectorState, width: number, rows: number): string[] {
  const item = state.items[0]
  const preview = item?.preview ?? ['', '(no output yet)']
  const split = preview.indexOf('')
  const metadata = split < 0 ? [] : preview.slice(0, split)
  const body = state.outputView?.showCommand
    ? metadata.filter(text => !/^  [●✓✗■]/u.test(text) && !text.includes('earlier line') && !text.includes('output file was capped'))
    : split < 0 ? preview : preview.slice(split + 1)
  const paused = state.outputView?.scrollOffset !== undefined
  const end = Math.min(body.length, state.outputView?.scrollOffset ?? body.length)
  const wrapBody = (entries: string[]) => entries.flatMap(entry =>
    wrapTextWithAnsi(entry, Math.max(1, width - 2)).map(text => `${width > 2 ? '  ' : ''}${text}`),
  )
  const wrapped = wrapBody(body.slice(0, end))
  // The outer renderer adds a leading blank and two borders. Leave those out
  // of this budget; keep enough space for activity even on short terminals.
  const budget = Math.max(4, Math.min(24, Math.floor(rows) - 4))
  const title = item?.label ?? state.title
  const status = metadata.find(text => /^  [●✓✗■]/u.test(text)) ?? item?.detail ?? ''
  const warnings = metadata.filter(text => text.includes('output file was capped'))
  const header = [
    styledLineToAnsi(line(bold(clipDisplayText(title, width)))),
    ...buildOutputBlocks([{ id: 'background-status', kind: 'tool', text: status }], { columns: width })
      .flatMap(block => block.lines).map(styledLineToAnsi),
    ...warnings.map(text => styledLineToAnsi(line(colored(clipDisplayText(text, width), 'yellow')))),
  ]
  const bodyBudget = Math.max(1, budget - header.length - 2)
  const visible = wrapped.slice(-bodyBudget)
  const hasEarlier = wrapped.length > visible.length || metadata.some(text => text.includes('earlier line'))
  const position = paused ? 'Paused · End to follow' : hasEarlier ? '… earlier output · ↑ scroll' : ''
  const hints = state.hints ?? [{ keys: 'escape', action: 'back' }]
  return [
    ...header,
    ...visible.map((text, index) => styledLineToAnsi(line(
      index === visible.length - 1 ? plain(text) : dim(text),
    ))),
    styledLineToAnsi(line(dim(position))),
    styledLineToAnsi(buildHintLine(hints)),
  ].map(text => wrapTextWithAnsi(text, width)[0] ?? '')
}

/** Mirrors pi's ModelSelectorComponent hierarchy and line geometry. */
function buildModelSelectorRegionLines(state: SelectorState, width: number, active: boolean): string[] {
  // Keyboard ownership only controls the search caret. The current model row
  // keeps the same shared selection treatment in previews and focused windows.
  const searchFocused = active && state.listFocused !== true
  const border = line(dim('─'.repeat(width)))
  const lines: StyledLine[] = [
    border,
    line(plain('')),
    line(dim('Only showing models from configured providers. Run /login to add cloud models.')),
    line(plain('')),
    buildModelSearchLine(state.query, width, searchFocused),
    line(plain('')),
  ]

  const maxVisible = SELECTOR_VIEWPORT
  const start = Math.max(
    0,
    Math.min(
      state.focusIndex - Math.floor(maxVisible / 2),
      state.items.length - maxVisible,
    ),
  )
  const end = Math.min(start + maxVisible, state.items.length)

  const { accentHex } = getTheme()
  let visibleListRowSeen = false
  for (let index = start; index < end; index++) {
    const item = state.items[index]!
    // Group headings are explicit rows, so the server's own label is shown
    // verbatim rather than reverse-engineered from a row's detail text.
    if (item.header) {
      // The viewport can begin halfway through a large group, with its header
      // scrolled offscreen. Still separate the next group from those rows.
      if (visibleListRowSeen) lines.push(line(plain('')))
      lines.push(line(plain('  '), { text: item.label, hex: accentHex, bold: true }))
      visibleListRowSeen = true
      continue
    }

    const highlighted = index === state.focusIndex
    lines.push(buildSelectorRow(item, {
      highlighted,
      query: state.query,
      dimIdleLabel: true,
      detailGap: ' ',
    }))
    visibleListRowSeen = true
  }

  if (start > 0 || end < state.items.length) {
    // Headings are not choices, so the counter reflects models only.
    const models = state.items.filter(item => !item.header)
    const position = models.indexOf(state.items[state.focusIndex]!) + 1
    lines.push(line(dim(`  (${position}/${models.length})`)))
  }

  if (state.items.length === 0) {
    lines.push(line(dim('  No matching models')))
  } else {
    const selected = state.items[state.focusIndex]
    if (selected && !selected.header) {
      lines.push(line(plain('')))
      lines.push(line(dim(`  Model Name: ${selected.label}`)))
    }
  }

  lines.push(line(plain('')))
  lines.push(border)
  return lines.flatMap((styledLine, index) => {
    const rendered = styledLineToAnsi(styledLine)
    if (!rendered) return ['']
    // pi's Input owns horizontal scrolling and does not pass through Text's
    // wrapping, even when the two-column prompt is wider than the viewport.
    if (index === 4) return [rendered]
    return wrapTextWithAnsi(rendered, width)
  })
}

function buildModelSearchLine(query: string, width: number, active: boolean): StyledLine {
  // pi's Input returns the two-column prompt unchanged when there is no room
  // for input text or a cursor cell.
  if (width <= 2) return line(plain('> '))

  const availableTextWidth = width - 3
  let visibleQuery = ''
  for (const char of [...query].reverse()) {
    if (stringWidth(char + visibleQuery) > availableTextWidth) break
    visibleQuery = char + visibleQuery
  }
  const padding = Math.max(0, width - (active ? 3 : 2) - stringWidth(visibleQuery))
  if (!active) {
    return line(plain('> '), dim(visibleQuery), plain(' '.repeat(padding)))
  }
  return line(
    plain('> '),
    plain(visibleQuery),
    plain(CURSOR_MARKER),
    inverse(' '),
    plain(' '.repeat(padding)),
  )
}

function highlightSpans(text: string, query: string, base: Partial<StyledSpan>): StyledSpan[] {
  if (!query) return [{ text, ...base }]
  const tokens = query.toLowerCase().trim().split(/\s+/).filter(Boolean)
  if (tokens.length === 0) return [{ text, ...base }]

  // Mark every occurrence of every keyword so multi-token filters light up all
  // matched fragments, not only the first contiguous phrase.
  const lower = text.toLowerCase()
  const marks = new Array<boolean>(text.length).fill(false)
  for (const token of tokens) {
    if (!token) continue
    let from = 0
    while (from < lower.length) {
      const idx = lower.indexOf(token, from)
      if (idx === -1) break
      for (let i = idx; i < idx + token.length; i++) marks[i] = true
      from = idx + token.length
    }
  }

  const spans: StyledSpan[] = []
  let index = 0
  while (index < text.length) {
    const marked = marks[index] === true
    let end = index + 1
    while (end < text.length && (marks[end] === true) === marked) end++
    const slice = text.slice(index, end)
    spans.push(marked ? { text: slice, fg: 'yellow', bold: true } : { text: slice, ...base })
    index = end
  }
  return spans.length > 0 ? spans : [{ text, ...base }]
}

export function buildSelectorBlocks(state: SelectorState, columns: number, active = true): ViewBlock[] {
  const selectable = (items: SelectorItem[]) => items.filter(i => !i.header).length
  // A selector that supplies its own hints also owns its header: its counts live
  // in the subtitle and group headings, so the generic row tally beside the
  // title would only restate them.
  const ownsHeader = state.hints !== undefined
  const countLabel = `${selectable(state.items)}${state.query ? ` of ${selectable(state.allItems)}` : ''}`
  const lines: StyledLine[] = [
    ownsHeader
      ? line(colored(state.title, 'cyan', { bold: true }))
      : line(bold(state.title), dim(`  ${countLabel}`)),
  ]

  if (state.subtitle) {
    lines.push(line(dim(state.subtitle)))
  }

  lines.push(line(plain('')))
  // A no-filter list reserves bare letters for actions, so it shows no filter
  // line: offering one would invite typing that goes nowhere.
  if (!state.noFilter) {
    const filterFocused = active && state.listFocused !== true
    if (state.query) {
      lines.push(line(
        colored('Filter  ', 'cyan'),
        plain(state.query),
        ...(filterFocused ? [plain(CURSOR_MARKER), colored('▌', 'cyan')] : []),
      ))
    } else if (filterFocused) {
      lines.push(line(
        colored('Filter  ', 'cyan'),
        plain(CURSOR_MARKER),
        colored('▌', 'cyan'),
        dim(` ${PLACEHOLDER_HINT}`),
      ))
    } else {
      // Nothing typed yet: the filter line doubles as the discoverability hint,
      // otherwise there is no on-screen signal that typing filters at all.
      lines.push(line(colored('Filter  ', 'cyan'), dim(PLACEHOLDER_HINT)))
    }
    lines.push(line(plain('')))
  }

  // The focused row's preview sits beside the list, so rows and pane share the
  // width budget. Without a preview the list keeps the whole line. One column
  // stays free so a full-width row cannot wrap into the next terminal line.
  const available = Math.max(1, finiteSize(columns, 80) - 1)
  const paneWidth = selectorPaneWidth(state, available)
  const listLines = buildSelectorListLines(state)
  if (paneWidth > 0) {
    const preview = state.items[state.focusIndex]?.preview ?? []
    const paneRows = Math.max(listLines.length, PANE_MIN_ROWS)
    lines.push(...joinPaneColumns(
      listLines,
      buildPreviewLines(preview, state.query, paneWidth, paneRows),
      available - paneWidth - PANE_DIVIDER.length,
    ))
  } else {
    lines.push(...listLines)
  }

  lines.push(line(plain('')))
  // A focused row's own hints win over the selector's, so a gesture is only ever
  // offered where it would actually do something. Selectors that predate the
  // hint list keep their hand-written lines below.
  const hints = state.items[state.focusIndex]?.hints ?? state.hints
  if (hints) {
    lines.push(buildHintLine(hints))
  } else if (state.owner === SELECTOR_OWNER.queue) {
    lines.push(line(
      colored('enter', 'cyan'), dim(' edit   '),
      colored('Ctrl+D', 'cyan'), dim(' remove   '),
      colored('esc', 'cyan'), dim(' close'),
    ))
  } else {
    lines.push(line(
      colored('↑↓', 'cyan'), dim(state.circularNavigation ? ' move · wraps   ' : ' move   '),
      colored('enter', 'cyan'), dim(' select   '),
      colored('type', 'cyan'), dim(' filter   '),
      colored('esc', 'cyan'), dim(' close'),
    ))
  }
  return [block(lines, 1)]
}

/**
 * Render hints as `↑/↓ to select · Enter to view output · Esc to close`.
 *
 * The key is coloured and the rest dimmed, so the line scans as a row of keys
 * rather than a sentence.
 */
function buildHintLine(hints: Hint[]): StyledLine {
  const spans: StyledSpan[] = []
  for (const hint of hints) {
    if (spans.length > 0) spans.push(dim(HINT_SEPARATOR))
    spans.push(colored(formatChord(hint.keys), 'cyan'), dim(` to ${hint.action}`))
  }
  return line(...spans)
}

/** The list rows themselves — everything between the filter line and the hints. */
function buildSelectorListLines(state: SelectorState): StyledLine[] {
  if (state.items.length === 0) {
    if (state.emptyMessage) return [line(dim(`  ${state.emptyMessage}`))]
    // A no-filter list has no query to explain an empty result, so the generic
    // "no matching items" would misdescribe it.
    if (state.noFilter) return []
    return [line(dim('  No matching items'))]
  }

  const lines: StyledLine[] = []
  const maxVisible = SELECTOR_VIEWPORT
  // The window follows scrollOffset (updated one row at a time by up/down),
  // clamped defensively so the focused row is always on screen.
  let start = Math.min(Math.max(state.scrollOffset, 0), Math.max(0, state.items.length - maxVisible))
  if (state.focusIndex < start) start = state.focusIndex
  else if (state.focusIndex >= start + maxVisible) start = state.focusIndex - maxVisible + 1
  const end = Math.min(start + maxVisible, state.items.length)

  if (start > 0) {
    lines.push(line(dim(`  ↑ ${start} above`)))
  }
  let seenRow = false
  for (let i = start; i < end; i++) {
    const item = state.items[i]!
    if (item.header) {
      if (!item.label) {
        lines.push(line(plain('')))
      } else if (item.headerCount !== undefined) {
        // A counted group reads as a heading over its rows, so the label is bold
        // and the tally dim rather than wrapped in a divider rule. Groups after
        // the first are separated by a blank line; the viewport can start
        // mid-list, so this keys off what is actually on screen.
        if (seenRow) lines.push(line(plain('')))
        lines.push(line(bold(`  ${item.label}`), dim(` (${item.headerCount})`)))
        seenRow = true
      } else {
        lines.push(line(dim(`── ${item.label} ──`)))
      }
      continue
    }
    seenRow = true
    const highlighted = i === state.focusIndex
    lines.push(buildSelectorRow(item, {
      highlighted,
      query: state.query,
    }))
  }
  if (end < state.items.length) {
    lines.push(line(dim(`  ↓ ${state.items.length - end} below`)))
  }
  return lines
}

/** Placeholder shown on the filter line before anything is typed. */
const PLACEHOLDER_HINT = 'type to search titles, prompts and transcript text'

/** Gap and rail between the list and its preview pane. */
const PANE_DIVIDER = '  │ '

/**
 * Rows the pane keeps even when the list is shorter. Session summaries need a
 * few lines of body text to be worth reading; a two-row pane beside a two-row
 * list would only ever show metadata.
 */
const PANE_MIN_ROWS = SELECTOR_VIEWPORT

/** Columns the list keeps for itself before a pane is worth showing. */
const PANE_MIN_LIST_WIDTH = 46

/** Widest the pane grows to. Beyond this, extra columns go back to the list. */
const PANE_MAX_WIDTH = 52

/**
 * Pane width in columns, or 0 when the pane is suppressed. The pane takes a
 * third of the terminal, but never at the cost of the list becoming unreadable
 * — narrow terminals keep the single-column layout they had before.
 */
function selectorPaneWidth(state: SelectorState, columns: number): number {
  const focused = state.items[state.focusIndex]
  if (!focused?.preview || focused.preview.length === 0) return 0
  const width = Math.min(PANE_MAX_WIDTH, Math.floor(columns / 3))
  if (width < 24) return 0
  if (columns - width - PANE_DIVIDER.length < PANE_MIN_LIST_WIDTH) return 0
  return width
}

/**
 * Rows the pane header may claim when there is a body to show. A long title
 * would otherwise push the body out entirely, and the body is the reason the
 * pane exists.
 */
const PANE_MAX_HEADER_ROWS = 4

/**
 * Rows one body entry may claim. A single long turn (a pasted draft, a spec)
 * would otherwise fill the pane by itself and read as an unattributed wall of
 * text, hiding every other turn in the session.
 */
const PANE_MAX_ENTRY_ROWS = 3

/**
 * Render a preview into the pane. Entries before the first blank are the header
 * and stay pinned; the rest is the body.
 *
 * The body is windowed by entry, never by wrapped row: an entry leads with a
 * marker (`› `) that attributes the text, so cutting into the middle of one
 * leaves orphaned continuation rows that read as noise. Entries are ordered
 * oldest-first, so an overflowing body keeps its tail — the latest turns say
 * where the session left off. While filtering, the window moves to the first
 * matching entry instead, so the user can see why the row matched. Either cut
 * is marked with `⋮`.
 */
function buildPreviewLines(preview: string[], query: string, width: number, maxRows: number): StyledLine[] {
  const split = preview.indexOf('')
  const header = split === -1 ? preview : preview.slice(0, split)
  const body = split === -1 ? [] : preview.slice(split + 1)

  const headerRows = header
    .flatMap((entry, index) => wrapTextWithAnsi(entry, width).map(row => ({ row, heading: index === 0 })))
    .slice(0, body.length > 0 ? Math.min(PANE_MAX_HEADER_ROWS, maxRows - 1) : maxRows)
    .map(({ row, heading }) => line(...(
      query
        ? highlightSpans(row, query, heading ? { bold: true } : { dim: true })
        : [heading ? bold(row) : dim(row)]
    )))

  // A blank row separates header from body, so the body's budget is one less.
  const budget = Math.max(0, maxRows - headerRows.length - 1)
  const entries = body.map(entry => entry ? wrapPreviewEntry(entry, width) : [''])
  const total = entries.reduce((sum, rows) => sum + rows.length, 0)
  if (entries.length === 0 || budget === 0) return headerRows
  if (total <= budget) {
    return [
      ...headerRows,
      line(plain('')),
      ...entries.flat().map(row => previewBodyLine(row, query)),
    ]
  }

  // One row of the budget goes to the `⋮` marker so the cut is visible.
  const visible = selectPreviewEntries(entries, query, budget - 1)
  return [
    ...headerRows,
    line(dim('  ⋮')),
    ...visible.map(row => previewBodyLine(row, query)),
  ]
}

/**
 * Whole entries that fit `budget` rows: the earliest filter hit onwards, else
 * the latest turns. Walking forward from the chosen anchor keeps entries in
 * conversation order and stops before one would be cut in half.
 */
function selectPreviewEntries(entries: string[][], query: string, budget: number): string[] {
  const anchor = previewAnchorEntry(entries, query, budget)
  const rows: string[] = []
  for (let i = anchor; i < entries.length; i++) {
    const entry = entries[i]!
    if (rows.length + entry.length > budget) break
    rows.push(...entry)
  }
  // A single entry taller than the whole budget still has to say something, so
  // it is truncated rather than dropped.
  if (rows.length === 0) return (entries[anchor] ?? []).slice(0, budget)
  return rows
}

/** Index of the first entry to show: the earliest filter hit, else the tail. */
function previewAnchorEntry(entries: string[][], query: string, budget: number): number {
  const tokens = query.toLowerCase().trim().split(/\s+/).filter(Boolean)
  if (tokens.length > 0) {
    const hit = entries.findIndex(entry => {
      const text = entry.join(' ').toLowerCase()
      return tokens.some(token => text.includes(token))
    })
    if (hit !== -1) return hit
  }
  // Walk back from the end while whole entries still fit.
  let used = 0
  let anchor = entries.length
  while (anchor > 0) {
    const rows = entries[anchor - 1]?.length ?? 0
    if (used + rows > budget) break
    used += rows
    anchor--
  }
  return Math.min(anchor, entries.length - 1)
}

function previewBodyLine(row: string, query: string): StyledLine {
  return line(...(query ? highlightSpans(row, query, { dim: true }) : [dim(row)]))
}

/**
 * Wrap one preview entry, indenting continuations under a leading marker so a
 * long entry reads as one block instead of merging into the next. Capped at
 * [`PANE_MAX_ENTRY_ROWS`], with the cut marked in place.
 */
function wrapPreviewEntry(entry: string, width: number): string[] {
  const marker = /^(\S\s)/.exec(entry)?.[1]
  const rows = marker
    ? wrapTextWithAnsi(entry.slice(marker.length), Math.max(1, width - marker.length))
      .map((row, index) => `${index === 0 ? marker : ' '.repeat(marker.length)}${row}`)
    : wrapTextWithAnsi(entry, width)
  if (rows.length <= PANE_MAX_ENTRY_ROWS) return rows

  const kept = rows.slice(0, PANE_MAX_ENTRY_ROWS)
  const last = kept[PANE_MAX_ENTRY_ROWS - 1] ?? ''
  kept[PANE_MAX_ENTRY_ROWS - 1] = `${truncateToWidth(last, Math.max(1, width - 1))}…`
  return kept
}

/**
 * Lay the list and pane side by side. Each list row is padded to `listWidth`
 * so the divider column stays straight regardless of row content, and a row's
 * own background (model presentation) is not used here, so plain padding is
 * enough.
 */
function joinPaneColumns(listLines: StyledLine[], paneLines: StyledLine[], listWidth: number): StyledLine[] {
  const rows = Math.max(listLines.length, paneLines.length)
  const result: StyledLine[] = []
  for (let i = 0; i < rows; i++) {
    const left = listLines[i]?.spans ?? [plain('')]
    const right = paneLines[i]?.spans ?? [plain('')]
    const spans = spansWidth(left) > listWidth ? truncateSpansToWidth(left, listWidth) : left
    const padding = Math.max(0, listWidth - spansWidth(spans))
    // Layout only supplies spacing; the shared row component exclusively owns
    // selection styling, so pane-backed rows look exactly like model rows.
    result.push(line(...spans, plain(' '.repeat(padding)), dim(PANE_DIVIDER), ...right))
  }
  return result
}