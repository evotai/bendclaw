/**
 * The prompt editor: what you type, what is being suggested, and the frame
 * around both.
 *
 * This module composes; the pieces live next door. `frame.ts` owns the border
 * and the fixed-width invariant, `prompt-footer.ts` owns the status line, and
 * `width.ts` owns column arithmetic. What stays here is the editor content
 * itself: input rows with the cursor and ghost hint, and the completion menu.
 */

import stringWidth from 'string-width'
import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
import { getTheme } from '../../render/theme/index.js'
import type { CompletionMenu } from '../input/editor.js'
import { nextGraphemeBoundary } from '../input/grapheme.js'
import { CURSOR_MARKER } from '../renderer.js'
import { atLeastHeight, atLeastWidth, heightTier, widthTier } from './breakpoints.js'
import { createFrame } from './frame.js'
import { promptMode, promptModeLabels, promptModeStyle, type PromptModeStyle } from './prompt-mode.js'
import { buildPromptFooterBlocks, type PromptFooterVM } from './prompt-footer.js'
import { line, block, plain, dim, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'
import { finiteSize, truncateToWidth, truncateTailToWidth, wrapTextByWidth } from './width.js'

export interface PromptVMInput extends PromptFooterVM {
  lines: string[]
  cursorLine: number
  cursorCol: number
  active: boolean
  completion: CompletionMenu | null
  ghostHint: string
  rows: number
  placeholder: boolean
  exitHint: boolean
  /** Blink phase of the end-of-line caret; the on-character block never blinks. */
  caretVisible: boolean
}

export interface PromptLayoutOptions {
  attachedAbove?: boolean
  /** Spinner and queue rows already occupying the live region above the prompt. */
  reservedAboveRows?: number
}

const KNOWN_COMMANDS = new Set(
  [...COMMANDS, ...HIDDEN_COMMANDS].flatMap(command => [command.name, ...(command.aliases ?? [])]),
)

/** Completion viewport height: taller terminals get more candidates. */
const COMPLETION_ROWS_COMPACT = 5
const COMPLETION_ROWS_TALL = 12

/**
 * Three-eighths block (U+258D): the end-of-line caret. Thin enough to read as
 * a caret rather than a filled cell, and wide enough to survive fonts that
 * render the one-eighth bar as a hairline or drop it entirely.
 */
const CARET = '▍'

/** Share of the terminal the editor may grow to before it starts scrolling. */
const MAX_INPUT_ROWS_RATIO = 0.3
const MAX_INPUT_ROWS_FLOOR = 5

/**
 * Blank rows kept around a short draft so the composer reads as a place to
 * write rather than a single cramped line.
 *
 * Three is the smallest floor that can centre: an even interior cannot put
 * equal blanks above and below a one-line draft, which is what makes a 2-row
 * composer look top-heavy. Only from the `md` height tier up — below it a fixed
 * composer plus borders and footer would claim over 40% of the screen.
 */
const MIN_INPUT_ROWS_TALL = 3

function completionRows(rows: number): number {
  return atLeastHeight(heightTier(rows), 'lg') ? COMPLETION_ROWS_TALL : COMPLETION_ROWS_COMPACT
}

function minInputRows(rows: number): number {
  return atLeastHeight(heightTier(rows), 'md') ? MIN_INPUT_ROWS_TALL : 1
}

/** `↑ 3 lines` / `↓ 1 line` — hidden rows above or below the viewport. */
function overflowLabel(arrow: '↑' | '↓', count: number): string | undefined {
  if (count <= 0) return undefined
  return `${arrow} ${count} ${count === 1 ? 'line' : 'lines'}`
}

/**
 * The top border label. Mode and scroll overflow compete for one slot, so they
 * share it: `╭─ plan · ↑ 3 lines ──╮`. Mode leads because it says what pressing
 * enter will do, while overflow only says where you are in a draft.
 *
 * The label carries the mode as text, not just as border hue — colour alone
 * fails on monochrome terminals and for colour-blind users.
 */
function topLabel(modes: string[], overflow: string | undefined): string | undefined {
  const mode = modes.join(' · ')
  if (!mode) return overflow
  return overflow ? `${mode} · ${overflow}` : mode
}

export function buildPromptBlocks(input: PromptVMInput, options: PromptLayoutOptions = {}): ViewBlock[] {
  const columns = finiteSize(input.columns, 80)
  const rows = finiteSize(input.rows, 24)
  const mode = promptModeStyle(promptMode(input))
  const modeLabels = promptModeLabels(input)
  const frame = createFrame(columns, { rows, hex: mode.hex })

  const visual = buildInputLines(input, frame.contentWidth, columns, mode)
  const maxInputRows = Math.max(MAX_INPUT_ROWS_FLOOR, Math.floor(rows * MAX_INPUT_ROWS_RATIO))
  const start = Math.max(0, Math.min(visual.cursorIndex - maxInputRows + 1, visual.lines.length - maxInputRows))
  const end = Math.min(visual.lines.length, start + maxInputRows)
  const inputRows = visual.lines.slice(start, end)

  const completionBudget = Math.max(0, rows
    - (options.reservedAboveRows ?? 0)
    - (options.attachedAbove ? 0 : 1)
    - (frame.ruled ? 2 : 0)
    - inputRows.length
    - (frame.framed ? 1 : 0)
    - (input.exitHint ? 1 : 0)
    - (input.backgroundProcessCount > 0 ? 1 : 0)
    - 2) // repository footer + trailing blank; background status is budgeted above
  const completionLines = buildCompletionLines(
    input.completion,
    frame.contentWidth,
    rows,
    completionBudget,
  )
  // The candidate list already gives the composer height, so the blank-row
  // floor only applies when no menu is open. Otherwise the two stack and push
  // the candidates away from what you typed. Rails are what make the blanks
  // read as composer space, so an unframed terminal gets none.
  const filler = completionLines.length > 0 || !frame.framed
    ? 0
    : Math.max(0, minInputRows(rows) - inputRows.length)
  // Split the filler around the draft so a short one sits centred. Odd
  // remainders go below, keeping the text on or above the middle rather than
  // sinking it a row lower than the eye expects.
  const above = Math.floor(filler / 2)
  const blank = () => frame.row(line(plain('')))

  const blocks: ViewBlock[] = []
  // On a very short terminal the border rows are two of very few, and the
  // transcript needs them more than the composer needs an outline. The caret
  // still marks the row as the place you type, and the footer takes the mode
  // back over.
  if (frame.ruled) blocks.push(block([frame.top(topLabel(modeLabels, overflowLabel('↑', start)))], options.attachedAbove ? 0 : 1))
  blocks.push(block([
    ...Array.from({ length: above }, blank),
    ...inputRows.map(frame.row),
    ...Array.from({ length: filler - above }, blank),
  ], frame.ruled ? undefined : (options.attachedAbove ? 0 : 1)))

  if (completionLines.length > 0) {
    // A blank rail row separates what you typed from what is being suggested;
    // without it the selected candidate reads as a continuation of the input.
    // Only inside the frame: unframed terminals are too short to spend a row.
    if (frame.framed) blocks.push(block([frame.row(line(plain('')))]))
    blocks.push(block(completionLines.map(frame.row)))
  }
  if (frame.ruled) blocks.push(block([frame.bottom(overflowLabel('↓', visual.lines.length - end))]))

  if (input.exitHint) blocks.push(block([line(dim(truncateToWidth('  Press Ctrl+C again to exit', columns)))]))
  // The border label carries the mode whenever it is drawn, so the footer only
  // repeats it on terminals too short for a border.
  blocks.push(...buildPromptFooterBlocks(input, { modeShownAbove: frame.ruled }))
  return blocks
}

/** `contentWidth` is the room inside the frame, not the terminal width. */
function buildInputLines(
  input: PromptVMInput,
  contentWidth: number,
  columns: number,
  mode: PromptModeStyle,
): { lines: StyledLine[]; cursorIndex: number } {
  const lines: StyledLine[] = []
  // No prompt prefix any more, so text gets the full content width. When the
  // cursor lands past a full row, the end-of-line caret wraps onto a fresh row
  // rather than stealing a column from every line.
  const width = Math.max(1, contentWidth)
  let cursorIndex = 0

  for (let lineIndex = 0; lineIndex < input.lines.length; lineIndex++) {
    const text = input.lines[lineIndex]!
    const active = input.active && lineIndex === input.cursorLine
    if (active && text === '' && input.lines.length === 1 && input.placeholder) {
      cursorIndex = lines.length
      // The caret occupies 1 column; the rest is hint.
      const full = atLeastWidth(widthTier(columns), 'md') ? mode.hint : mode.shortHint
      const hint = truncateToWidth(` ${full}`, Math.max(0, contentWidth - 1))
      lines.push(line(
        plain(CURSOR_MARKER),
        caret(input.caretVisible),
        ...(hint ? [dim(hint)] : []),
      ))
      continue
    }

    const chunks = wrapTextByWidth(text, width)
    let cursorChunk = -1
    if (active) {
      cursorChunk = chunks.findIndex(chunk => input.cursorCol >= chunk.start && input.cursorCol < chunk.end)
      if (cursorChunk < 0) {
        const last = chunks[chunks.length - 1]!
        if (input.cursorCol === text.length && stringWidth(text.slice(last.start, last.end)) >= width) {
          chunks.push({ start: text.length, end: text.length })
        }
        cursorChunk = chunks.length - 1
      }
    }

    for (let chunkIndex = 0; chunkIndex < chunks.length; chunkIndex++) {
      const chunk = chunks[chunkIndex]!
      const textChunk = text.slice(chunk.start, chunk.end)
      if (!active || chunkIndex !== cursorChunk) {
        const spans = textChunk ? styleInputText(textChunk) : [plain(' ')]
        // An unfocused composer recedes: an overlay owns the screen, and a
        // draft at full brightness competes with the modal for attention.
        lines.push(line(...(input.active ? spans : spans.map(blurred))))
        continue
      }

      cursorIndex = lines.length
      const cursorCol = Math.max(0, input.cursorCol - chunk.start)
      const rawGhost = !input.completion && chunk.end === text.length ? input.ghostHint : ''
      // Style the whole chunk before splitting, otherwise a command straddling
      // the cursor is scored as two fragments and loses its brand hue.
      const styled = styleInputText(textChunk)
      const graphemeEnd = nextGraphemeBoundary(textChunk, cursorCol)
      const onChar = cursorCol < textChunk.length
      const [before, rest] = splitSpansAt(styled, cursorCol)
      const spans: StyledSpan[] = [...before, plain(CURSOR_MARKER)]
      if (onChar) {
        // The cursor sits on a character: flip that cell to a solid block so
        // the glyph stays visible and the line width is unchanged — no extra
        // column, no word split.
        const [onSpans, after] = splitSpansAt(rest, graphemeEnd - cursorCol)
        spans.push(...onSpans.map(cursorBlock), ...after)
      } else {
        // At the end of the line there is nothing to sit on, so draw a thin
        // bar in the caret column instead.
        spans.push(caret(input.caretVisible))
      }
      // The hint is advisory, so it yields to the terminal edge rather than
      // overflowing it: `getGhostHint` can return the full command list, which
      // is far wider than a narrow terminal. The bar costs one column when shown.
      const ghostBudget = contentWidth - stringWidth(textChunk) - (onChar ? 0 : 1)
      const ghost = rawGhost ? truncateToWidth(rawGhost, Math.max(0, ghostBudget)) : ''
      if (ghost) spans.push(dim(ghost))
      lines.push(line(...spans))
    }
  }

  return { lines, cursorIndex }
}

/** `contentWidth` is the room inside the frame, not the terminal width. */
function buildCompletionLines(
  menu: CompletionMenu | null,
  contentWidth: number,
  rows: number,
  lineBudget: number,
): StyledLine[] {
  if (!menu || lineBudget <= 0) return []
  if (menu.items.length === 0) {
    return menu.note
      ? [line(dim(truncateToWidth(`  ${menu.note}`, contentWidth)))]
      : []
  }
  const { brandHex, selectionBgHex, selectionMutedHex } = getTheme()
  const showNote = Boolean(menu.note) && lineBudget >= 2
  const available = Math.max(1, lineBudget - (showNote ? 1 : 0))
  let visible = Math.min(completionRows(rows), menu.items.length, available)
  const showCounter = menu.items.length > visible && available >= 2
  if (showCounter) visible = Math.min(visible, available - 1)
  // Keep the selection near the middle of the viewport so paging down does not
  // pin it to the last row (which reads as "nothing below").
  const ideal = menu.selectedIndex - Math.floor((visible - 1) / 2)
  const start = Math.max(0, Math.min(ideal, menu.items.length - visible))
  const end = start + visible
  const hasDescriptions = menu.items.slice(start, end).some(item => item.description)
  const labelWidth = Math.min(
    Math.max(...menu.items.slice(start, end).map(item => stringWidth(item.label))),
    Math.max(1, hasDescriptions ? Math.floor(contentWidth * 0.45) : contentWidth - 2),
  )
  const lines: StyledLine[] = []

  for (let index = start; index < end; index++) {
    const item = menu.items[index]!
    const selected = index === menu.selectedIndex
    const label = truncateTailToWidth(item.label, labelWidth)
    const padding = ' '.repeat(Math.max(0, labelWidth - stringWidth(label)))
    const bg = selected ? selectionBgHex : undefined
    const prefix = selected ? { text: '❯ ', hex: brandHex, bold: true, bg } : plain('  ')
    const labelSpan = selected ? { text: label, hex: brandHex, bold: true, bg } : plain(label)
    const descriptionWidth = Math.max(0, contentWidth - 2 - labelWidth - 2)
    const description = item.description && descriptionWidth > 0
      ? truncateToWidth(item.description, descriptionWidth)
      : ''
    // On the selection fill the normal dim gray loses contrast, so descriptions
    // switch to the lighter selection-muted tier.
    const descriptionSpan = !description
      ? plain('')
      : selected
        ? { text: `  ${description}`, hex: selectionMutedHex, bg }
        : dim(`  ${description}`)
    lines.push({
      spans: [prefix, labelSpan, { text: padding, ...(bg ? { bg } : {}) }, descriptionSpan],
      ...(bg ? { bg } : {}),
    })
  }

  if (showCounter) {
    lines.push(line(dim(`  ${menu.selectedIndex + 1}/${menu.items.length}`)))
  }
  if (showNote && menu.note) {
    lines.push(line(dim(truncateToWidth(`  ${menu.note}`, contentWidth))))
  }
  return lines
}

function styleInputText(text: string): StyledSpan[] {
  const match = /^(\/[a-z]+)(\s.*)?$/.exec(text)
  if (!match || !KNOWN_COMMANDS.has(match[1]!)) return [plain(text)]
  return [
    { text: match[1]!, hex: getTheme().brandHex, bold: true },
    ...(match[2] ? [plain(match[2])] : []),
  ]
}

/**
 * Push a span into the background for an unfocused composer.
 *
 * Dropping the hue as well as dimming matters: a bold brand-coloured command
 * stays loud under `dim` alone, which is exactly the text that should recede
 * while a modal has the screen.
 */
function blurred(span: StyledSpan): StyledSpan {
  const { hex: _hex, bold: _bold, ...rest } = span
  return { ...rest, dim: true }
}

/**
 * The end-of-line caret: a thin vertical bar in the cursor hue. Used only
 * where there is no character to sit on, so it reads as "type here" without an
 * `❯` prefix. Mid-line, `cursorBlock` highlights the character instead.
 *
 * On the blink's off phase the bar becomes a space, so the column stays claimed
 * and nothing after it shifts.
 */
function caret(visible = true): StyledSpan {
  if (!visible) return plain(' ')
  return { text: CARET, hex: getTheme().cursorHex, bold: true }
}

/**
 * Recolour a span as the cell the cursor sits on: a solid block of the cursor
 * hue with contrasting text. Unlike inserting a bar, this keeps the glyph
 * visible and does not shift the rest of the line by a column.
 */
function cursorBlock(span: StyledSpan): StyledSpan {
  const { cursorHex, cursorFgHex } = getTheme()
  return { ...span, hex: cursorFgHex, fg: undefined, bg: cursorHex, dim: false, bold: true }
}

/**
 * Cut a styled run at `index` code units, splitting the span that straddles it
 * and preserving each span's styling on both sides.
 */
function splitSpansAt(spans: StyledSpan[], index: number): [StyledSpan[], StyledSpan[]] {
  const before: StyledSpan[] = []
  const after: StyledSpan[] = []
  let seen = 0
  for (const span of spans) {
    const length = span.text.length
    if (seen >= index) {
      after.push(span)
    } else if (seen + length <= index) {
      before.push(span)
    } else {
      const at = index - seen
      before.push({ ...span, text: span.text.slice(0, at) })
      after.push({ ...span, text: span.text.slice(at) })
    }
    seen += length
  }
  return [before, after]
}
