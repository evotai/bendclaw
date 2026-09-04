/**
 * Shared formatting utilities.
 */

import stringWidth from 'string-width'

export function errorText(err: unknown): string {
  if (err instanceof Error) return err.message
  const message = (err as { message?: unknown } | null)?.message
  return typeof message === 'string' ? message : String(err)
}

function repeatCount(n: number): number {
  return Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0
}

/** Standards-backed grapheme segmentation for safe Unicode truncation. */
const graphemeSegmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

export function padRight(s: string, n: number): string {
  n = repeatCount(n)

  // Session rows are overwhelmingly ASCII. Keep that path allocation-free,
  // and delegate every other script to string-width rather than maintaining a
  // partial Unicode table that mismeasures Indic and decomposed Hangul text.
  if (/^[\x20-\x7e]*$/.test(s)) {
    if (s.length <= n) return s + ' '.repeat(n - s.length)
    return s.slice(0, Math.max(0, n - 1)) + '…'
  }

  const width = stringWidth(s)
  if (width <= n) return s + ' '.repeat(n - width)

  const budget = n - 1
  let truncated = ''
  let truncatedWidth = 0
  for (const { segment } of graphemeSegmenter.segment(s)) {
    const segmentWidth = stringWidth(segment)
    if (truncatedWidth + segmentWidth > budget) break
    truncated += segment
    truncatedWidth += segmentWidth
  }
  return truncated + '…'
}

export function relativeTime(iso: string): string {
  try {
    const date = new Date(iso)
    if (isNaN(date.getTime())) return iso
    const ms = Date.now() - date.getTime()
    const mins = Math.floor(ms / 60000)
    if (mins < 1) return 'just now'
    if (mins < 60) return `${mins}m ago`
    const hours = Math.floor(mins / 60)
    if (hours < 24) return `${hours}h ago`
    const days = Math.floor(hours / 24)
    return `${days}d ago`
  } catch {
    return iso
  }
}

export function humanTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return `${n}`
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

