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
 *   O — Outline (tree-sitter structural extraction)
 *   H — HeadTail (head + tail truncation)
 *   S — Summarized (turn summarized)
 *   D — Dropped (messages evicted)
 *   C — LifecycleCleared (current-run result cleared after use)
 *   A — AgeCleared (old result cleared by age policy)
 *   X — OversizeCapped (oversized result capped)
 *   K — Kept (retained message, used as default in L3)
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
 * Returns `{ bar, legend }` so the caller can place them independently.
 */
export function renderPositionBar(beforeCount: number, sortedActions: any[], level: number): { bar: string; legend: string } {
  const WIDTH = 40
  if (beforeCount === 0) return { bar: `[${'·'.repeat(WIDTH)}]`, legend: '' }

  const defaultChar = level === 3 ? 'K' : '·'
  const slotCount = Math.min(WIDTH, beforeCount)
  const slots = new Array(slotCount).fill(defaultChar)

  for (const a of sortedActions) {
    const start = (a.index as number) ?? 0
    const end = (a.end_index as number) ?? start
    const method = (a.method as string) ?? ''
    const ch = COMPACTION_METHOD_CHARS[method] ?? '?'

    if (beforeCount <= WIDTH) {
      for (let i = start; i <= Math.min(end, slotCount - 1); i++) slots[i] = ch
    } else {
      const map = (idx: number) => Math.floor(idx * slotCount / beforeCount)
      const s = map(start)
      const e = map(end)
      for (let i = s; i <= Math.min(e, slotCount - 1); i++) slots[i] = ch
    }
  }

  const bar = `[${slots.join('')}]`

  // Build legend from chars that actually appear in the bar
  const seen = new Set(slots)
  const legendParts: string[] = []
  if (seen.has('K')) legendParts.push('K=Kept')
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

  const capLine = (l: string) => truncateHeadTail(l, MAX_LINE_WIDTH)

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
      result.push(`... (+${omitted} lines)`)
      result.push(...allLines.slice(-TAIL_LINES).map(capLine))
      return result
    }
    return allLines.map(capLine)
  }
  return [summarize()]
}
