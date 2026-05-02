/**
 * Diff rendering — structured diff with line numbers, background colors,
 * and word-level highlighting (inspired by Claude Code's StructuredDiffFallback).
 */

import chalk, { type ChalkInstance } from 'chalk'
import { structuredPatch, diffWordsWithSpace } from 'diff'
import { getTheme } from './theme.js'

export interface DiffResult {
  text: string
  linesAdded: number
  linesRemoved: number
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

// Colors adapted to dark/light theme:
// - Line bg: very muted tint (solid bar across full terminal width)
// - Word bg: slightly brighter to highlight changed words
// - Context lines: dim, no background
function getStyle() {
  const t = getTheme()
  return {
    addedBg:     chalk.bgRgb(...t.addedBg),
    removedBg:   chalk.bgRgb(...t.removedBg),
    addedWord:   chalk.bgRgb(...t.addedWord),
    removedWord: chalk.bgRgb(...t.removedWord),
    context:     chalk.dim,
    gutter:      chalk.gray,
    ellipsis:    chalk.dim,
  }
}

const DEFAULT_WIDTH = 80
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
  const width = process.stdout.columns || DEFAULT_WIDTH
  const style = getStyle()
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
      output.push(renderLine(line, numWidth, width, style))
    }
  }

  return { text: output.join('\n'), linesAdded, linesRemoved }
}

/**
 * Colorize a pre-computed unified diff string (from Rust engine).
 */
export function colorizeUnifiedDiff(diff: string): string {
  const raw = diff.split('\n')
  const body = raw.filter(l => !l.startsWith('---') && !l.startsWith('+++'))
  const width = process.stdout.columns || DEFAULT_WIDTH
  const style = getStyle()
  const output: string[] = []

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
    if (hi > 0) output.push(style.ellipsis('  …'))
    const hunk = hunks[hi]!
    const startLine = parseHunkStart(hunk.header)
    const lines = buildDiffLines(hunk.lines, startLine)
    const numW = gutterWidth(lines)
    for (const line of lines) {
      output.push(renderLine(line, numW, width, style))
    }
  }

  return output.join('\n')
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
 * Render one line as a solid colored bar spanning the full terminal width.
 * Gutter (line number + sigil) and content share the same background color
 * for visual continuity — matching Claude Code's diff rendering.
 */
function renderLine(line: DiffLine, numWidth: number, termWidth: number, style: ReturnType<typeof getStyle>): string {
  const num = String(line.lineNum).padStart(numWidth)
  const sigil = line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' '
  const gutterStr = `${num} ${sigil}`
  const gutterLen = numWidth + 2 // num + space + sigil

  if (line.type === 'context') {
    // Context: dim gutter + dim code, no background, no padding
    return style.context(gutterStr + line.code)
  }

  const bg = line.type === 'add' ? style.addedBg : style.removedBg
  const contentLen = gutterLen + line.code.length
  const padding = Math.max(0, termWidth - contentLen)

  // Try word-level diff — changed words get brighter bg, rest gets line bg
  if (line.paired) {
    const wbg = line.type === 'add' ? style.addedWord : style.removedWord
    const wd = wordDiff(line, wbg, bg)
    if (wd !== null) {
      return bg(style.gutter(gutterStr)) + wd + bg(' '.repeat(padding))
    }
  }

  // Whole line: single bg() call wrapping gutter + code + padding
  return bg(style.gutter(gutterStr) + line.code + ' '.repeat(padding))
}

/** Word-level diff. Returns null if change ratio too high. */
function wordDiff(line: DiffLine, wordBg: ChalkInstance, lineBg: ChalkInstance): string | null {
  if (!line.paired) return null
  const oldText = line.type === 'remove' ? line.code : line.paired.code
  const newText = line.type === 'remove' ? line.paired.code : line.code
  const parts = diffWordsWithSpace(oldText, newText)

  const totalLen = oldText.length + newText.length
  if (totalLen === 0) return null
  const changedLen = parts.filter(p => p.added || p.removed).reduce((s, p) => s + p.value.length, 0)
  if (changedLen / totalLen > WORD_DIFF_THRESHOLD) return null

  const segs: string[] = []
  for (const p of parts) {
    if (line.type === 'add') {
      if (p.removed) continue
      segs.push(p.added ? wordBg(p.value) : lineBg(p.value))
    } else {
      if (p.added) continue
      segs.push(p.removed ? wordBg(p.value) : lineBg(p.value))
    }
  }
  return segs.join('')
}


