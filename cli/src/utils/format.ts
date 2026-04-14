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

/** Format ms dynamically: <1s shows as ms, ≥1s shows as seconds */
export function formatMsDynamic(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`
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
