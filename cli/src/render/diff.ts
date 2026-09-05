/**
 * Diff rendering — foreground-colored structured diff with line numbers and
 * word-level highlighting. Aligned with pi's TUI diff component: removed lines
 * red, added lines green, context dim, with inverse on changed tokens for
 * single-line edits. Long lines wrap via the shared ANSI-aware primitive so
 * nothing is truncated (the renderer runs with auto-wrap off).
 */

import chalk from 'chalk'
import { structuredPatch, diffWordsWithSpace } from 'diff'

export interface DiffResult {
  text: string
  linesAdded: number
  linesRemoved: number
}

// Foreground styles — no full-width background bars, so wrapped continuation
// lines stay clean and match pi's look.
const style = {
  added: chalk.green,
  removed: chalk.red,
  context: chalk.dim,
  ellipsis: chalk.dim,
  inverse: (s: string) => `\x1b[7m${s}\x1b[27m`,
}

const WORD_DIFF_THRESHOLD = 0.4

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

interface DiffLine {
  type: 'add' | 'remove' | 'context'
  code: string
  lineNum: number
  paired?: DiffLine
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Compute a colored structured diff between old and new text.
 */
export function formatDiff(oldText: string, newText: string, filename = ''): DiffResult {
  const patch = structuredPatch(filename, filename, oldText, newText, '', '', { context: 3 })
  let linesAdded = 0
  let linesRemoved = 0
  const output: string[] = []

  for (let hi = 0; hi < patch.hunks.length; hi++) {
    if (hi > 0) output.push(style.ellipsis('  …'))
    const hunk = patch.hunks[hi]!
    const lines = buildDiffLines(hunk.lines, hunk.oldStart)
    const numWidth = gutterWidth(lines)
    for (const line of lines) {
      if (line.type === 'add') linesAdded++
      if (line.type === 'remove') linesRemoved++
      output.push(renderLine(line, numWidth))
    }
  }

  return { text: output.join('\n'), linesAdded, linesRemoved }
}

/**
 * Colorize a pre-computed unified diff string (from the Rust engine).
 */
export function colorizeUnifiedDiff(diff: string): string {
  return colorizeUnifiedDiffRows(diff).map(row => row.text).join('\n')
}

export type DiffRowKind = 'add' | 'remove' | 'context' | 'ellipsis'

export interface DiffRow {
  /** Foreground-styled row text (gutter + sigil + code). */
  text: string
  kind: DiffRowKind
}

/**
 * Colorize a unified diff one row at a time. Callers that lay rows out on a
 * filled block use `kind` to give added/removed rows their own fill, the way
 * opencode tints diff rows inside a tool block.
 */
export function colorizeUnifiedDiffRows(diff: string): DiffRow[] {
  const raw = diff.split('\n')
  const body = raw.filter(l => !l.startsWith('---') && !l.startsWith('+++'))
  const output: DiffRow[] = []

  // Group lines by hunk
  const hunks: { header: string; lines: string[] }[] = []
  let cur: { header: string; lines: string[] } | null = null
  for (const line of body) {
    if (line.startsWith('@@')) {
      cur = { header: line, lines: [] }
      hunks.push(cur)
    } else if (cur) {
      cur.lines.push(line)
    } else {
      // Lines before any @@ header — treat as a single hunk at line 1
      if (!hunks.length || hunks[0]!.header !== '') {
        cur = { header: '', lines: [] }
        hunks.unshift(cur)
      }
      cur = hunks[0]!
      cur.lines.push(line)
    }
  }

  for (let hi = 0; hi < hunks.length; hi++) {
    if (hi > 0) output.push({ text: style.ellipsis('  …'), kind: 'ellipsis' })
    const hunk = hunks[hi]!
    const startLine = parseHunkStart(hunk.header)
    const lines = buildDiffLines(hunk.lines, startLine)
    const numW = gutterWidth(lines)
    for (const line of lines) {
      output.push({ text: renderLine(line, numW), kind: line.type })
    }
  }

  return output
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function parseHunkStart(header: string): number {
  const m = header.match(/@@ -(\d+)/)
  return m ? parseInt(m[1]!, 10) : 1
}

function gutterWidth(lines: DiffLine[]): number {
  const maxNum = Math.max(...lines.map(l => l.lineNum), 0)
  return Math.max(String(maxNum).length, 1)
}

/** Parse raw diff lines → structured DiffLines with line numbers + pairing. */
function buildDiffLines(rawLines: string[], startLine: number): DiffLine[] {
  const parsed = rawLines.map(raw => {
    if (raw.startsWith('+')) return { type: 'add' as const, code: raw.slice(1) }
    if (raw.startsWith('-')) return { type: 'remove' as const, code: raw.slice(1) }
    return { type: 'context' as const, code: raw.startsWith(' ') ? raw.slice(1) : raw }
  })
  const paired = pairChanges(parsed)
  return assignLineNumbers(paired, startLine)
}

/** Pair adjacent remove→add sequences for word-level diff. */
function pairChanges(
  lines: { type: 'add' | 'remove' | 'context'; code: string }[],
): { type: 'add' | 'remove' | 'context'; code: string; pairedCode?: string }[] {
  const out: { type: 'add' | 'remove' | 'context'; code: string; pairedCode?: string }[] = []
  let i = 0
  while (i < lines.length) {
    if (lines[i]!.type !== 'remove') { out.push(lines[i]!); i++; continue }

    const removes: typeof lines = []
    while (i < lines.length && lines[i]!.type === 'remove') { removes.push(lines[i]!); i++ }
    const adds: typeof lines = []
    while (i < lines.length && lines[i]!.type === 'add') { adds.push(lines[i]!); i++ }

    const n = Math.min(removes.length, adds.length)
    for (let k = 0; k < n; k++) out.push({ ...removes[k]!, pairedCode: adds[k]!.code })
    for (let k = n; k < removes.length; k++) out.push(removes[k]!)
    for (let k = 0; k < n; k++) out.push({ ...adds[k]!, pairedCode: removes[k]!.code })
    for (let k = n; k < adds.length; k++) out.push(adds[k]!)
  }
  return out
}

/** Assign line numbers and link paired lines. */
function assignLineNumbers(
  lines: { type: 'add' | 'remove' | 'context'; code: string; pairedCode?: string }[],
  startLine: number,
): DiffLine[] {
  const dls: (DiffLine & { pairedCode?: string })[] = lines.map(l => ({
    type: l.type, code: l.code, lineNum: 0, pairedCode: l.pairedCode,
  }))

  let oldNum = startLine
  let newNum = startLine
  for (const dl of dls) {
    if (dl.type === 'context') { dl.lineNum = oldNum; oldNum++; newNum++ }
    else if (dl.type === 'remove') { dl.lineNum = oldNum; oldNum++ }
    else { dl.lineNum = newNum; newNum++ }
  }

  for (const dl of dls) {
    if (dl.pairedCode !== undefined) {
      dl.paired = { type: dl.type === 'remove' ? 'add' : 'remove', code: dl.pairedCode, lineNum: 0 }
    }
  }
  return dls
}

/**
 * Render one diff line: `<num> <sigil> <code>` in the line's foreground color.
 * Single-line edits get inverse highlighting on changed tokens. No background
 * bars and no padding, so the shared wrapper can reflow long lines cleanly.
 */
function renderLine(line: DiffLine, numWidth: number): string {
  const num = String(line.lineNum).padStart(numWidth)
  const sigil = line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' '
  const gutterStr = `${num} ${sigil}`

  if (line.type === 'context') {
    return style.context(gutterStr + line.code)
  }

  const paint = line.type === 'add' ? style.added : style.removed

  // Word-level diff for single-line edits: inverse the changed tokens.
  if (line.paired) {
    const body = wordDiff(line)
    if (body !== null) return paint(gutterStr) + body
  }

  return paint(gutterStr + line.code)
}

/** Word-level diff. Returns the painted line body, or null if too different. */
function wordDiff(line: DiffLine): string | null {
  if (!line.paired) return null
  const oldText = line.type === 'remove' ? line.code : line.paired.code
  const newText = line.type === 'remove' ? line.paired.code : line.code
  const parts = diffWordsWithSpace(oldText, newText)

  const totalLen = oldText.length + newText.length
  if (totalLen === 0) return null
  const changedLen = parts.filter(p => p.added || p.removed).reduce((s, p) => s + p.value.length, 0)
  if (changedLen / totalLen > WORD_DIFF_THRESHOLD) return null

  const paint = line.type === 'add' ? style.added : style.removed
  const segs: string[] = []
  for (const p of parts) {
    if (line.type === 'add') {
      if (p.removed) continue
      segs.push(p.added ? paint(style.inverse(p.value)) : paint(p.value))
    } else {
      if (p.added) continue
      segs.push(p.removed ? paint(style.inverse(p.value)) : paint(p.value))
    }
  }
  return segs.join('')
}