/**
 * Wall-clock runtime as `2s` / `1m 34s` / `1h 30m`.
 *
 * Distinct from `formatDuration`, which is for sub-second tool latency. This is
 * for long-lived work, and one definition is shared by the background panel and
 * the task tool cards so a task reads the same in both places.
 */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  if (total < 60) return `${total}s`
  const minutes = Math.floor(total / 60)
  const seconds = total % 60
  if (minutes < 60) return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`
  const hours = Math.floor(minutes / 60)
  const restMinutes = minutes % 60
  return restMinutes === 0 ? `${hours}h` : `${hours}h ${restMinutes}m`
}

export function renderBar(value: number, max: number, width: number): string {
  width = repeatCount(width)
  if (width === 0) return ''
  if (max <= 0 || !Number.isFinite(max) || !Number.isFinite(value)) return '░'.repeat(width)
  const filled = repeatCount(Math.round((value / max) * width))
  return '█'.repeat(Math.min(filled, width)) + '░'.repeat(Math.max(0, width - filled))
}

/**
 * Position bar character mapping for compaction methods.
 *
 *   · — Unchanged / kept
 *   O — Outline (tree-sitter structural extraction)
 *   H — HeadTail (head + tail truncation)
 *   S — Summarized (turn summarized)
 *   D — Dropped (messages evicted)
 *   L — LifecycleReclaimed (current-run result reclaimed after use)
 *   A — AgeCleared (old result cleared by age policy)
 *   X — OversizeCapped (oversized result capped)
 *   I — ImageStripped (old image stripped under severe pressure)
 *   T — TurnCollapsed (assistant/tool turn collapsed into summary)
 *   E — MessagesEvicted (messages evicted)
 */
const COMPACTION_METHOD_CHARS: Record<string, string> = {
  Outline: 'O',
  HeadTail: 'H',
  Summarized: 'T',
  TurnCollapsed: 'T',
  Dropped: 'E',
  MessagesEvicted: 'E',
  LifecycleCleared: 'L',
  LifecycleReclaimed: 'L',
  AgeCleared: 'A',
  OversizeCapped: 'X',
  ImageStripped: 'I',
}

const COMPACTION_METHOD_LEGEND: Record<string, string> = {
  O: 'Outline',
  H: 'HeadTail',
  T: 'TurnCollapsed',
  E: 'MessagesEvicted',
  L: 'LifecycleReclaimed',
  A: 'AgeCleared',
  X: 'OversizeCapped',
  I: 'ImageStripped',
}

/**
 * Render a position bar showing which messages were affected by compaction,
 * plus a legend line listing only the characters that actually appear.
 *
 * When beforeCount > WIDTH, large action blocks are sqrt-compressed so kept
 * ranges get enough slots to show their approximate position.
 *
 * Returns `{ bar, legend }` so the caller can place them independently.
 */
export interface CompactionAction {
  index?: number
  end_index?: number
  method?: string
  before_tokens?: number
  after_tokens?: number
  related_count?: number
  tool_name?: string
}

export function renderPositionBar(beforeCount: number, sortedActions: CompactionAction[], _level: number): { bar: string; legend: string } {
  const WIDTH = 40
  if (beforeCount === 0) return { bar: `[${'·'.repeat(WIDTH)}]`, legend: '·=unchanged/kept' }

  const slotCount = Math.min(WIDTH, beforeCount)
  const slots = new Array(slotCount).fill('·')

  if (beforeCount <= WIDTH) {
    // 1:1 mapping — each message gets its own slot
    for (const a of sortedActions) {
      const start = a.index ?? 0
      const end = a.end_index ?? start
      const method = a.method ?? ''
      const ch = COMPACTION_METHOD_CHARS[method] ?? '?'
      for (let i = start; i <= Math.min(end, slotCount - 1); i++) slots[i] = ch
    }
  } else {
    // Segment-based allocation: sqrt-compress action blocks, keep '·' linear
    // so kept ranges are clearly visible at their approximate position.
    type Seg = { ch: string; count: number }

    // Sort actions by index, build segments with gaps as kept ranges
    const byIdx = [...sortedActions]
      .map(a => ({
        s: a.index ?? 0,
        e: a.end_index ?? a.index ?? 0,
        ch: COMPACTION_METHOD_CHARS[a.method ?? ''] ?? '?',
      }))
      .sort((a, b) => a.s - b.s)

    const raw: Seg[] = []
    let cursor = 0
    for (const a of byIdx) {
      if (a.s > cursor) raw.push({ ch: '·', count: a.s - cursor })
      if (a.e >= cursor) {
        const start = Math.max(a.s, cursor)
        raw.push({ ch: a.ch, count: a.e - start + 1 })
        cursor = a.e + 1
      }
    }
    if (cursor < beforeCount) raw.push({ ch: '·', count: beforeCount - cursor })

    // Merge adjacent segments with same char
    const segs: Seg[] = []
    for (const s of raw) {
      const last = segs.length > 0 ? segs[segs.length - 1]! : null
      if (last && last.ch === s.ch) last.count += s.count
      else segs.push({ ...s })
    }

    // Weight: kept = linear count, action = sqrt (compresses large blocks)
    const weights = segs.map(s => s.ch === '·' ? s.count : Math.max(1, Math.ceil(Math.sqrt(s.count))))
    const totalWeight = weights.reduce((a, b) => a + b, 0)

    // Allocate slots proportionally (min 1 per segment)
    const alloc = weights.map(w => Math.max(1, Math.round(w / totalWeight * slotCount)))
    let sum = alloc.reduce((a, b) => a + b, 0)

    // Adjust to exactly slotCount
    const MAX_ADJUST = slotCount
    for (let iter = 0; iter < MAX_ADJUST && sum !== slotCount; iter++) {
      if (sum > slotCount) {
        // Shrink largest action segment (prefer non-kept, skip segments at 1)
        let best = -1
        for (let i = 0; i < alloc.length; i++) {
          if (alloc[i]! <= 1) continue
          if (best === -1) { best = i; continue }
          // Prefer shrinking action over kept
          if (segs[best]!.ch === '·' && segs[i]!.ch !== '·') { best = i; continue }
          if (segs[best]!.ch !== '·' && segs[i]!.ch === '·') continue
          if (alloc[i]! > alloc[best]!) best = i
        }
        if (best === -1) break
        alloc[best]!--
        sum--
      } else {
        // Grow: prefer kept segments
        let best = 0
        for (let i = 1; i < alloc.length; i++) {
          if (segs[i]!.ch === '·' && segs[best]!.ch !== '·') { best = i; continue }
          if (segs[i]!.ch !== '·' && segs[best]!.ch === '·') continue
          if (alloc[i]! > alloc[best]!) best = i
        }
        alloc[best]!++
        sum++
      }
    }

    // Fill bar from segments, embedding [N..] labels in large action blocks
    const barChars: string[] = []
    const usedChars = new Set<string>()
    let hasKept = false

    for (let i = 0; i < segs.length; i++) {
      const seg = segs[i]!
      const width = alloc[i]!

      if (seg.ch === '·') {
        hasKept = true
        for (let j = 0; j < width; j++) barChars.push('·')
        continue
      }

      usedChars.add(seg.ch)
      const label = `─${seg.count}─`
      const minWidth = label.length + 2 // at least 1 action char on each side

      if (seg.count > width && width >= minWidth) {
        const remaining = width - label.length
        const left = Math.ceil(remaining / 2)
        const right = remaining - left
        for (let j = 0; j < left; j++) barChars.push(seg.ch)
        for (const c of label) barChars.push(c)
        for (let j = 0; j < right; j++) barChars.push(seg.ch)
      } else {
        for (let j = 0; j < width; j++) barChars.push(seg.ch)
      }
    }

    const bar = `[${barChars.join('')}]`

    // Build legend from segments
    const legendParts: string[] = []
    if (hasKept) legendParts.push('·=unchanged/kept')
    for (const [ch, method] of Object.entries(COMPACTION_METHOD_LEGEND)) {
      if (usedChars.has(ch)) legendParts.push(`${ch}=${method}`)
    }
    const legend = legendParts.join('  ')

    return { bar, legend }
  }

  const bar = `[${slots.join('')}]`

  // Build legend from chars that actually appear in the bar
  const seen = new Set(slots)
  const legendParts: string[] = []
  if (seen.has('·')) legendParts.push('·=unchanged/kept')
  for (const [ch, method] of Object.entries(COMPACTION_METHOD_LEGEND)) {
    if (seen.has(ch)) legendParts.push(`${ch}=${method}`)
  }
  const legend = legendParts.join('  ')

  return { bar, legend }
}

export function truncate(s: string, max: number): string {
  const oneLine = s.replace(/\n/g, ' ').trim()
  if (oneLine.length <= max) return oneLine
  return oneLine.slice(0, max - 1) + '…'
}

export function truncateHeadTail(s: string, max: number): string {
  const SEP = ' ... '
  if (s.length <= max || max < SEP.length + 6) return truncate(s, max)
  const budget = max - SEP.length
  const headLen = Math.floor(budget / 2)
  const tailLen = budget - headLen
  return s.slice(0, headLen).trimEnd() + SEP + s.slice(s.length - tailLen).trimStart()
}

export function summarizeInline(value: string, maxChars: number): string {
  const collapsed = value.split(/\s+/).join(' ')
  return truncate(collapsed, maxChars)
}

/** Max visible columns for the first line of a collapsed bash command. */
const BASH_CMD_FIRST_LINE_MAX = 120

/** Shared expand/collapse copy for bash commands, tool results, and progress. */
export function expandLinesHint(n: number): string {
  return `(+${n} lines, ctrl+o to expand)`
}

export const COLLAPSE_HINT = '(ctrl+o to collapse)'

export interface BashCommandDisplay {
  /** Text after `⌘ bash  ` on the card header. Empty means header is just the tool name. */
  headline: string
  /** Extra indented lines under the header (expanded multi-line commands only). */
  detailLines: string[]
}

/**
 * Format a bash tool command for the tool card.
 *
 * Collapsed: keep short one-liners; multi-line / huge heredocs become
 * `first line … (+N lines, ctrl+o to expand)` so the transcript is not a
 * wrapped wall of text and the expand shortcut matches tool-result cards.
 * Expanded: multi-line commands are shown in full under the header (newlines
 * preserved), matching readable shell transcript style rather than flattening.
 */
export function formatBashCommandDisplay(command: string, expanded = false): BashCommandDisplay {
  const normalized = command.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  const trimmed = normalized.replace(/\n+$/, '')
  if (!trimmed) return { headline: '', detailLines: [] }

  const lines = trimmed.split('\n')
  const multi = lines.length > 1
  const first = (lines[0] ?? '').trimEnd()

  if (expanded && multi) {
    // Header carries the first line; remaining lines sit indented underneath.
    return {
      headline: first.trimEnd(),
      detailLines: lines.slice(1).map((line) => `  ${line}`),
    }
  }

  if (!multi) {
    const one = first.trim()
    if (one.length <= BASH_CMD_FIRST_LINE_MAX) return { headline: one, detailLines: [] }
    return { headline: `${one.slice(0, BASH_CMD_FIRST_LINE_MAX - 1)}…`, detailLines: [] }
  }

  // Collapsed multi-line: first non-empty-ish line + shared expand hint.
  const headRaw = first.trim() || lines.find((l) => l.trim())?.trim() || ''
  const head =
    headRaw.length <= BASH_CMD_FIRST_LINE_MAX
      ? headRaw
      : `${headRaw.slice(0, BASH_CMD_FIRST_LINE_MAX - 1)}…`
  return { headline: `${head} … ${expandLinesHint(lines.length)}`, detailLines: [] }
}

export function toolResultLines(content: string, isError: boolean, _toolName?: string, expanded?: boolean): string[] {
  const MAX_LINE_WIDTH = 256

  const capLine = (l: string) => l.length <= MAX_LINE_WIDTH ? l : truncateHeadTail(l, MAX_LINE_WIDTH)

  const summarize = (): string => {
    if (!content.trim()) {
      return isError ? 'tool returned an error' : 'completed'
    }
    return summarizeInline(content, 160)
  }

  const normalized = content.replace(/\r\n/g, '\n')
  if (normalized.includes('\n')) {
    const trimmed = normalized.replace(/\n+$/, '')
    if (!trimmed) return [summarize()]
    const allLines = trimmed.split('\n')
    if (expanded) return allLines.map(capLine)
    // Collapsed view: don't preview any content lines. A tool result (bash,
    // read, search, ...) is often long and noisy, so the default card shows
    // only a single hint with the full line count; ctrl+o expands it. A
    // single-line result has nothing to collapse, so it's shown inline.
    if (allLines.length > 1) {
      return [`... ${expandLinesHint(allLines.length)}`]
    }
    return allLines.map(capLine)
  }
  return [summarize()]
}
