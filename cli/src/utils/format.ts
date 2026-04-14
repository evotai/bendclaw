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
