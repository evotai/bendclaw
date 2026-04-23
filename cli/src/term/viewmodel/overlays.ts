import { line, block, plain, dim, bold, colored, inverse, type ViewBlock, type StyledSpan, type StyledLine } from './types.js'
import type { SelectorState } from '../selector.js'
import type { AskState } from '../ask.js'

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
      return buildSelectorBlocks(overlay.state)
    case 'ask-user':
      return buildAskBlocks(overlay.state, columns)
  }
}

function buildHelpBlocks(columns: number): ViewBlock[] {
  const entries = [
    ['Enter', 'Submit message'],
    ['Alt+Enter', 'Insert newline'],
    ['Ctrl+C', 'Clear / Exit (×2)'],
    ['Esc', 'Clear input / Dismiss / Interrupt'],
    ['↑ / ↓', 'History navigation / multi-line'],
    ['Tab', 'Complete command / path'],
    ['Ctrl+U', 'Clear line before cursor'],
    ['Ctrl+K', 'Clear line after cursor'],
    ['Ctrl+W', 'Delete word before cursor'],
    ['Ctrl+D', 'Delete char / Exit if empty'],
    ['Ctrl+A/E', 'Move to start/end of line'],
    ['Ctrl+L', 'Clear all input'],
    ['Ctrl+O', 'Toggle verbose mode'],
    ['/help', 'Show this help'],
    ['/model <name>', 'Switch model'],
    ['/resume [id]', 'Resume session'],
    ['/new', 'Start new session'],
    ['/goto <n>', 'Go to message'],
    ['/history [n]', 'Show recent messages'],
    ['/compact', 'Compact context'],
    ['/plan', 'Toggle planning mode'],
    ['/env', 'Manage variables'],
    ['/skill', 'Manage skills'],
    ['/update', 'Update evot'],
    ['/verbose', 'Toggle verbose mode'],
    ['/clear', 'Clear screen'],
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
  const lower = text.toLowerCase()
  const lowerQuery = query.toLowerCase()
  const idx = lower.indexOf(lowerQuery)
  if (idx === -1) return [{ text, ...base }]
  const spans: StyledSpan[] = []
  if (idx > 0) spans.push({ text: text.slice(0, idx), ...base })
  spans.push({ text: text.slice(idx, idx + lowerQuery.length), fg: 'yellow', bold: true })
  if (idx + lowerQuery.length < text.length) spans.push({ text: text.slice(idx + lowerQuery.length), ...base })
  return spans
}

function buildSelectorBlocks(state: SelectorState): ViewBlock[] {
  const lines = [
    line(bold(state.title)),
  ]

  if (state.query) {
    lines.push(line(dim('  search: '), plain(state.query)))
  }

  lines.push(line(plain('')))

  if (state.items.length === 0) {
    lines.push(line(dim('  No matches')))
  } else {
    const maxVisible = 10
    // Keep focused item visible within the window
    let start = 0
    if (state.items.length > maxVisible) {
      start = Math.min(
        Math.max(0, state.focusIndex - Math.floor(maxVisible / 2)),
        state.items.length - maxVisible
      )
    }
    const end = Math.min(start + maxVisible, state.items.length)

    if (start > 0) {
      lines.push(line(dim(`  ↑ ${start} more`)))
    }
    for (let i = start; i < end; i++) {
      const item = state.items[i]!
      const focused = i === state.focusIndex
      const prefix: StyledSpan = focused ? colored('❯ ', 'cyan') : plain('  ')
      const labelSpans = state.query
        ? highlightSpans(item.label, state.query, focused ? { bold: true } : {})
        : [focused ? bold(item.label) : plain(item.label)]
      const detailSpans = item.detail && state.query
        ? highlightSpans(` ${item.detail}`, state.query, { dim: true })
        : [item.detail ? dim(` ${item.detail}`) : plain('')]
      lines.push(line(prefix, ...labelSpans, ...detailSpans))
    }
    if (end < state.items.length) {
      lines.push(line(dim(`  ↓ ${state.items.length - end} more`)))
    }
  }

  lines.push(line(plain('')))
  lines.push(line(dim('↑↓ navigate · type to filter · enter select · esc cancel')))
  return [block(lines, 1)]
}

const CHECKBOX_ON = '☒'
const CHECKBOX_OFF = '☐'
const TICK = '✔'

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
    const allAnswered = state.questions.every((_, i) => isAnswered(state, i))
    if (allAnswered) {
      tabLine.push(plain('  '))
      if (state.onSubmitTab) {
        tabLine.push(inverse(` ${TICK} Submit `))
      } else {
        tabLine.push(plain(` ${TICK} Submit `))
      }
    }

    // Right arrow
    const canGoRight = !state.onSubmitTab && state.currentTab < state.questions.length - 1
    tabLine.push(canGoRight ? plain(' →') : dim(' →'))

    result.push(line(...tabLine))
    result.push(line(plain('')))
  }

  // ── Submit review page ─────────────────────────────────────────
  if (state.onSubmitTab) {
    result.push(line(bold('Review your answers')))
    result.push(line(plain('')))

    for (let i = 0; i < state.questions.length; i++) {
      const qq = state.questions[i]!
      const a = state.answers[i]
      const answerText = a?.customText ?? (a?.selectedOption !== null ? qq.options[a!.selectedOption!]?.label : '—')
      result.push(line(plain(`  ${qq.question}`)))
      result.push(line(colored(`    → ${answerText}`, 'green')))
    }

    result.push(line(plain('')))

    // Submit / Cancel options
    const submitFocused = state.submitFocus === 0
    const cancelFocused = state.submitFocus === 1
    result.push(line(
      submitFocused ? colored('❯ ', 'cyan') : plain('  '),
      submitFocused ? bold('Submit answers') : plain('Submit answers')
    ))
    result.push(line(
      cancelFocused ? colored('❯ ', 'cyan') : plain('  '),
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

  const ui = state.uiStates.get(state.currentTab) ?? { focusIndex: 0, inOtherMode: false, otherText: '' }

  // ── Options ────────────────────────────────────────────────────
  for (let i = 0; i < q.options.length; i++) {
    const opt = q.options[i]!
    const focused = !ui.inOtherMode && i === state.focusIndex
    const prefix: StyledSpan = focused ? colored('❯ ', 'cyan') : plain('  ')
    const label: StyledSpan = focused ? bold(opt.label) : plain(opt.label)
    const desc: StyledSpan = opt.description ? dim(` — ${opt.description}`) : plain('')
    result.push(line(prefix, label, desc))
  }

  // ── Other ──────────────────────────────────────────────────────
  if (ui.inOtherMode && ui.otherText) {
    result.push(line(colored('❯ ', 'cyan'), plain(ui.otherText), inverse(' ')))
  } else if (ui.inOtherMode) {
    result.push(line(colored('❯ ', 'cyan'), inverse(' '), dim(' Type something.')))
  } else {
    const isOtherSelected = state.focusIndex === q.options.length
    const prefix: StyledSpan = isOtherSelected ? colored('❯ ', 'cyan') : plain('  ')
    result.push(line(prefix, dim('Other...')))
  }

  result.push(line(plain('')))

  // ── Footer hint ────────────────────────────────────────────────
  if (isMulti) {
    result.push(line(dim('↑↓ navigate · ←→ switch tab · enter select · esc cancel')))
  } else {
    result.push(line(dim('↑↓ navigate · enter select · esc cancel')))
  }

  return [block(result, 1)]
}
