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
const ENDPOINT_CHARS = 2

/**
 * Drop anything the terminal would act on instead of print.
 *
 * A value is arbitrary bytes the user pasted, and the mask puts two of its
 * characters into the transcript. `\x1b[31mSECRET\x1b[0m` masked to
 * `\x1b[******0m`, which is a live escape byte plus `0m` — enough to form a real
 * SGR sequence and restyle the row from inside a line that is supposed to be
 * inert text. Endpoints are taken from the printable remainder instead.
 */
function printableOnly(value: string): string {
  return value
    .replace(/\u001b(?:\[[0-9;?]*[ -/]*[@-~]|].*?(?:\u0007|\u001b\\)|.)/g, '')
    .replace(/[\u0000-\u001f\u007f-\u009f]/g, '')
}

function maskValue(value: string): string {
  // Code points, not UTF-16 units. `slice(-2)` cut an emoji's surrogate pair in
  // half and emitted a lone surrogate, which is not valid text.
  const safe = Array.from(printableOnly(value))
  if (safe.length < MIN_LENGTH_FOR_ENDS) return STARS
  return [
    safe.slice(0, ENDPOINT_CHARS).join(''),
    STARS,
    safe.slice(-ENDPOINT_CHARS).join(''),
  ].join('')
}

export function describe(value: string): string {
  const text = value ?? ''
  if (text.length === 0) return '(empty)'
  // Length counts what was stored, including any characters the mask withheld:
  // it answers "did my paste arrive whole", so it must not shrink with sanitizing.
  return `${maskValue(text)}  ${Array.from(text).length} chars`
}

/** Render an ISO timestamp as a bare date; falls back to '-' when absent. */
export function shortDate(updatedAt?: string): string {
  if (!updatedAt) return '-'
  const date = new Date(updatedAt)
  if (Number.isNaN(date.getTime())) return '-'
  return date.toISOString().slice(0, 10)
}
