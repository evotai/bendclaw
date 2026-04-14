/**
 * Shared formatting utilities.
 */

export function padRight(s: string, n: number): string {
  if (s.length > n) return s.slice(0, n - 1) + '…'
  return s + ' '.repeat(Math.max(0, n - s.length))
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

/** Truncate a long line keeping head and tail with " ... " in between. */
export function truncateHeadTail(s: string, max: number): string {
  const SEP = ' ... '
  if (s.length <= max || max < SEP.length + 6) return truncate(s, max)
  const budget = max - SEP.length
  const headLen = Math.floor(budget / 2)
  const tailLen = budget - headLen
  return s.slice(0, headLen).trimEnd() + SEP + s.slice(s.length - tailLen).trimStart()
}

/** Collapse whitespace and truncate to a single line summary. */
export function summarizeInline(value: string, maxChars: number): string {
  const collapsed = value.split(/\s+/).join(' ')
  return truncate(collapsed, maxChars)
}

/**
 * Format tool result content for display, matching Rust REPL style:
 * - Multi-line: head 5 lines + "... (N more lines)" + tail 3 lines
 * - Each line truncated at 256 chars
 * - Single-line: "Result: <summary>"
 */
export function toolResultLines(content: string, isError: boolean): string[] {
  const HEAD_LINES = 5
  const TAIL_LINES = 3
  const COMPACT_THRESHOLD = HEAD_LINES + TAIL_LINES + 2
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
    if (allLines.length > COMPACT_THRESHOLD) {
      const result: string[] = []
      result.push(...allLines.slice(0, HEAD_LINES).map(capLine))
      const omitted = allLines.length - HEAD_LINES - TAIL_LINES
      result.push(`... (${omitted} more lines)`)
      result.push(...allLines.slice(-TAIL_LINES).map(capLine))
      return result
    }
    return allLines.map(capLine)
  }
  return [summarize()]
}
