/**
 * Describe a stored value without revealing any of it.
 *
 * Every variable is treated as a secret: these exist to inject credentials into
 * bash, and guessing which keys are sensitive from their names misses the ones
 * that matter most (MY_CONN, DB_URL).
 *
 * A trailing fragment is deliberately not shown. It is the convention for
 * opaque tokens, but these values are often URL-shaped, where the tail is the
 * warehouse or database name: it leaks characters while telling entries apart
 * no better than the key already does. The length is enough to confirm a value
 * was pasted whole; `--reveal` covers exact checks.
 */
export function describe(value: string): string {
  const text = value ?? ''
  if (text.length === 0) return '(empty)'
  return `${text.length} chars`
}

/** Render an ISO timestamp as a bare date; falls back to '-' when absent. */
export function shortDate(updatedAt?: string): string {
  if (!updatedAt) return '-'
  const date = new Date(updatedAt)
  if (Number.isNaN(date.getTime())) return '-'
  return date.toISOString().slice(0, 10)
}
