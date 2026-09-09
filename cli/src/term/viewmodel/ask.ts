import { wrapTextWithAnsi } from '../../render/wrap.js'
import { blocksToLines, styledLineToAnsi } from './types.js'
import type { AskState } from '../ask.js'
import { CURSOR_MARKER } from '../render-frame.js'
import { line, block, plain, dim, bold, colored, inverse, type ViewBlock, type StyledSpan, type StyledLine } from './types.js'

const CHECKBOX_ON = '☒'
const CHECKBOX_OFF = '☐'
const TICK = '✓'
const POINTER = '❯'
const BULLET = '•'
const ARROW_RIGHT = '→'

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

export function buildAskRegionLines(state: AskState, columns: number): string[] {
  const width = Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 80
  const border = styledLineToAnsi(line(dim('─'.repeat(width))))
  const body = blocksToLines(buildAskBlocks(state, width))
    .flatMap(rendered => wrapTextWithAnsi(rendered, width))
  return ['', border, ...body, border]
}

/** Ask-user presentation, independent of selectors, native calls and layout ownership. */
export function buildAskBlocks(state: AskState, _columns: number): ViewBlock[] {
  const result: StyledLine[] = []
  const isMulti = state.questions.length > 1
  if (isMulti) {
    const tabLine: StyledSpan[] = []
    const canGoLeft = state.currentTab > 0 || state.onSubmitTab
    tabLine.push(canGoLeft ? plain('← ') : dim('← '))
    for (let i = 0; i < state.questions.length; i++) {
      if (i > 0) tabLine.push(plain('  '))
      const qq = state.questions[i]!
      const active = !state.onSubmitTab && i === state.currentTab
      const checkbox = isAnswered(state, i) ? CHECKBOX_ON : CHECKBOX_OFF
      tabLine.push(active ? inverse(` ${checkbox} ${qq.header} `) : plain(` ${checkbox} ${qq.header} `))
    }
    tabLine.push(plain('  '))
    tabLine.push(state.onSubmitTab ? inverse(` ${TICK} Submit `) : plain(` ${TICK} Submit `))
    tabLine.push(!state.onSubmitTab ? plain(' →') : dim(' →'))
    result.push(line(...tabLine), line(plain('')))
  }

  if (state.onSubmitTab) {
    const allAnswered = state.questions.every((_, i) => isAnswered(state, i))
    result.push(line(bold('Review your answers')), line(plain('')))
    if (!allAnswered) result.push(line(colored('⚠ You have not answered all questions', 'yellow')), line(plain('')))
    for (let i = 0; i < state.questions.length; i++) {
      const qq = state.questions[i]!
      const answerText = selectedAnswerText(state, i)
      if (!answerText) continue
      result.push(line(plain(`  ${BULLET} ${qq.question}`)), line(colored(`    ${ARROW_RIGHT} ${answerText}`, 'green')))
    }
    result.push(line(plain('')), line(dim('Ready to submit your answers?')), line(plain('')))
    const submitFocused = state.submitFocus === 0
    const cancelFocused = state.submitFocus === 1
    result.push(line(
      submitFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      submitFocused ? bold('Submit answers') : plain('Submit answers'),
    ))
    result.push(line(
      cancelFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      cancelFocused ? bold('Cancel') : plain('Cancel'),
    ))
    result.push(line(plain('')), line(dim('↑↓ navigate · enter select · ← back · esc cancel')))
    return [block(result, 1)]
  }

  const q = state.questions[state.currentTab]!
  result.push(line(bold(q.question)), line(plain('')))
  const ui = state.uiStates.get(state.currentTab) ?? { focusIndex: 0, inOtherMode: false, otherText: '', otherCursor: 0 }
  const answer = state.answers[state.currentTab]
  const otherSelected = answer !== undefined && answer.customText !== null
  const selectedIndex = !otherSelected ? answer?.selectedOption : null
  const maxIndexWidth = (q.options.length + 1).toString().length
  const optionIndex = (index: number) => `${index}.`.padEnd(maxIndexWidth + 2)
  const appendTick = (spans: StyledSpan[]): StyledSpan[] => [...spans, colored(TICK, 'green')]

  for (let i = 0; i < q.options.length; i++) {
    const opt = q.options[i]!
    const focused = !ui.inOtherMode && i === state.focusIndex
    const selected = selectedIndex === i
    const spans: StyledSpan[] = [
      focused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      dim(optionIndex(i + 1)),
      selected ? colored(opt.label, 'green') : focused ? colored(opt.label, 'cyan') : plain(opt.label),
    ]
    if (opt.description) spans.push(dim(` — ${opt.description}`))
    result.push(line(...(selected ? appendTick(spans) : spans)))
  }

  const otherFocused = ui.inOtherMode
  const otherText = otherFocused ? ui.otherText : otherSelected ? selectedAnswerText(state, state.currentTab) ?? '' : ui.otherText
  const otherSpans: StyledSpan[] = [
    otherFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
    dim(optionIndex(q.options.length + 1)),
  ]
  if (otherFocused) {
    if (otherText) {
      const cursor = ui.otherCursor ?? otherText.length
      const before = otherText.slice(0, cursor)
      const after = otherText.slice(cursor)
      if (before) otherSpans.push(plain(before))
      otherSpans.push(plain(CURSOR_MARKER))
      if (after) otherSpans.push(plain(after))
    } else {
      otherSpans.push(plain(CURSOR_MARKER), dim('Type something.'))
    }
  } else {
    otherSpans.push(otherSelected ? colored(otherText || 'Type something.', 'green') : dim(otherText || 'Type something.'))
  }
  if (otherSelected) otherSpans.push(plain(' '))
  result.push(line(...(otherSelected ? appendTick(otherSpans) : otherSpans)), line(plain('')))
  result.push(line(dim(isMulti
    ? '↑↓ navigate · ←→ switch tab · enter select · esc cancel'
    : '↑↓ navigate · enter select · esc cancel')))
  return [block(result, 1)]
}
