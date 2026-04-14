/**
 * Secret value masking — replaces known secret values in displayed output.
 * Ported from Rust cli/format.rs mask_secrets/mask_value.
 */

/**
 * Replace all known secret values in text with masked form.
 * Secrets are sorted by length descending so longer secrets mask first.
 */
export function maskSecrets(text: string, secrets: string[]): string {
  if (secrets.length === 0) return text

  const sorted = [...new Set(secrets.filter(s => s.length > 0))]
  sorted.sort((a, b) => b.length - a.length)

  let result = text
  for (const secret of sorted) {
    result = result.replaceAll(secret, maskValue(secret))
  }
  return result
}

/**
 * Mask a secret value: show first 2 and last 2 chars, stars in between.
 * Short values (<=5 chars) are fully masked.
 */
function maskValue(s: string): string {
  if (s.length <= 5) return '*'.repeat(s.length)
  const head = s.slice(0, 2)
  const tail = s.slice(-2)
  const mid = '*'.repeat(s.length - 4)
  return `${head}${mid}${tail}`
}
