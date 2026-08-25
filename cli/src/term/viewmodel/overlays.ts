import stringWidth from 'string-width'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { CURSOR_MARKER } from '../renderer.js'
import { line, block, plain, dim, bold, colored, inverse, blocksToLines, styledLineToAnsi, type ViewBlock, type StyledSpan, type StyledLine } from './types.js'
import { SELECTOR_VIEWPORT, type SelectorItem, type SelectorState } from '../selector.js'
import type { AskState } from '../ask.js'
import { getTheme } from '../../render/theme.js'

export type OverlayState =
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'selector'; state: SelectorState }
  | { kind: 'ask-user'; state: AskState }

export function buildOverlayBlocks(overlay: OverlayState, columns: number): ViewBlock[] {
  switch (overlay.kind) {
    case 'none':
      return []
    case 'help':
      return buildHelpBlocks(columns)
    case 'selector':
      return buildSelectorBlocks(overlay.state, columns)
    case 'ask-user':
      return buildAskBlocks(overlay.state, columns)
  }
}

export function buildAskRegionLines(state: AskState, columns: number): string[] {
  const width = Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 80
  const border = styledLineToAnsi(line(dim('─'.repeat(width))))
  const body = blocksToLines(buildAskBlocks(state, width))
    .flatMap(rendered => wrapTextWithAnsi(rendered, width))
  return ['', border, ...body, border]
}

/** Render a selector in pi's editorContainer position, never as a modal. */
export function buildSelectorRegionLines(state: SelectorState, columns: number): string[] {
  const width = Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 80
  if (state.presentation === 'model') return ['', ...buildModelSelectorRegionLines(state, width)]

  const border = styledLineToAnsi(line(dim('─'.repeat(width))))
  return ['', border, ...blocksToLines(buildSelectorBlocks(state, width)), border]
}

