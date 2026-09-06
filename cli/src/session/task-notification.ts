/** Historical background notifications were stored as model-facing user text.
 * Recognize only the complete runtime envelope, not arbitrary XML mentions.
 * This projection is display-only; stored content and model input are untouched.
 */
export type ReplayTextPart = { kind: 'user' | 'task-notification'; text: string }

const envelope = /^<task-notification>\r?\n<task-id>([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})<\/task-id>\r?\n<status>(completed|failed|killed|timed_out)<\/status>\r?\n(?:<exit-code>(-?\d+)<\/exit-code>\r?\n)?(?:<output-truncated>true<\/output-truncated>\r?\n)?<summary>[^\r\n]*<\/summary>\r?\n<output-file>[^\r\n]+<\/output-file>\r?\n<\/task-notification>(?=\r?$)/gm

export function replayUserText(text: string): ReplayTextPart[] {
  const parts: ReplayTextPart[] = []
  let cursor = 0
  for (const match of text.matchAll(envelope)) {
    const start = match.index
    // A quoted code example is still the user's text. Do not reinterpret it.
    const before = text.slice(0, start)
    let fence: string | undefined
    for (const line of before.split('\n')) {
      const marker = /^\s{0,3}(`{3,}|~{3,})/.exec(line)?.[1]
      if (!marker) continue
      if (!fence) fence = marker
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = undefined
    }
    if (fence) continue
    if (start > cursor && text.slice(cursor, start).trim()) {
      parts.push({ kind: 'user', text: text.slice(cursor, start).trim() })
    }
    const id = match[1] ?? ''
    const status = match[2]
    const label = status === 'completed' ? '✓ Background task completed'
      : status === 'killed' ? '■ Background task cancelled'
      : status === 'timed_out' ? '✗ Background task timed out' : '✗ Background task failed'
    parts.push({ kind: 'task-notification', text: `${label} · ${id.slice(0, 8)}${match[3] !== undefined ? ` · exit ${match[3]}` : ''}` })
    cursor = start + match[0].length
  }
  if (cursor === 0) return [{ kind: 'user', text }]
  if (text.slice(cursor).trim()) parts.push({ kind: 'user', text: text.slice(cursor).trim() })
  return parts
}
