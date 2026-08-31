import type { OutputLine } from '../../render/output.js'
import stringWidth from 'string-width'
import { line, block, plain, dim, bold, colored, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'
import { wrapTextByWidth } from './width.js'
import { wrapTextWithAnsi } from '../../render/wrap.js'
import { BOX_DRAWING_RE } from '../../markdown/primitives.js'
import { getTheme } from '../../render/theme.js'
import stripAnsi from 'strip-ansi'

export interface OutputContext {
  prevKind?: string
  columns?: number
}

// A committed user message is marked by a left rail rather than a prompt glyph:
// it spans every rendered row, so a wrapped or multi-line message reads as one
// block instead of a first line plus loose continuations.
//
// U+258D (left five-eighths block) matches freebuff's rail. It paints most of
// the cell, so the rail still reads as continuous down the message, while a
// fully background-filled cell was wide enough to look like a selection
// highlight rather than a margin marker.
const USER_BAR = '\u258d'

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
    switch (ol.kind) {
      case 'user': {
        const cols = initialContext.columns
        // The bar occupies one cell plus a separating space, matching the
        // 2-column prefix every other kind uses so wrapped text stays aligned.
        const availWidth = cols ? Math.max(1, cols - 2) : 0
        const barHex = getTheme().brandHex
        const bar = (): StyledSpan => ({ text: USER_BAR, hex: barHex })
        const userLines: StyledLine[] = []
        if (ol.timestamp !== undefined) {
          userLines.push(line(bar(), plain(' '), dim(formatClock(ol.timestamp))))
        }
        // Shift+Enter and pasted input carry hard newlines. Each logical line is
        // wrapped on its own so every rendered row keeps the bar: letting a raw
        // newline reach the renderer splits the row *after* the prefix, which is
        // what dropped the bar on continuation lines.
        for (const segment of ol.text.split('\n')) {
          if (!segment) {
            userLines.push(line(bar()))
            continue
          }
          if (availWidth > 0) {
            for (const c of wrapTextByWidth(segment, availWidth)) {
              userLines.push(line(bar(), plain(' '), bold(segment.slice(c.start, c.end))))
            }
          } else {
            userLines.push(line(bar(), plain(' '), bold(segment)))
          }
        }
        if (userLines.length === 0) userLines.push(line(bar()))
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
        // Keep reasoning rows within the terminal width just like pi's Text /
        // Markdown components. A long unwrapped thinking line violates the
        // renderer's one-logical-line-per-terminal-row invariant.
        const isBlockStart = prevKind !== 'thinking'
        const prefix = isBlockStart ? colored('✻ ', 'magenta', { dim: true }) : plain('  ')
        const cols = initialContext.columns
        const avail = cols ? Math.max(1, cols - 2) : 0
        const wrapped = avail > 0 ? wrapTextWithAnsi(ol.text, avail) : [ol.text]
        const thinkingLines = wrapped.map((text, index) => {
          const body = ol.thinkingStyle
            ? { text, italic: true, dim: true }
            : dim(text)
          return line(index === 0 ? prefix : plain('  '), body)
        })
        blocks.push(block(thinkingLines, isBlockStart ? 1 : 0))
        break
      }

      case 'tool':
        blocks.push(ol.toolCodePreview
          ? buildToolCodePreviewBlock(ol.text, initialContext.columns)
          : buildToolBlock(ol.text, initialContext.columns))
        break

      case 'tool_result':
        blocks.push(block([line(colored(ol.text, 'gray'))]))
        break

      case 'verbose':
        blocks.push(buildVerboseBlock(ol.text, initialContext.columns))
        break

      case 'error': {
        const cols = initialContext.columns
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

      case 'system': {
        const cols = initialContext.columns
        const systemLines = cols
          ? wrapTextWithAnsi(ol.text, Math.max(1, cols))
          : ol.text.split(/\r\n|\r|\n/)
        blocks.push(block(systemLines.map(l => line(dim(l)))))
        break
      }

      default:
        break
    }
    prevKind = nextPrevKind

    // Attach OSC 133 zone markers from this line's own flags. Purely local, so
    // it is invariant to how the history is sliced across cache appends.
    if (blocks.length > blockStart) {
      if (ol.zoneStart) prependZoneMarker(blocks[blockStart]!)
      if (ol.zoneEnd) appendZoneMarker(blocks[blocks.length - 1]!)
    }
  }

  return blocks
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
