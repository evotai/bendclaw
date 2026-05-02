/**
 * Shared formatting utilities.
 */

import stringWidth from 'string-width'

export function padRight(s: string, n: number): string {
  const w = stringWidth(s)
  if (w > n) {
    let truncated = ''
    let tw = 0
    for (const ch of s) {
      const cw = stringWidth(ch)
      if (tw + cw > n - 1) break
      truncated += ch
      tw += cw
    }
    return truncated + '…'
  }
  return s + ' '.repeat(Math.max(0, n - w))
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
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return `${n}`
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

export function renderBar(value: number, max: number, width: number): string {
  if (max <= 0) return '░'.repeat(width)
  const filled = Math.round((value / max) * width)
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
 *   C — LifecycleCleared (current-run result cleared after use)
 *   A — AgeCleared (old result cleared by age policy)
 *   X — OversizeCapped (oversized result capped)
 */
const COMPACTION_METHOD_CHARS: Record<string, string> = {
  Outline: 'O',
  HeadTail: 'H',
  Summarized: 'S',
  Dropped: 'D',
  LifecycleCleared: 'C',
  AgeCleared: 'A',
  OversizeCapped: 'X',
}

/** Reverse lookup: char → method name */
const CHAR_TO_METHOD: Record<string, string> = Object.fromEntries(
  Object.entries(COMPACTION_METHOD_CHARS).map(([k, v]) => [v, k])
)

/**
 * Render a position bar showing which messages were affected by compaction,
 * plus a legend line listing only the characters that actually appear.
 *
 * When beforeCount > WIDTH, large action blocks are sqrt-compressed so kept
 * ranges get enough slots to show their approximate position.
 *
 * Returns `{ bar, legend }` so the caller can place them independently.
 */
export function renderPositionBar(beforeCount: number, sortedActions: any[], _level: number): { bar: string; legend: string } {
  const WIDTH = 40
  if (beforeCount === 0) return { bar: `[${'·'.repeat(WIDTH)}]`, legend: '·=unchanged/kept' }

  const slotCount = Math.min(WIDTH, beforeCount)
  const slots = new Array(slotCount).fill('·')

  if (beforeCount <= WIDTH) {
    // 1:1 mapping — each message gets its own slot
    for (const a of sortedActions) {
      const start = (a.index as number) ?? 0
      const end = (a.end_index as number) ?? start
      const method = (a.method as string) ?? ''
      const ch = COMPACTION_METHOD_CHARS[method] ?? '?'
      for (let i = start; i <= Math.min(end, slotCount - 1); i++) slots[i] = ch
    }
  } else {
    // Segment-based allocation: sqrt-compress action blocks, keep '·' linear
    // so kept ranges are clearly visible at their approximate position.
    type Seg = { ch: string; count: number }

    // Sort actions by index, build segments with gaps as kept ranges
    const byIdx = [...sortedActions]
      .map((a: any) => ({
        s: (a.index as number) ?? 0,
        e: (a.end_index as number) ?? (a.index as number) ?? 0,
        ch: COMPACTION_METHOD_CHARS[(a.method as string) ?? ''] ?? '?',
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
    for (const [method, ch] of Object.entries(COMPACTION_METHOD_CHARS)) {
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
  for (const [method, ch] of Object.entries(COMPACTION_METHOD_CHARS)) {
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

export function truncateResult(s: string, maxChars: number): string {
  const lines = s.split('\n')
  let result = ''
  for (const line of lines) {
    if (result.length + line.length > maxChars) {
      result += '…'
      break
    }
    if (result.length > 0) result += '\n'
    result += line
  }
  return result
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

export function toolResultLines(content: string, isError: boolean, _toolName?: string, expanded?: boolean): string[] {
  const TAIL_LINES = 5
  const MAX_LINE_WIDTH = 256

  const capLine = (l: string) => l.length <= MAX_LINE_WIDTH ? l : truncateHeadTail(l, MAX_LINE_WIDTH)

  const summarize = (): string => {
    if (!content.trim()) {
      return isError ? 'Result: tool returned an error' : 'Result: completed'
    }
    return `Result: ${summarizeInline(content, 160)}`
  }

  const normalized = content.replace(/\r\n/g, '\n')
  if (normalized.includes('\n')) {
    const trimmed = normalized.replace(/\n+$/, '')
    if (!trimmed) return [summarize()]
    const allLines = trimmed.split('\n')
    if (expanded) return allLines.map(capLine)
    if (allLines.length > TAIL_LINES) {
      const omitted = allLines.length - TAIL_LINES
      const result: string[] = []
      result.push(...allLines.slice(0, TAIL_LINES).map(capLine))
      result.push(`... (+${omitted} lines, ctrl+o to expand)`)
      return result
    }
    return allLines.map(capLine)
  }
  return [summarize()]
}
