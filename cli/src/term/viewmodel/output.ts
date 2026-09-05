import type { OutputLine, ToolCardMembership, ToolCardState } from '../../render/output.js'
import stringWidth from 'string-width'
import { line, block, plain, dim, bold, colored, ansi, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'
import { spansWidth, wrapTextByWidth } from './width.js'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { BOX_DRAWING_RE } from '../../markdown/primitives.js'
import { getTheme } from '../../render/theme/index.js'
import stripAnsi from 'strip-ansi'
import { buildSystemBlock } from './system.js'

export interface OutputContext {
  prevKind?: string
  columns?: number
}

// Transcript panels: a full-width filled slab with a blank padded row above
// and below — pi's Box(paddingY=1, bg). The fill is what separates "input and
// tool activity" from the model's prose, which stays on the bare page. The
// user message carries opencode's border-left (`┃`) in the brand colour; tool
// cards keep the border cell blank, so their content sits on the same column,
// and say their state through the fill instead (pi's pending/success/error).
const PANEL_RAIL = '\u2503'
// Rail (or blank) + gap on the left, one cell of fill on the right.
const PANEL_PAD_LEFT = 2
const PANEL_PAD_RIGHT = 1

/**
 * Lay a row out as part of a panel filled with `bg`. `rail` paints the
 * border-left cell. Without a known width the fill covers only the content
 * (the renderer still gets a well-formed line — it just cannot reach the
 * margin).
 */
function panelRow(
  spans: StyledSpan[],
  columns: number | undefined,
  bg: string,
  rail?: string,
): StyledLine {
  const used = PANEL_PAD_LEFT + spansWidth(spans)
  const fill = ' '.repeat(columns ? Math.max(PANEL_PAD_RIGHT, columns - used) : PANEL_PAD_RIGHT)
  const lead: StyledSpan = rail ? { text: PANEL_RAIL, hex: rail } : plain(' ')
  return { spans: [lead, plain(' '), ...spans, plain(fill)], bg }
}

/** Content width inside a panel. 0 means "unknown, do not wrap". */
function panelInnerWidth(columns: number | undefined): number {
  return columns ? Math.max(1, columns - PANEL_PAD_LEFT - PANEL_PAD_RIGHT) : 0
}

/** Card fill for a tool call in the given lifecycle state. */
function toolCardBg(state: ToolCardState): string {
  const theme = getTheme()
  switch (state) {
    case 'pending': return theme.toolPendingBg
    case 'success': return theme.toolSuccessBg
    case 'error': return theme.toolErrorBg
  }
}

/** Row fill for a diff row inside a card; context rows keep the card fill. */
function diffRowBg(kind: OutputLine['diffRow']): string | undefined {
  const theme = getTheme()
  if (kind === 'add') return theme.diffAddedBg
  if (kind === 'remove') return theme.diffRemovedBg
  return undefined
}

/**
 * Wall-clock header shown above a user message, e.g. `[06:11 PM]`.
 *
 * Formatted from the timestamp captured when the message was built, so a
 * re-render (or the incremental history cache) reproduces the original time
 * instead of drifting to "now".
 */
export function formatClock(timestamp: number): string {
  const at = new Date(timestamp)
  const hours = at.getHours()
  const suffix = hours < 12 ? 'AM' : 'PM'
  const hour12 = hours % 12 === 0 ? 12 : hours % 12
  const hh = String(hour12).padStart(2, '0')
  const mm = String(at.getMinutes()).padStart(2, '0')
  return `[${hh}:${mm} ${suffix}]`
}

// OSC 133 semantic zone markers (the shell-integration protocol). Wrapping each
// committed user/assistant message in a zone lets supporting terminals (iTerm2,
// WezTerm, Kitty, Ghostty, VSCode) jump between messages and select/copy a whole
// message as one block instead of hand-dragging across wrapped lines.
// Unsupported terminals ignore the sequences, and strip-ansi removes them, so
// line-width math is unaffected. Mirrors pi's user/assistant message components:
// first line gets 133;A (zone start), last line gets 133;B + 133;C (zone end).
//
// Boundaries come from per-line flags (zoneStart/zoneEnd) set by the message
// builders, not from inspecting neighbor lines. This keeps marker placement
// purely local so the incremental history cache (which flattens one commit at a
// time) produces byte-identical output to a full rebuild.
const OSC133_ZONE_START = '\x1b]133;A\x07'
const OSC133_ZONE_END = '\x1b]133;B\x07\x1b]133;C\x07'

function prependZoneMarker(b: ViewBlock): void {
  const first = b.lines[0]
  if (first) first.spans.unshift(plain(OSC133_ZONE_START))
}

function appendZoneMarker(b: ViewBlock): void {
  const last = b.lines[b.lines.length - 1]
  if (last) last.spans.push(plain(OSC133_ZONE_END))
}

export function buildOutputBlocks(lines: OutputLine[], context: OutputContext | string = {}): ViewBlock[] {
  const blocks: ViewBlock[] = []
  const initialContext: OutputContext = typeof context === 'string' ? { prevKind: context } : context
  let prevKind: string | undefined = initialContext.prevKind

  for (const ol of lines) {
    // Track which blocks this line produces so zone markers attach to its
    // first/last rendered line without disturbing the rest.
    const blockStart = blocks.length
    let nextPrevKind: string | undefined = ol.kind
    // Lines inside a panel wrap to its inner width so the side padding
    // survives; everything else wraps to the full terminal.
    const wrapColumns = ol.toolCard
      ? (initialContext.columns ? panelInnerWidth(initialContext.columns) : undefined)
      : initialContext.columns
    switch (ol.kind) {
      case 'user': {
        // opencode's UserMessage: border-left in the brand colour, the message
        // on the panel fill, a blank padded row above and below. The panel is
        // what says "you said this" — no glyph, no bold — so a wrapped or
        // multi-line message reads as one slab.
        const cols = initialContext.columns
        const { panelBg, brandHex } = getTheme()
        const availWidth = panelInnerWidth(cols)
        const row = (...spans: StyledSpan[]): StyledLine => panelRow(spans, cols, panelBg, brandHex)
        const userLines: StyledLine[] = [row()]
        if (ol.timestamp !== undefined) {
          userLines.push(row(dim(formatClock(ol.timestamp))))
        }
        // Shift+Enter and pasted input carry hard newlines. Each logical line is
        // wrapped on its own so every rendered row is a complete panel row:
        // letting a raw newline reach the renderer would split the row *after*
        // the padding and leave the continuation unfilled.
        for (const segment of ol.text.split('\n')) {
          if (!segment) {
            userLines.push(row())
            continue
          }
          if (availWidth > 0) {
            for (const c of wrapTextByWidth(segment, availWidth)) {
              userLines.push(row(plain(segment.slice(c.start, c.end))))
            }
          } else {
            userLines.push(row(plain(segment)))
          }
        }
        userLines.push(row())
        blocks.push(block(userLines, 1))
        break
      }

      case 'assistant': {
        // Empty-text assistant lines are block-spacing separators inserted by
        // the stream machine. Continuation spacers keep the next rendered
        // assistant line in the same message, so headings in later streamed
        // chunks don't get another leading dot.
        if (!ol.text) {
          blocks.push(block([line(plain(''))]))
          nextPrevKind = ol.isContinuationSpacer ? 'assistant' : prevKind
          break   // intentionally skip normal prevKind update
        }
        const isBlockStart = prevKind !== 'assistant'
        const dot = isBlockStart ? colored('⏺ ', 'cyan') : plain('  ')
        // Wrap at render-time width (prefix is 2 cols) so committed assistant
        // text reflows on resize instead of being truncated by the renderer.
        const cols = initialContext.columns
        const avail = cols ? Math.max(1, cols - 2) : 0
        // Never reflow box-drawing rows (rendered tables, tree/diagram art):
        // wrapping a border line mid-cell shatters the grid. Those lines are
        // left intact and the renderer clips them if the terminal is narrower
        // — same rule as the markdown wrapper. Matches pi, which never re-wraps
        // structural block art.
        const isBoxArt = BOX_DRAWING_RE.test(stripAnsi(ol.text))
        if (avail > 0 && !isBoxArt && stringWidth(ol.text) > avail) {
          const wrapped = wrapTextWithAnsi(ol.text, avail)
          const asstLines = wrapped.map((w, k) =>
            k === 0 ? line(dot, plain(w)) : line(plain('  '), plain(w)),
          )
          blocks.push(block(asstLines, isBlockStart ? 1 : 0))
        } else {
          blocks.push(block([line(dot, plain(ol.text))], isBlockStart ? 1 : 0))
        }
        break
      }

      case 'thinking': {
        // Reasoning stays visible and readable: `✻` in the accent hue marks the
        // block (assistant prose uses `⏺`), and the body is muted grey rather
        // than dim italic — dim italic on a dark terminal was barely legible,
        // especially for CJK. Continuations indent under the marker.
        const theme = getTheme()
        const isBlockStart = prevKind !== 'thinking'
        const prefix = isBlockStart
          ? ansi(theme.thinkHeader.paint('✻ '))
          : plain('  ')
        const cols = initialContext.columns
        const avail = cols ? Math.max(1, cols - 2) : 0
        const wrapped = avail > 0 ? wrapTextWithAnsi(ol.text, avail) : [ol.text]
        const thinkingLines = wrapped.map((text, index) => {
          const body = ol.thinkingStyle ? ansi(theme.thinkText.paint(text)) : dim(text)
          return line(index === 0 ? prefix : plain('  '), body)
        })
        blocks.push(block(thinkingLines, isBlockStart ? 1 : 0))
        break
      }

      case 'tool':
        if (ol.diffRow) {
          // Diff rows are fully styled by the diff renderer; bypass the
          // headline/status heuristics so a `  5 + code` row is not re-dimmed.
          blocks.push(block(wrapToolLines(ol.text, wrapColumns).map(part => line(ansi(part)))))
          break
        }
        blocks.push(ol.toolCodePreview
          ? buildToolCodePreviewBlock(ol.text, wrapColumns)
          : buildToolBlock(ol.text, wrapColumns))
        break

      case 'tool_result':
        blocks.push(block([line(colored(ol.text, 'gray'))]))
        break

      case 'verbose':
        blocks.push(buildVerboseBlock(ol.text, initialContext.columns))
        break

      case 'error': {
        const cols = wrapColumns
        // Preserve the 2-space indent used by LLM-error body lines so wrapped
        // continuations align under the first line.
        const indentMatch = ol.text.match(/^(\s*)/)
        const indent = indentMatch ? indentMatch[1]! : ''
        const avail = cols ? Math.max(1, cols - indent.length) : 0
        const body = ol.text.slice(indent.length)
        if (avail > 0 && stringWidth(body) > avail) {
          const chunks = wrapTextByWidth(body, avail)
          const errLines = chunks.map(c => line(colored(`${indent}${body.slice(c.start, c.end)}`, 'red')))
          blocks.push(block(errLines))
        } else {
          blocks.push(block([line(colored(ol.text, 'red'))]))
        }
        break
      }

      case 'cancelled': {
        const cols = initialContext.columns
        const cancelledLines = cols
          ? wrapTextWithAnsi(ol.text, Math.max(1, cols))
          : ol.text.split(/\r\n|\r|\n/)
        blocks.push(block(cancelledLines.map(l => line(colored(l, 'yellow')))))
        break
      }

      case 'system':
        blocks.push(buildSystemBlock(ol.text, {
          columns: initialContext.columns, prevKind, preStyled: ol.preStyled,
        }))
        break

      default:
        break
    }
    prevKind = nextPrevKind

    // A tool card lays every row it produced on its lifecycle fill and adds
    // the padded rows at its edges. Done per line from the line's own stamp,
    // so the incremental history cache stays byte-identical.
    if (ol.toolCard && blocks.length > blockStart) {
      paintToolCard(blocks, blockStart, ol.toolCard, initialContext.columns, diffRowBg(ol.diffRow))
    }

    // Attach OSC 133 zone markers from this line's own flags. Purely local, so
    // it is invariant to how the history is sliced across cache appends.
    if (blocks.length > blockStart) {
      if (ol.zoneStart) prependZoneMarker(blocks[blockStart]!)
      if (ol.zoneEnd) appendZoneMarker(blocks[blocks.length - 1]!)
    }
  }

  return blocks
}

/**
 * Lay the blocks a tool-card line produced onto the card's lifecycle fill,
 * pi's ToolExecutionComponent: a Spacer above, then a Box whose background is
 * pending / success / error. The card's first line keeps its top margin and
 * gains the blank padded row that opens the slab; the last line gains the one
 * that closes it. `rowBg` swaps the fill for this line's rows only (diff
 * add/remove rows); the padding rows always use the card fill.
 */
function paintToolCard(
  blocks: ViewBlock[],
  from: number,
  card: ToolCardMembership,
  columns: number | undefined,
  rowBg?: string,
): void {
  const bg = toolCardBg(card.state)
  for (let index = from; index < blocks.length; index++) {
    const b = blocks[index]!
    b.lines = b.lines.map(l => panelRow(l.spans, columns, rowBg ?? bg))
    b.marginTop = index === from && card.first ? 1 : 0
  }
  if (card.first) blocks[from]!.lines.unshift(panelRow([], columns, bg))
  if (card.last) blocks[blocks.length - 1]!.lines.push(panelRow([], columns, bg))
}

function buildToolCodePreviewBlock(text: string, columns?: number): ViewBlock {
  return block(wrapToolLines(text, columns).map(part => line(plain(part))))
}

function buildToolBlock(text: string, columns?: number): ViewBlock {
  // Tool call line: `<glyph> <name>  <arg>` (no status mark — status lives on
  // the subordinate line below). Paint glyph cyan, name bold, arg dim. When the
  // line exceeds the terminal width, wrap the arg onto continuation lines so the
  // full command is always visible (the tail is never truncated).
  const cardMatch = text.match(/^([⌘◫⌕⊕✎·✦◇]) (.+)$/u)
  if (cardMatch) {
    const glyph = cardMatch[1]!
    const rest = cardMatch[2]!.trimEnd()
    const sep = rest.indexOf('  ')
    const name = sep < 0 ? rest : rest.slice(0, sep)
    const arg = sep < 0 ? '' : rest.slice(sep + 2)
    if (!arg) {
      return block([line(colored(glyph, 'cyan', { bold: true }), bold(` ${name}`))], 1)
    }
    // Prefix is `<glyph> <name>  ` — continuation lines indent to align under arg.
    const prefixWidth = stringWidth(`${glyph} ${name}  `)
    const avail = columns ? Math.max(1, columns - prefixWidth) : 0
    if (avail > 0 && stringWidth(arg) > avail) {
      const chunks = wrapTextByWidth(arg, avail)
      const pad = ' '.repeat(prefixWidth)
      const lines: StyledLine[] = chunks.map((c, k) =>
        k === 0
          ? line(colored(glyph, 'cyan', { bold: true }), bold(` ${name}`), dim(`  ${arg.slice(c.start, c.end)}`))
          : line(dim(`${pad}${arg.slice(c.start, c.end)}`)),
      )
      return block(lines, 1)
    }
    return block([line(colored(glyph, 'cyan', { bold: true }), bold(` ${name}`), dim(`  ${arg}`))], 1)
  }

  // Stable lifecycle row under a tool headline. Queued/running are cyan,
  // success is green, failure red, and retry yellow; metadata stays dim.
  const statusMatch = text.match(/^ {2}([○✓✗↻●])(.*)$/u)
  if (statusMatch) {
    const mark = statusMatch[1]!
    const tail = statusMatch[2] ?? ''
    const color = mark === '✗'
      ? 'red'
      : mark === '↻'
        ? 'yellow'
        : mark === '✓'
          ? 'green'
          : 'cyan'
    const spans = [colored(`  ${mark}`, color, { bold: true })]
    if (tail) spans.push(dim(tail))
    return block([line(...spans)])
  }

  if (text.startsWith('  ')) {
    const trimmed = text.trimStart()
    if (/^[{}\[\],]/.test(trimmed) || /^"[^"\\]*(?:\\.[^"\\]*)*"\s*:/.test(trimmed)) {
      return block(wrapToolLines(text, columns).map(l => line(plain(l))))
    }
    return block(wrapToolLines(text, columns).map(l => line(dim(l))))
  }
  return block(wrapToolLines(text, columns).map(l => line(plain(l))))
}

/**
 * Split a tool-output blob into physical lines and soft-wrap each to the
 * terminal width via the shared ANSI-aware primitive. This is what keeps
 * multi-line diffs and JSON output from being hard-truncated by the renderer
 * (which runs with auto-wrap off).
 */
function wrapToolLines(text: string, columns?: number): string[] {
  const width = columns ? Math.max(1, columns) : 0
  const out: string[] = []
  for (const physical of text.split(/\r\n|\r|\n/)) {
    if (width <= 0 || stringWidth(physical) <= width) {
      out.push(physical)
      continue
    }
    for (const wrapped of wrapTextWithAnsi(physical, width)) out.push(wrapped)
  }
  return out
}

function buildVerboseBlock(text: string, columns?: number): ViewBlock {
  const width = columns ? Math.max(1, columns) : 0
  if (width > 0 && stringWidth(stripAnsi(text)) > width) {
    return block(wrapTextWithAnsi(text, width).map(part => line(dim(part))), 1)
  }

  const naturalMatch = text.match(/^([●✓✗↻])\s+(LLM|COMPACT|SPILL)\s*(.*)$/)
  if (naturalMatch) {
    const status = naturalMatch[1]!
    const badge = naturalMatch[2]!
    const rest = naturalMatch[3] ?? ''
    const color = verboseStatusColor()
    const spans = [colored(status, color, { bold: true }), colored(` ${badge}`, color, { bold: true })]
    if (rest) spans.push(dim(` ${rest}`))
    return block([line(...spans)], 1)
  }

  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const statusMatch = rest.match(/^([●✓✗↻])\s*(.*)$/)
    const color = verboseStatusColor()
    const spans = [colored(`[${badge}]`, color, { bold: true })]
    if (statusMatch) {
      spans.push(colored(` ${statusMatch[1]}`, color, { bold: true }))
      const tail = statusMatch[2] ?? ''
      if (tail) spans.push(dim(` ${tail}`))
    } else if (rest) {
      spans.push(dim(` ${rest}`))
    }
    return block([line(...spans)], 1)
  }
  return block((width > 0 ? wrapTextWithAnsi(text, width) : [text]).map(part => line(dim(part))))
}

function verboseStatusColor(): 'cyan' {
  return 'cyan'
}
