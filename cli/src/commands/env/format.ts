/**
 * Describe a stored value without handing over enough of it to be used.
 *
 * Every variable is treated as a secret: these exist to inject credentials into
 * bash, and guessing which keys are sensitive from their names misses the ones
 * that matter most (MY_CONN, DB_URL).
 *
 * Two characters at each end make a value recognizable — you can tell which
 * token you pasted, and spot the wrong one — while the middle stays hidden. The
 * star run is a fixed width so it does not double as a length readout and so
 * the column aligns; the exact length is printed beside it, which is what
 * answers "did my paste get truncated".
 *
 * Short values show no ends at all. Below nine characters, four revealed
 * characters is most of the secret, and the ones that short are usually
 * passwords rather than tokens.
 */
const STARS = '*'.repeat(6)
const MIN_LENGTH_FOR_ENDS = 9

export function maskValue(value: string): string {
  const text = value ?? ''
  if (text.length === 0) return '(empty)'
  if (text.length < MIN_LENGTH_FOR_ENDS) return STARS
  return `${text.slice(0, 2)}${STARS}${text.slice(-2)}`
}

export function describe(value: string): string {
  const text = value ?? ''
  if (text.length === 0) return '(empty)'
  return `${maskValue(text)}  ${text.length} chars`
}

/** Render an ISO timestamp as a bare date; falls back to '-' when absent. */
export function shortDate(updatedAt?: string): string {
  if (!updatedAt) return '-'
  const date = new Date(updatedAt)
  if (Number.isNaN(date.getTime())) return '-'
  return date.toISOString().slice(0, 10)
}