/** Mirrors pi's ModelSelectorComponent hierarchy and line geometry. */
function buildModelSelectorRegionLines(state: SelectorState, width: number): string[] {
  const border = line(dim('─'.repeat(width)))
  const lines: StyledLine[] = [
    border,
    line(plain('')),
    line(dim('Only showing models from configured providers. Run /login to add cloud models.')),
    line(plain('')),
    buildModelSearchLine(state.query, width),
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

  const { brandHex, accentHex, selectionBgHex, selectionMutedHex } = getTheme()
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

    const focused = index === state.focusIndex
    const bg = focused ? selectionBgHex : undefined
    const prefix = focused
      ? { text: '❯ ', hex: brandHex, bold: true, bg }
      : plain('  ')
    const name = focused
      ? { text: item.label, hex: brandHex, bold: true, bg }
      : dim(item.label)
    const detail = item.detail
      ? focused
        ? { text: ` ${item.detail}`, hex: selectionMutedHex, bg }
        : dim(` ${item.detail}`)
      : undefined
    const check = item.selected
      ? { text: ' ✓', hex: brandHex, bold: true, ...(bg ? { bg } : {}) }
      : undefined
    lines.push({
      spans: [prefix, name, ...(detail ? [detail] : []), ...(check ? [check] : [])],
      ...(bg ? { bg } : {}),
    })
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

function buildModelSearchLine(query: string, width: number): StyledLine {
  // pi's Input returns the two-column prompt unchanged when there is no room
  // for input text or a cursor cell.
  if (width <= 2) return line(plain('> '))

  const availableTextWidth = width - 3
  let visibleQuery = ''
  for (const char of [...query].reverse()) {
    if (stringWidth(char + visibleQuery) > availableTextWidth) break
    visibleQuery = char + visibleQuery
  }
  const padding = Math.max(0, width - 3 - stringWidth(visibleQuery))
  return line(
    plain('> '),
    plain(visibleQuery),
    plain(CURSOR_MARKER),
    inverse(' '),
    plain(' '.repeat(padding)),
  )
}

function buildHelpBlocks(columns: number): ViewBlock[] {
  const entries = [
    ['Enter', 'Submit message'],
    ['Alt+Enter', 'Insert newline'],
    ['Ctrl+C', 'Clear / Exit (×2)'],
    ['Esc', 'Clear input / Dismiss / Interrupt'],
    ['Ctrl+B', 'Focus queued prompts when non-empty'],
    ['↑ / ↓', 'History navigation / multi-line'],
    ['Tab', 'Complete command / path'],
    ['Ctrl+U', 'Clear line before cursor'],
    ['Ctrl+K', 'Clear line after cursor'],
    ['Ctrl+W', 'Delete word before cursor'],
    ['Ctrl+D', 'Delete char / Exit if empty'],
    ['Ctrl+A/E', 'Move to start/end of line'],
    ['Ctrl+L', 'Clear all input'],
    ['Ctrl+O', 'Expand/collapse output'],
    ['Shift+Tab', 'Cycle thinking level'],
    ['/help', 'Show this help'],
    ['/model <name>', 'Switch model'],
    ['/resume [id|query]', 'Resume session'],
    ['/new', 'Start new session'],
    ['/plan', 'Toggle planning mode'],
    ['/env', 'Manage variables'],
    ['/skill', 'Manage skills'],
    ['/copy', 'Copy last agent message (Markdown)'],
    ['/clip [all]', 'Clip last reply to vault; all = distill session'],
    ['/share [id|url]', 'Share or import a session'],
    ['/compact', 'Compact session context'],
    ['/update', 'Update evot'],
    ['/login', 'Log in to evot cloud'],
    ['/logout', 'Log out of evot cloud'],
    ['/clear', 'Clear session context'],
    ['/exit', 'Exit'],
  ]

  const maxKeyLen = Math.max(...entries.map(e => e[0]!.length))
  const lines = [
    line(bold('  Keyboard Shortcuts & Commands')),
    line(plain('')),
    ...entries.map(([key, desc]) =>
      line(colored(`  ${key!.padEnd(maxKeyLen + 2)}`, 'cyan'), dim(desc!))
    ),
    line(plain('')),
    line(dim('  Press Esc to dismiss')),
  ]

  return [block(lines, 1)]
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

function buildSelectorBlocks(state: SelectorState, _columns: number): ViewBlock[] {
  const selectable = (items: SelectorItem[]) => items.filter(i => !i.header).length
  const countLabel = `${selectable(state.items)}${state.query ? ` of ${selectable(state.allItems)}` : ''}`
  const lines: StyledLine[] = [
    line(bold(state.title), dim(`  ${countLabel}`)),
  ]

  if (state.subtitle) {
    lines.push(line(dim(state.subtitle)))
  }

  lines.push(line(plain('')))
  if (state.query) {
    lines.push(line(colored('Filter  ', 'cyan'), plain(state.query), colored('▌', 'cyan')))
    lines.push(line(plain('')))
  }

  if (state.items.length === 0) {
    lines.push(line(dim('  No matching items')))
  } else {
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
    for (let i = start; i < end; i++) {
      const item = state.items[i]!
      if (item.header) {
        lines.push(item.label ? line(dim(`── ${item.label} ──`)) : line(plain('')))
        continue
      }
      const focused = i === state.focusIndex
      const prefix: StyledSpan = focused ? colored('❯ ', 'cyan', { bold: true }) : plain('  ')
      const labelSpans = state.query
        ? highlightSpans(item.label, state.query, focused ? { bold: true } : {})
        : [focused ? bold(item.label) : plain(item.label)]
      const detailSpans = item.detail && state.query
        ? highlightSpans(`  ${item.detail}`, state.query, { dim: true })
        : [item.detail ? dim(`  ${item.detail}`) : plain('')]
      const selectedSpans = item.selected
        ? [plain(' '), colored('✓', 'green')]
        : []
      lines.push(line(prefix, ...labelSpans, ...detailSpans, ...selectedSpans))
    }
    if (end < state.items.length) {
      lines.push(line(dim(`  ↓ ${state.items.length - end} below`)))
    }
  }

  lines.push(line(plain('')))
  if (state.title === 'Prompt queue') {
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

const CHECKBOX_ON = '☒'
const CHECKBOX_OFF = '☐'
const TICK = '✓'
const POINTER = '❯'
const BULLET = '•'
const ARROW_RIGHT = '→'

function optionCountStringWidth(count: number): number {
  return count.toString().length
}

function optionIndexText(index: number, maxIndexWidth: number): string {
  return `${index}.`.padEnd(maxIndexWidth + 2)
}

function appendTick(spans: StyledSpan[]): StyledSpan[] {
  return [...spans, colored(TICK, 'green')]
}

function selectedAnswerKind(state: AskState, questionIndex: number): 'option' | 'other' | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  if (answer.customText !== null) return 'other'
  if (answer.selectedOption !== null) return 'option'
  return null
}

function selectedOptionIndex(state: AskState, questionIndex: number): number | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  return answer.selectedOption
}

function selectedAnswerText(state: AskState, questionIndex: number): string | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  if (answer.customText !== null) return answer.customText
  if (answer.selectedOption !== null) return state.questions[questionIndex]?.options[answer.selectedOption]?.label ?? null
  return null
}

function isAnswered(state: AskState, index: number): boolean {
  const a = state.answers[index]
  return a !== undefined && (a.selectedOption !== null || a.customText !== null)
}

export function buildAskBlocks(state: AskState, columns: number): ViewBlock[] {
  const result: StyledLine[] = []
  const isMulti = state.questions.length > 1

  // ── Tab bar (multi-question only) ──────────────────────────────
  if (isMulti) {
    const tabLine: StyledSpan[] = []

    // Left arrow
    const canGoLeft = state.currentTab > 0 || state.onSubmitTab
    tabLine.push(canGoLeft ? plain('← ') : dim('← '))

    // Tabs with checkboxes
    for (let i = 0; i < state.questions.length; i++) {
      if (i > 0) tabLine.push(plain('  '))
      const qq = state.questions[i]!
      const active = !state.onSubmitTab && i === state.currentTab
      const answered = isAnswered(state, i)
      const checkbox = answered ? CHECKBOX_ON : CHECKBOX_OFF
      if (active) {
        tabLine.push(inverse(` ${checkbox} ${qq.header} `))
      } else {
        tabLine.push(plain(` ${checkbox} ${qq.header} `))
      }
    }

    // Submit tab
    tabLine.push(plain('  '))
    if (state.onSubmitTab) {
      tabLine.push(inverse(` ${TICK} Submit `))
    } else {
      tabLine.push(plain(` ${TICK} Submit `))
    }

    // Right arrow
    const canGoRight = !state.onSubmitTab
    tabLine.push(canGoRight ? plain(' →') : dim(' →'))

    result.push(line(...tabLine))
    result.push(line(plain('')))
  }

  // ── Submit review page ─────────────────────────────────────────
  if (state.onSubmitTab) {
    const allAnswered = state.questions.every((_, i) => isAnswered(state, i))

    result.push(line(bold('Review your answers')))
    result.push(line(plain('')))

    if (!allAnswered) {
      result.push(line(colored('⚠ You have not answered all questions', 'yellow')))
      result.push(line(plain('')))
    }

    for (let i = 0; i < state.questions.length; i++) {
      const qq = state.questions[i]!
      const answerText = selectedAnswerText(state, i)
      if (!answerText) continue
      result.push(line(plain(`  ${BULLET} ${qq.question}`)))
      result.push(line(colored(`    ${ARROW_RIGHT} ${answerText}`, 'green')))
    }

    result.push(line(plain('')))
    result.push(line(dim('Ready to submit your answers?')))
    result.push(line(plain('')))

    // Submit / Cancel options
    const submitFocused = state.submitFocus === 0
    const cancelFocused = state.submitFocus === 1
    result.push(line(
      submitFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      submitFocused ? bold('Submit answers') : plain('Submit answers')
    ))
    result.push(line(
      cancelFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      cancelFocused ? bold('Cancel') : plain('Cancel')
    ))

    result.push(line(plain('')))
    result.push(line(dim('↑↓ navigate · enter select · ← back · esc cancel')))

    return [block(result, 1)]
  }

  // ── Question view ──────────────────────────────────────────────
  const q = state.questions[state.currentTab]!

  // ── Question text ──────────────────────────────────────────────
  result.push(line(bold(q.question)))
  result.push(line(plain('')))

  const ui = state.uiStates.get(state.currentTab) ?? { focusIndex: 0, inOtherMode: false, otherText: '', otherCursor: 0 }
  const selectedKind = selectedAnswerKind(state, state.currentTab)
  const selectedIndex = selectedOptionIndex(state, state.currentTab)
  const selectedText = selectedAnswerText(state, state.currentTab)
  const maxIndexWidth = optionCountStringWidth(q.options.length + 1)

  // ── Options ────────────────────────────────────────────────────
  for (let i = 0; i < q.options.length; i++) {
    const opt = q.options[i]!
    const focused = !ui.inOtherMode && i === state.focusIndex
    const selected = selectedKind === 'option' && selectedIndex === i
    const spans: StyledSpan[] = [
      focused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      dim(optionIndexText(i + 1, maxIndexWidth)),
      selected
        ? colored(opt.label, 'green')
        : focused
          ? colored(opt.label, 'cyan')
          : plain(opt.label),
    ]
    if (opt.description) spans.push(dim(` — ${opt.description}`))
    result.push(line(...(selected ? appendTick(spans) : spans)))
  }

  // ── Other ──────────────────────────────────────────────────────
  const otherSelected = selectedKind === 'other'
  const otherFocused = ui.inOtherMode
  const otherText = otherFocused ? ui.otherText : otherSelected ? selectedText ?? '' : ui.otherText
  const otherSpans: StyledSpan[] = [
    otherFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
    dim(optionIndexText(q.options.length + 1, maxIndexWidth)),
  ]
  if (otherFocused) {
    if (otherText) {
      const cursor = ui.otherCursor ?? otherText.length
      const before = otherText.slice(0, cursor)
      const atCursor = otherText[cursor] ?? ' '
      const after = otherText.slice(cursor + 1)
      if (before) otherSpans.push(plain(before))
      otherSpans.push(inverse(atCursor))
      if (after) otherSpans.push(plain(after))
    } else {
      otherSpans.push(inverse(' '), dim('Type something.'))
    }
  } else {
    otherSpans.push(otherSelected ? colored(otherText || 'Type something.', 'green') : dim(otherText || 'Type something.'))
  }
  result.push(line(...(otherSelected ? appendTick(otherSpans) : otherSpans)))

  result.push(line(plain('')))

  // ── Footer hint ────────────────────────────────────────────────
  if (isMulti) {
    result.push(line(dim('↑↓ navigate · ←→ switch tab · enter select · esc cancel')))
  } else {
    result.push(line(dim('↑↓ navigate · enter select · esc cancel')))
  }

  return [block(result, 1)]
}
